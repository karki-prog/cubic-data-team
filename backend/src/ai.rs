//! AI drafting for resume sections.
//!
//! Provider is env-driven, matching the scrapper's `llm_client.py` so the two
//! projects can be pointed at the same model:
//!
//! * `LLM_PROVIDER=ollama` (default) — `OLLAMA_HOST`, `OLLAMA_MODEL`
//! * `LLM_PROVIDER=openrouter` — `OPENROUTER_API_KEY`, `OPENROUTER_MODEL`
//! * `LLM_PROVIDER=anthropic` — `ANTHROPIC_API_KEY`, `ANTHROPIC_MODEL`
//! * `LLM_PROVIDER=claude-code` — the Claude Code CLI on this machine, using its
//!   own sign-in (no API key). `CLAUDE_CODE_BIN`, `CLAUDE_CODE_MODEL`. Local only.
//!
//! Ollama is the default because it runs locally: candidate resumes never leave
//! the machine, which matters more here than raw model quality.

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::admin::is_superadmin;
use crate::auth::session_from_headers;
use crate::AppState;

/// A shape the same section can be written in. Choosing one swaps the format
/// rules sent to the model; everything else about the section is unchanged.
#[derive(Serialize, Clone, Copy, PartialEq, Eq)]
pub struct Format {
    pub id: &'static str,
    pub label: &'static str,
    pub guidance: &'static str,
    /// A literal skeleton. Small models follow a shape they can copy far better
    /// than they follow a description of one.
    pub shape: &'static str,
}

/// The resume broken into the parts a superadmin edits independently.
#[derive(Serialize, Clone, Copy, PartialEq, Eq)]
pub struct Section {
    pub id: &'static str,
    pub label: &'static str,
    /// What the model is being asked to produce.
    pub brief: &'static str,
    /// How many separate entries this section holds (1 = a single block).
    pub entries: u8,
    /// Roughly how long each entry should run. Used when the section offers no
    /// format choice, or none was picked.
    pub guidance: &'static str,
    /// Selectable shapes. Empty means the section has one fixed shape.
    pub formats: &'static [Format],
    /// Each entry is one job: the panel asks for company, position, timeline and
    /// a bullet count, and those alone are enough to draft.
    pub role: bool,
    /// A fixed list to pick from instead of drafting. Empty for most sections.
    pub choices: &'static [Choice],
    /// Not shown in the panel's section list — used by a button instead.
    pub hidden: bool,
}

/// One pickable entry, e.g. a certification.
#[derive(Serialize, Clone, Copy, PartialEq, Eq)]
pub struct Choice {
    pub id: &'static str,
    pub name: &'static str,
    pub issuer: &'static str,
    /// Heading the panel groups entries under.
    pub group: &'static str,
    /// No longer offered to new candidates. Kept so people who already hold
    /// it can list it; the panel marks it.
    pub retired: bool,
}

const fn cert(
    id: &'static str,
    name: &'static str,
    issuer: &'static str,
    group: &'static str,
) -> Choice {
    Choice { id, name, issuer, group, retired: false }
}

/// Certifications relevant to data engineering roles.
pub const DATA_ENGINEERING_CERTS: &[Choice] = &[
    // AWS
    cert("aws-dea", "AWS Certified Data Engineer – Associate", "Amazon Web Services", "AWS"),
    cert("aws-saa", "AWS Certified Solutions Architect – Associate", "Amazon Web Services", "AWS"),
    cert("aws-sap", "AWS Certified Solutions Architect – Professional", "Amazon Web Services", "AWS"),
    cert("aws-dva", "AWS Certified Developer – Associate", "Amazon Web Services", "AWS"),
    cert("aws-clf", "AWS Certified Cloud Practitioner", "Amazon Web Services", "AWS"),
    Choice {
        id: "aws-das",
        name: "AWS Certified Data Analytics – Specialty",
        issuer: "Amazon Web Services",
        group: "AWS",
        retired: true,
    },
    // Microsoft
    cert("ms-dp700", "Microsoft Certified: Fabric Data Engineer Associate (DP-700)", "Microsoft", "Microsoft Azure & Fabric"),
    cert("ms-dp600", "Microsoft Certified: Fabric Analytics Engineer Associate (DP-600)", "Microsoft", "Microsoft Azure & Fabric"),
    Choice {
        id: "ms-dp203",
        name: "Microsoft Certified: Azure Data Engineer Associate (DP-203)",
        issuer: "Microsoft",
        group: "Microsoft Azure & Fabric",
        retired: true,
    },
    cert("ms-dp300", "Microsoft Certified: Azure Database Administrator Associate (DP-300)", "Microsoft", "Microsoft Azure & Fabric"),
    cert("ms-pl300", "Microsoft Certified: Power BI Data Analyst Associate (PL-300)", "Microsoft", "Microsoft Azure & Fabric"),
    cert("ms-dp900", "Microsoft Certified: Azure Data Fundamentals (DP-900)", "Microsoft", "Microsoft Azure & Fabric"),
    cert("ms-az305", "Microsoft Certified: Azure Solutions Architect Expert (AZ-305)", "Microsoft", "Microsoft Azure & Fabric"),
    cert("ms-az204", "Microsoft Certified: Azure Developer Associate (AZ-204)", "Microsoft", "Microsoft Azure & Fabric"),
    cert("ms-az104", "Microsoft Certified: Azure Administrator Associate (AZ-104)", "Microsoft", "Microsoft Azure & Fabric"),
    cert("ms-az900", "Microsoft Certified: Azure Fundamentals (AZ-900)", "Microsoft", "Microsoft Azure & Fabric"),
    // Google Cloud
    cert("gcp-pde", "Google Cloud Certified Professional Data Engineer", "Google Cloud", "Google Cloud"),
    cert("gcp-pca", "Google Cloud Certified Professional Cloud Architect", "Google Cloud", "Google Cloud"),
    cert("gcp-ace", "Google Cloud Certified Associate Cloud Engineer", "Google Cloud", "Google Cloud"),
    cert("gcp-adp", "Google Cloud Certified Associate Data Practitioner", "Google Cloud", "Google Cloud"),
    cert("gcp-cdl", "Google Cloud Certified Cloud Digital Leader", "Google Cloud", "Google Cloud"),
    // Databricks
    cert("dbx-dea", "Databricks Certified Data Engineer Associate", "Databricks", "Databricks"),
    cert("dbx-dep", "Databricks Certified Data Engineer Professional", "Databricks", "Databricks"),
    cert("dbx-spark", "Databricks Certified Associate Developer for Apache Spark", "Databricks", "Databricks"),
    cert("dbx-daa", "Databricks Certified Data Analyst Associate", "Databricks", "Databricks"),
    // Snowflake
    cert("snow-core", "SnowPro Core Certification", "Snowflake", "Snowflake"),
    cert("snow-de", "SnowPro Advanced: Data Engineer", "Snowflake", "Snowflake"),
    cert("snow-arch", "SnowPro Advanced: Architect", "Snowflake", "Snowflake"),
    cert("snow-assoc", "SnowPro Associate: Platform Certification", "Snowflake", "Snowflake"),
    cert("snow-da", "SnowPro Advanced: Data Analyst", "Snowflake", "Snowflake"),
    cert("snow-admin", "SnowPro Advanced: Administrator", "Snowflake", "Snowflake"),
    cert("snow-ds", "SnowPro Advanced: Data Scientist", "Snowflake", "Snowflake"),
    cert("snow-genai", "SnowPro Specialty: Gen AI", "Snowflake", "Snowflake"),
    // Streaming, orchestration, transformation
    cert("cfl-ccdak", "Confluent Certified Developer for Apache Kafka (CCDAK)", "Confluent", "Streaming, orchestration & modeling"),
    cert("cfl-ccaak", "Confluent Certified Administrator for Apache Kafka (CCAAK)", "Confluent", "Streaming, orchestration & modeling"),
    cert("astro-airflow", "Astronomer Certification for Apache Airflow Fundamentals", "Astronomer", "Streaming, orchestration & modeling"),
    cert("dbt-ae", "dbt Analytics Engineering Certification", "dbt Labs", "Streaming, orchestration & modeling"),
    cert("dbt-dev", "dbt Cloud Certified Developer", "dbt Labs", "Streaming, orchestration & modeling"),
    cert("spark-assoc", "Databricks Certified Associate Developer for Apache Spark 3", "Databricks", "Streaming, orchestration & modeling"),
    cert("astro-dag", "Astronomer Certification: DAG Authoring for Apache Airflow", "Astronomer", "Streaming, orchestration & modeling"),
    cert("cfl-flink", "Confluent Certified Developer for Apache Flink", "Confluent", "Streaming, orchestration & modeling"),
    // Infrastructure
    cert("hc-tf", "HashiCorp Certified: Terraform Associate", "HashiCorp", "Infrastructure & DevOps"),
    cert("cncf-cka", "Certified Kubernetes Administrator (CKA)", "The Linux Foundation", "Infrastructure & DevOps"),
    cert("cncf-ckad", "Certified Kubernetes Application Developer (CKAD)", "The Linux Foundation", "Infrastructure & DevOps"),
    cert("hc-vault", "HashiCorp Certified: Vault Associate", "HashiCorp", "Infrastructure & DevOps"),
    // Platforms and tools that show up on data engineering resumes
    cert("cldr-cdp", "Cloudera Data Platform Generalist", "Cloudera", "Data platforms & tools"),
    cert("ibm-de", "IBM Data Engineering Professional Certificate", "IBM", "Data platforms & tools"),
    cert("infa-cdi", "Informatica Cloud Data Integration Certified Professional", "Informatica", "Data platforms & tools"),
    cert("talend-di", "Talend Data Integration Certified Developer", "Talend (Qlik)", "Data platforms & tools"),
    cert("mongo-dev", "MongoDB Certified Associate Developer", "MongoDB", "Data platforms & tools"),
    cert("mongo-dba", "MongoDB Certified DBA Associate", "MongoDB", "Data platforms & tools"),
    cert("elastic-eng", "Elastic Certified Engineer", "Elastic", "Data platforms & tools"),
    cert("neo4j-pro", "Neo4j Certified Professional", "Neo4j", "Data platforms & tools"),
    cert("datastax-cass", "DataStax Apache Cassandra Developer Associate", "DataStax", "Data platforms & tools"),
    cert("oracle-sql", "Oracle Database SQL Certified Associate", "Oracle", "Data platforms & tools"),
    // Analytics / BI, often paired with a data engineering resume
    cert("tableau-da", "Tableau Certified Data Analyst", "Tableau (Salesforce)", "Analytics & BI"),
    cert("tableau-ca", "Tableau Desktop Specialist", "Tableau (Salesforce)", "Analytics & BI"),
    cert("alteryx-core", "Alteryx Designer Core Certification", "Alteryx", "Analytics & BI"),
    cert("sas-bdp", "SAS Certified Big Data Professional", "SAS", "Analytics & BI"),
    // Governance and data management
    cert("dama-cdmp", "Certified Data Management Professional (CDMP)", "DAMA International", "Governance & data management"),
    cert("iapp-cipp", "Certified Information Privacy Professional (CIPP/US)", "IAPP", "Governance & data management"),
    // AI and machine learning — increasingly expected on a data engineering resume
    cert("aws-mla", "AWS Certified Machine Learning Engineer – Associate", "Amazon Web Services", "AI & machine learning"),
    cert("aws-mls", "AWS Certified Machine Learning – Specialty", "Amazon Web Services", "AI & machine learning"),
    cert("ms-ai102", "Microsoft Certified: Azure AI Engineer Associate (AI-102)", "Microsoft", "AI & machine learning"),
    cert("ms-dp100", "Microsoft Certified: Azure Data Scientist Associate (DP-100)", "Microsoft", "AI & machine learning"),
    cert("ms-ai900", "Microsoft Certified: Azure AI Fundamentals (AI-900)", "Microsoft", "AI & machine learning"),
    cert("gcp-pmle", "Google Cloud Certified Professional Machine Learning Engineer", "Google Cloud", "AI & machine learning"),
    cert("gcp-genai", "Google Cloud Generative AI Leader", "Google Cloud", "AI & machine learning"),
    cert("dbx-genai", "Databricks Certified Generative AI Engineer Associate", "Databricks", "AI & machine learning"),
    cert("dbx-mla", "Databricks Certified Machine Learning Associate", "Databricks", "AI & machine learning"),
    cert("dbx-mlp", "Databricks Certified Machine Learning Professional", "Databricks", "AI & machine learning"),
    cert("nvidia-genl", "NVIDIA-Certified Associate: Generative AI LLMs", "NVIDIA", "AI & machine learning"),
    cert("ibm-ai", "IBM AI Engineering Professional Certificate", "IBM", "AI & machine learning"),
    cert("certnexus-caip", "Certified Artificial Intelligence Practitioner (CAIP)", "CertNexus", "AI & machine learning"),
];

/// How many bullets a role may ask for. Fewer reads thin for a senior profile;
/// more stops fitting on the page.
pub const MIN_POINTS: u8 = 10;
pub const MAX_POINTS: u8 = 18;
pub const DEFAULT_POINTS: u8 = 12;

/// Summary can be written three ways; the choice only changes the format rules.
const SUMMARY_FORMATS: &[Format] = &[
    Format {
        id: "one_paragraph",
        label: "One paragraph",
        guidance: "Return ONE paragraph of 2-3 sentences. No line breaks, no bullet points, third person without pronouns.",
        shape: "<one paragraph, 2-3 sentences, on a single line>",
    },
    Format {
        id: "two_paragraphs",
        label: "Two paragraphs",
        guidance: "Return EXACTLY two paragraphs separated by a single blank line. Each 2-3 sentences. No bullet points, third person without pronouns.",
        shape: "<paragraph one, 2-3 sentences>\n\n<paragraph two, 2-3 sentences>",
    },
    Format {
        id: "bullets",
        label: "Bullet points",
        guidance: "Return 3-5 bullet points, one per line, no leading dashes or numbers. Each a single line, third person without pronouns.",
        shape: "<bullet one>\n<bullet two>\n<bullet three>",
    },
];

pub const SECTIONS: &[Section] = &[
    Section {
        id: "overall",
        label: "Overall",
        brief: "the whole resume, rewritten end to end",
        entries: 1,
        guidance: "Return the complete resume as plain text with clear section headings. Keep it to one page of content.",
        formats: &[],
        role: false,
        choices: &[],
        hidden: false,
    },
    Section {
        id: "title",
        label: "Title",
        brief: "the headline under the candidate's name",
        entries: 1,
        guidance: "Return a single job title line, at most 8 words. No punctuation at the end.",
        formats: &[],
        role: false,
        choices: &[],
        hidden: false,
    },
    Section {
        id: "summary",
        label: "Summary",
        brief: "the short professional summary at the top",
        entries: 1,
        guidance: "Return 2-3 sentences, no bullet points, written in the third person without pronouns.",
        formats: SUMMARY_FORMATS,
        role: false,
        choices: &[],
        hidden: false,
    },
    Section {
        id: "tech_stack",
        label: "Tech stack",
        brief: "the technical skills section",
        entries: 1,
        guidance: "Return grouped skill lines like 'Languages: ...', 'Data & Cloud: ...', 'Tools: ...'. Comma separated, no prose. Keep every tool from the current text with its exact spelling — regroup and relabel only, never invent, rename or drop one, so the resume still matches an ATS keyword search.",
        formats: &[],
        role: false,
        choices: &[],
        hidden: false,
    },
    Section {
        id: "experience",
        label: "Professional experience",
        brief: "one role's bullet points",
        entries: 5,
        guidance: "Enter the company, position and timeline, pick 10-18 points, and draft.",
        formats: &[],
        role: true,
        choices: &[],
        hidden: false,
    },
    Section {
        id: "certifications",
        label: "Certifications",
        brief: "the certifications list",
        entries: 1,
        guidance: "Tick the certifications the candidate holds, add a year if you like, and insert them. Or describe changes below and draft.",
        formats: &[],
        role: false,
        choices: DATA_ENGINEERING_CERTS,
        hidden: false,
    },
    Section {
        id: "education",
        label: "Education",
        brief: "the education section",
        entries: 1,
        guidance: "Fill the degree and university below and it writes itself — location optional, no graduation year.",
        formats: &[],
        role: false,
        choices: &[],
        hidden: false,
    },
    TECH_SUGGEST,
];

/// Suggests further tools for the skills section. Hidden: the panel reaches it
/// through the "Suggest tools" button, not the section list.
const TECH_SUGGEST: Section = Section {
    id: "tech_suggest",
    label: "Skill suggestions",
    brief: "tools worth considering alongside the ones already listed",
    entries: 1,
    guidance: "Return ONLY tool names separated by commas, on one line. No headings, no numbering, no sentences, no explanation.",
    formats: &[],
    role: false,
    choices: &[],
    hidden: true,
};

pub fn section(id: &str) -> Option<&'static Section> {
    SECTIONS.iter().find(|s| s.id == id)
}

/// `GET /api/admin/ai/sections` — what the UI renders.
pub async fn sections(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(res) = gate(&state, &headers) {
        return res;
    }
    (
        StatusCode::OK,
        Json(json!({ "ok": true, "sections": SECTIONS, "provider": provider() })),
    )
        .into_response()
}

#[derive(Deserialize, Default)]
pub struct DraftRequest {
    /// Section id from `SECTIONS`.
    section: String,
    /// What the superadmin typed. Optional for role sections, where the role
    /// details are the instruction.
    #[serde(default)]
    prompt: String,
    /// Existing text for this section, so the model edits rather than invents.
    #[serde(default)]
    current: String,
    /// Optional wider context (the rest of the resume, a job description).
    #[serde(default)]
    context: String,
    /// Which entry of a multi-entry section (1-based). Ignored when entries = 1.
    #[serde(default)]
    entry: Option<u8>,
    /// Chosen format id, for sections that offer a choice.
    #[serde(default)]
    format: Option<String>,
    /// Role sections only: the job the bullets are for.
    #[serde(default)]
    company: String,
    #[serde(default)]
    position: String,
    /// Free text as it should read on the resume, e.g. "Jan 2021 – Present".
    #[serde(default)]
    timeline: String,
    /// Role sections only: bullet count, clamped to MIN_POINTS..=MAX_POINTS.
    #[serde(default)]
    points: Option<u8>,
}

impl DraftRequest {
    fn points(&self) -> u8 {
        self.points
            .unwrap_or(DEFAULT_POINTS)
            .clamp(MIN_POINTS, MAX_POINTS)
    }
}

/// Resolves the section and rejects requests that could not produce anything
/// useful. The message is safe to show in the panel.
fn validate(req: &DraftRequest) -> Result<&'static Section, &'static str> {
    let sec = section(&req.section).ok_or("Unknown resume section.")?;
    if sec.role {
        if req.company.trim().is_empty() {
            return Err("Enter the company name.");
        }
        if req.position.trim().is_empty() {
            return Err("Enter the position.");
        }
        if req.timeline.trim().is_empty() {
            return Err("Enter the timeline.");
        }
    } else if req.prompt.trim().is_empty() {
        return Err("Say what you want changed.");
    }
    Ok(sec)
}

/// `POST /api/admin/ai/draft`
pub async fn draft(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<DraftRequest>,
) -> Response {
    if let Err(res) = gate(&state, &headers) {
        return res;
    }
    let sec = match validate(&req) {
        Ok(sec) => sec,
        Err(msg) => return err(StatusCode::BAD_REQUEST, msg),
    };

    let full = build_prompt(sec, &req);
    match generate(&full).await {
        Ok(text) => {
            let text = if sec.role {
                clean_bullets(&text, req.points())
            } else {
                text.trim().to_string()
            };
            (
                StatusCode::OK,
                Json(json!({ "ok": true, "section": sec.id, "text": text })),
            )
                .into_response()
        }
        Err(msg) => err(StatusCode::BAD_GATEWAY, &msg),
    }
}

/// Models decorate bullets however they like ("- ", "•", "3.", "**") and
/// sometimes overshoot the count. The document wants plain lines, exactly `n`
/// of them at most — a short result is left short so the gap is visible.
pub fn clean_bullets(text: &str, n: u8) -> String {
    text.lines()
        .map(|l| {
            let l = l.trim();
            let l = l.trim_start_matches(|c: char| matches!(c, '-' | '*' | '•' | '–' | '·'));
            // "12. " / "12) " numbering — the space is what separates it from a
            // metric like "3.5x faster".
            let digits = l.chars().take_while(char::is_ascii_digit).count();
            let rest = &l[digits..];
            let l = if digits > 0
                && (rest.starts_with(". ") || rest.starts_with(") "))
            {
                &rest[2..]
            } else {
                l
            };
            l.trim().trim_matches('*').trim().to_string()
        })
        // A bullet never ends in a colon; a preamble ("Here are 12 bullets:") does.
        .filter(|l| !l.is_empty() && !l.ends_with(':'))
        .take(n as usize)
        .collect::<Vec<_>>()
        .join("\n")
}

/// The model is told to return only the section body — anything else has to be
/// stripped by hand before it can go into the document.
fn build_prompt(sec: &Section, req: &DraftRequest) -> String {
    if sec.role {
        return build_role_prompt(req);
    }
    let mut p = String::new();
    p.push_str("You are editing one section of a professional resume.\n\n");
    p.push_str(&format!("SECTION: {} — {}\n", sec.label, sec.brief));
    if sec.entries > 1 {
        if let Some(n) = req.entry {
            p.push_str(&format!("This is entry {n} of {}.\n", sec.entries));
        }
    }
    // A chosen format overrides the section default; an unknown id falls back
    // rather than silently sending no format rules at all.
    let chosen = req
        .format
        .as_deref()
        .and_then(|id| sec.formats.iter().find(|f| f.id == id));
    let rules = chosen.map(|f| f.guidance).unwrap_or(sec.guidance);

    if !req.current.trim().is_empty() {
        p.push_str("CURRENT TEXT (edit this, keep what still works):\n");
        p.push_str(req.current.trim());
        p.push_str("\n\n");
    }
    if !req.context.trim().is_empty() {
        p.push_str("CONTEXT (background only, do not copy verbatim):\n");
        p.push_str(req.context.trim());
        p.push_str("\n\n");
    }
    p.push_str("INSTRUCTION:\n");
    p.push_str(req.prompt.trim());

    // Format rules go LAST, immediately before generation. Stated early they
    // get ignored — the model anchors on the shape of the current text instead.
    p.push_str("\n\nREQUIRED OUTPUT FORMAT (this overrides the shape of the current text):\n");
    p.push_str(rules);
    if let Some(f) = chosen {
        p.push_str("\n\nMatch this shape exactly:\n");
        p.push_str(f.shape);
    }
    p.push_str(
        "\n\nReturn ONLY the replacement text. \
         No preamble, no explanation, no markdown fences, no section heading.",
    );
    p
}

/// One job's bullets, from company + position + timeline alone.
///
/// Follows the house formula from the retargeting prompt
/// (src/lib/content/resumeRetargetingPrompt.ts) so a drafted role reads like the
/// rest of a retargeted resume.
fn build_role_prompt(req: &DraftRequest) -> String {
    let n = req.points();
    let mut p = String::new();
    p.push_str(
        "You are an elite resume writer and senior hiring manager. Write the \
         achievement bullets for ONE role in the Professional Experience section \
         of a professional resume.\n\n",
    );
    p.push_str("ROLE:\n");
    p.push_str(&format!("Company: {}\n", req.company.trim()));
    p.push_str(&format!("Position: {}\n", req.position.trim()));
    p.push_str(&format!("Timeline: {}\n\n", req.timeline.trim()));

    if !req.current.trim().is_empty() {
        p.push_str("CURRENT BULLETS (keep the real facts, upgrade the wording):\n");
        p.push_str(req.current.trim());
        p.push_str("\n\n");
    }
    if !req.context.trim().is_empty() {
        p.push_str("CONTEXT (background only, do not copy verbatim):\n");
        p.push_str(req.context.trim());
        p.push_str("\n\n");
    }
    if !req.prompt.trim().is_empty() {
        p.push_str("EXTRA INSTRUCTION:\n");
        p.push_str(req.prompt.trim());
        p.push_str("\n\n");
    }

    p.push_str(
        "WRITING RULES:\n\
         - Each bullet follows: Action verb + what was built or owned + tech stack + scale + measurable business impact.\n\
         - Fit the work to what this company actually does and to the seniority of this position.\n\
         - Use only technologies that existed and were in common use during this timeline.\n\
         - Past tense, unless the timeline runs to Present — then present tense.\n\
         - Start every bullet with a different strong verb (Architected, Engineered, Led, Optimized, Automated, Migrated, Designed, Delivered...). Never 'Responsible for', 'Worked on' or 'Helped'.\n\
         - Include a concrete metric in most bullets (%, $, records/day, latency, hours saved, team size), kept realistic and interview-defensible.\n\
         - Cover a spread: architecture, delivery, performance and cost, reliability and quality, collaboration and leadership.\n\
         - Put the strongest achievements first. No two bullets may say the same thing.\n\
         - Each bullet is one sentence of 20-35 words. No first-person pronouns.\n\n",
    );

    // Format rules last, with a numbered skeleton: small models hit an exact
    // count far more reliably when they can see every slot.
    p.push_str(&format!(
        "REQUIRED OUTPUT FORMAT:\nReturn EXACTLY {n} bullets, one per line. \
         No leading dashes, bullets or numbers. No blank lines. No bold.\n\n\
         Match this shape exactly ({n} lines):\n"
    ));
    for i in 1..=n {
        p.push_str(&format!("<bullet {i} of {n}>\n"));
    }
    p.push_str(
        "\nReturn ONLY the bullets. \
         No preamble, no explanation, no markdown fences, no heading, no company or title line.",
    );
    p
}

// ————— Providers ——————————————————————————————————————————————————

pub fn provider() -> String {
    let raw = std::env::var("LLM_PROVIDER").unwrap_or_default();
    let raw = raw.trim().to_lowercase();
    match raw.as_str() {
        // The scrapper keeps this alias: "openai" historically meant OpenRouter.
        "openai" | "openrouter" => "openrouter".into(),
        "anthropic" => "anthropic".into(),
        "claude-code" | "claude_code" | "claude" => "claude-code".into(),
        _ => "ollama".into(),
    }
}

async fn generate(prompt: &str) -> Result<String, String> {
    match provider().as_str() {
        "openrouter" => gen_openrouter(prompt).await,
        "anthropic" => gen_anthropic(prompt).await,
        "claude-code" => {
            let mut text = String::new();
            let mut steps = claude_code_stream(prompt)?;
            use futures_util::StreamExt;
            while let Some(step) = steps.next().await {
                match step {
                    StreamStep::Text(t) => text.push_str(&t),
                    StreamStep::Failed(msg) => return Err(msg),
                    StreamStep::Done => break,
                    StreamStep::Skip => {}
                }
            }
            if text.trim().is_empty() {
                return Err("Claude Code returned no text.".into());
            }
            Ok(text)
        }
        _ => gen_ollama(prompt).await,
    }
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        // Local models on a small box are slow; a short timeout just produces
        // confusing failures.
        .timeout(std::time::Duration::from_secs(180))
        .build()
        .map_err(|e| format!("Could not build HTTP client: {e}"))
}

async fn gen_ollama(prompt: &str) -> Result<String, String> {
    let host = std::env::var("OLLAMA_HOST")
        .unwrap_or_else(|_| "http://127.0.0.1:11434".into())
        .trim_end_matches('/')
        .to_string();
    let model = std::env::var("OLLAMA_MODEL").unwrap_or_else(|_| "qwen2.5-coder:latest".into());

    let res = client()?
        .post(format!("{host}/api/generate"))
        .json(&json!({ "model": model, "prompt": prompt, "stream": false }))
        .send()
        .await
        .map_err(|e| format!("Ollama is not reachable at {host}: {e}"))?;
    if !res.status().is_success() {
        return Err(format!("Ollama returned {}", res.status()));
    }
    let body: serde_json::Value = res
        .json()
        .await
        .map_err(|e| format!("Ollama sent an unreadable reply: {e}"))?;
    body.get("response")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| "Ollama returned no text.".to_string())
}

async fn gen_openrouter(prompt: &str) -> Result<String, String> {
    let key = std::env::var("OPENROUTER_API_KEY").unwrap_or_default();
    if key.trim().is_empty() {
        return Err("OPENROUTER_API_KEY is not set.".into());
    }
    let model = std::env::var("OPENROUTER_MODEL")
        .unwrap_or_else(|_| "nvidia/nemotron-3-super-120b-a12b:free".into());
    let res = client()?
        .post("https://openrouter.ai/api/v1/chat/completions")
        .bearer_auth(key.trim())
        .json(&json!({
            "model": model,
            "messages": [{ "role": "user", "content": prompt }],
        }))
        .send()
        .await
        .map_err(|e| format!("OpenRouter request failed: {e}"))?;
    if !res.status().is_success() {
        return Err(format!("OpenRouter returned {}", res.status()));
    }
    let body: serde_json::Value = res
        .json()
        .await
        .map_err(|e| format!("OpenRouter sent an unreadable reply: {e}"))?;
    body["choices"][0]["message"]["content"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| "OpenRouter returned no text.".to_string())
}

/// `ANTHROPIC_MODEL` or Claude Opus 5.
fn anthropic_model() -> String {
    std::env::var("ANTHROPIC_MODEL")
        .ok()
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| "claude-opus-5".into())
}

/// A Messages API request, raw HTTP (there is no official Rust SDK).
///
/// `fallbacks: "default"` lets the API re-run a request its safety classifiers
/// decline on a fallback model inside the same call, instead of just stopping.
fn anthropic_request(prompt: &str, stream: bool) -> Result<reqwest::RequestBuilder, String> {
    let key = std::env::var("ANTHROPIC_API_KEY").unwrap_or_default();
    if key.trim().is_empty() {
        return Err("ANTHROPIC_API_KEY is not set.".into());
    }
    Ok(client()?
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", key.trim())
        .header("anthropic-version", "2023-06-01")
        .header("anthropic-beta", "server-side-fallback-2026-07-01")
        .json(&json!({
            "model": anthropic_model(),
            "max_tokens": 16000,
            "stream": stream,
            "fallbacks": "default",
            "messages": [{ "role": "user", "content": prompt }],
        })))
}

/// A non-2xx reply, with Anthropic's own message when it sent one.
async fn anthropic_error(res: reqwest::Response) -> String {
    let status = res.status();
    let detail = res
        .json::<serde_json::Value>()
        .await
        .ok()
        .and_then(|b| b["error"]["message"].as_str().map(str::to_string))
        .unwrap_or_default();
    match status.as_u16() {
        401 => "Anthropic rejected the API key (401). Check ANTHROPIC_API_KEY.".into(),
        _ if detail.is_empty() => format!("Anthropic returned {status}"),
        _ => format!("Anthropic returned {status}: {detail}"),
    }
}

/// The reply's text. Claude Opus 5 thinks by default, so the first content
/// block is usually a (hidden) thinking block — only `text` blocks are the answer.
fn anthropic_text(body: &serde_json::Value) -> Result<String, String> {
    if body["stop_reason"] == "refusal" {
        return Err("Claude declined to write this. Rephrase the request and try again.".into());
    }
    let text: String = body["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|b| b["type"] == "text")
        .filter_map(|b| b["text"].as_str())
        .collect();
    if text.trim().is_empty() {
        return Err("Anthropic returned no text.".into());
    }
    Ok(text)
}

async fn gen_anthropic(prompt: &str) -> Result<String, String> {
    let res = anthropic_request(prompt, false)?
        .send()
        .await
        .map_err(|e| format!("Anthropic request failed: {e}"))?;
    if !res.status().is_success() {
        return Err(anthropic_error(res).await);
    }
    let body: serde_json::Value = res
        .json()
        .await
        .map_err(|e| format!("Anthropic sent an unreadable reply: {e}"))?;
    anthropic_text(&body)
}

/// What one Anthropic stream event means for the panel.
#[derive(Debug, PartialEq)]
enum StreamStep {
    Text(String),
    Done,
    Failed(String),
    Skip,
}

/// Reads one SSE `data:` payload. Only `text_delta`s are forwarded; thinking
/// deltas and bookkeeping events are skipped.
fn anthropic_stream_step(data: &str) -> StreamStep {
    let Ok(ev) = serde_json::from_str::<serde_json::Value>(data) else {
        return StreamStep::Skip;
    };
    match ev["type"].as_str().unwrap_or("") {
        "content_block_delta" if ev["delta"]["type"] == "text_delta" => {
            StreamStep::Text(ev["delta"]["text"].as_str().unwrap_or("").to_string())
        }
        "message_delta" if ev["delta"]["stop_reason"] == "refusal" => StreamStep::Failed(
            "Claude declined to write this. Rephrase the request and try again.".into(),
        ),
        "message_stop" => StreamStep::Done,
        "error" => StreamStep::Failed(format!(
            "Anthropic stream error: {}",
            ev["error"]["message"].as_str().unwrap_or("unknown")
        )),
        _ => StreamStep::Skip,
    }
}

// ————— Claude Code CLI ————————————————————————————————————————————————

/// The `claude` binary: `CLAUDE_CODE_BIN`, else the usual install locations. A
/// server started from a GUI app often has no Homebrew directory on its PATH.
fn claude_code_bin() -> String {
    if let Ok(bin) = std::env::var("CLAUDE_CODE_BIN") {
        if !bin.trim().is_empty() {
            return bin.trim().to_string();
        }
    }
    let home = std::env::var("HOME").unwrap_or_default();
    [
        "/opt/homebrew/bin/claude".to_string(),
        "/usr/local/bin/claude".to_string(),
        format!("{home}/.local/bin/claude"),
        format!("{home}/.claude/local/claude"),
    ]
    .into_iter()
    .find(|p| std::path::Path::new(p).is_file())
    .unwrap_or_else(|| "claude".into())
}

/// Reads one line of `claude -p --output-format stream-json` output.
///
/// With `--include-partial-messages`, model events arrive wrapped as
/// `{"type":"stream_event","event":{…}}` — the same events the Messages API
/// streams — and the run ends with one `{"type":"result"}` line.
fn claude_code_step(line: &str) -> StreamStep {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
        return StreamStep::Skip;
    };
    match v["type"].as_str().unwrap_or("") {
        "stream_event" => match anthropic_stream_step(&v["event"].to_string()) {
            // The CLI's own result line is the real end of the run.
            StreamStep::Done => StreamStep::Skip,
            other => other,
        },
        "result" if v["is_error"] == true => {
            let msg = v["result"].as_str().unwrap_or("Claude Code failed.");
            if msg.contains("Not logged in") || msg.contains("/login") {
                StreamStep::Failed(
                    "Claude Code on this computer isn't signed in. Run `claude` in Terminal, type /login, then try again."
                        .into(),
                )
            } else {
                StreamStep::Failed(format!("Claude Code: {msg}"))
            }
        }
        "result" => StreamStep::Done,
        _ => StreamStep::Skip,
    }
}

/// Runs the prompt through the local Claude Code CLI and streams its steps.
///
/// It runs as a plain text generator: no tools, no MCP servers, no slash
/// commands, no saved session, in an empty scratch folder so it never reads
/// this repo's CLAUDE.md. Variables inherited from a Claude Code session that
/// launched this server are removed, so the CLI uses its own sign-in.
fn claude_code_stream(
    prompt: &str,
) -> Result<std::pin::Pin<Box<dyn futures_util::Stream<Item = StreamStep> + Send>>, String> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

    let dir = std::env::temp_dir().join("cubic-resume-ai");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Could not prepare a folder for Claude Code: {e}"))?;

    let mut cmd = tokio::process::Command::new(claude_code_bin());
    cmd.args([
        "-p",
        "--output-format",
        "stream-json",
        "--include-partial-messages",
        "--verbose",
        "--tools",
        "",
        "--strict-mcp-config",
        "--disable-slash-commands",
        "--no-session-persistence",
    ]);
    if let Ok(model) = std::env::var("CLAUDE_CODE_MODEL") {
        if !model.trim().is_empty() {
            cmd.args(["--model", model.trim()]);
        }
    }
    for (key, _) in std::env::vars() {
        if key.starts_with("CLAUDE") || key == "ANTHROPIC_BASE_URL" || key == "ANTHROPIC_API_KEY" {
            cmd.env_remove(key);
        }
    }
    cmd.current_dir(&dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);

    let mut child = cmd.spawn().map_err(|e| {
        format!("Could not start Claude Code ({}): {e}. Install it or set CLAUDE_CODE_BIN.", claude_code_bin())
    })?;
    let mut stdin = child.stdin.take().ok_or("Claude Code has no stdin.")?;
    let stdout = child.stdout.take().ok_or("Claude Code has no stdout.")?;
    let prompt = prompt.to_string();

    Ok(Box::pin(async_stream::stream! {
        // The prompt goes over stdin: resume context can outgrow argv limits.
        if let Err(e) = stdin.write_all(prompt.as_bytes()).await {
            yield StreamStep::Failed(format!("Could not send the prompt to Claude Code: {e}"));
            return;
        }
        drop(stdin);

        let mut lines = tokio::io::BufReader::new(stdout).lines();
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(300);
        loop {
            match tokio::time::timeout_at(deadline, lines.next_line()).await {
                Err(_) => {
                    yield StreamStep::Failed("Claude Code took longer than 5 minutes.".into());
                    return;
                }
                Ok(Ok(Some(line))) => match claude_code_step(&line) {
                    StreamStep::Skip => {}
                    StreamStep::Done => { yield StreamStep::Done; return; }
                    step @ StreamStep::Failed(_) => { yield step; return; }
                    step => yield step,
                },
                Ok(Ok(None)) => {
                    let status = child.wait().await.ok();
                    if status.is_some_and(|s| s.success()) {
                        yield StreamStep::Done;
                    } else {
                        yield StreamStep::Failed("Claude Code stopped without an answer.".into());
                    }
                    return;
                }
                Ok(Err(e)) => {
                    yield StreamStep::Failed(format!("Could not read Claude Code's output: {e}"));
                    return;
                }
            }
        }
    }))
}

// ————— Helpers ————————————————————————————————————————————————————

/// Session cookie *or* a plugin bearer token.
///
/// The ONLYOFFICE plugin cannot send our cookie — it lives in a cross-site
/// iframe and the cookie is SameSite=Lax — so it presents a scoped token
/// instead. See `admin::mint_plugin_token`.
fn gate(state: &AppState, headers: &HeaderMap) -> Result<(), Response> {
    if let Some(user) = session_from_headers(&state.cfg, headers) {
        if is_superadmin(&state.cfg, &user.email) {
            return Ok(());
        }
        return Err(err(StatusCode::FORBIDDEN, "Superadmin only."));
    }

    let bearer = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .unwrap_or("");
    if !bearer.is_empty() && crate::admin::verify_plugin_token(&state.cfg, bearer).is_some() {
        return Ok(());
    }
    Err(err(StatusCode::UNAUTHORIZED, "Sign in first."))
}

fn err(code: StatusCode, msg: &str) -> Response {
    (code, Json(json!({ "ok": false, "error": msg }))).into_response()
}

#[cfg(test)]
mod tests {
    use super::{build_prompt, clean_bullets, section, validate, DraftRequest, SECTIONS};

    #[test]
    fn summary_offers_the_three_shapes() {
        let f = section("summary").unwrap().formats;
        let ids: Vec<_> = f.iter().map(|x| x.id).collect();
        assert_eq!(ids, ["one_paragraph", "two_paragraphs", "bullets"]);
        // Sections without a choice must expose an empty list, not a stray one.
        assert!(section("title").unwrap().formats.is_empty());
    }

    #[test]
    fn chosen_format_replaces_the_default_rules() {
        let mut req = DraftRequest {
            section: "summary".into(),
            prompt: "Tighten it".into(),
            current: String::new(),
            context: String::new(),
            format: Some("bullets".into()),
            ..Default::default()
        };
        let p = build_prompt(section("summary").unwrap(), &req);
        assert!(p.contains("3-5 bullet points"));
        assert!(p.contains("<bullet one>"), "skeleton must be included");
        // Format rules must come after the instruction, not before it.
        assert!(p.find("REQUIRED OUTPUT FORMAT").unwrap() > p.find("INSTRUCTION:").unwrap());

        req.format = Some("two_paragraphs".into());
        assert!(build_prompt(section("summary").unwrap(), &req).contains("EXACTLY two paragraphs"));

        // Unknown id must fall back to the section default, not drop the rules.
        req.format = Some("nonsense".into());
        let p = build_prompt(section("summary").unwrap(), &req);
        assert!(p.contains("REQUIRED OUTPUT FORMAT"));
        assert!(p.contains(section("summary").unwrap().guidance));
    }

    #[test]
    fn every_section_the_ui_offers_is_resolvable() {
        for id in [
            "overall",
            "title",
            "summary",
            "tech_stack",
            "experience",
            "certifications",
            "education",
        ] {
            assert!(section(id).is_some(), "{id} missing from SECTIONS");
        }
        assert!(section("nope").is_none());
        assert_eq!(SECTIONS.len(), 8);
        // The suggestions section exists but stays out of the section list.
        assert!(section("tech_suggest").unwrap().hidden);
        assert_eq!(SECTIONS.iter().filter(|s| s.hidden).count(), 1);
    }

    #[test]
    fn certifications_offer_a_clean_pick_list() {
        let list = section("certifications").unwrap().choices;
        assert!(list.len() >= 40, "the pick list should cover the common certs");
        let mut ids: Vec<_> = list.iter().map(|c| c.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), list.len(), "choice ids must be unique");
        assert!(list.iter().all(|c| !c.name.is_empty() && !c.issuer.is_empty() && !c.group.is_empty()));
        // Groups stay contiguous: the panel prints a heading each time one changes.
        let mut seen: Vec<&str> = Vec::new();
        for c in list {
            if seen.last() != Some(&c.group) {
                assert!(!seen.contains(&c.group), "{} is split into two blocks", c.group);
                seen.push(c.group);
            }
        }
        // Only certifications have a pick list.
        assert!(SECTIONS.iter().filter(|s| s.id != "certifications").all(|s| s.choices.is_empty()));
    }

    #[test]
    fn experience_holds_five_entries() {
        assert_eq!(section("experience").unwrap().entries, 5);
    }

    /// The model must be told to return the body only — otherwise its preamble
    /// ends up pasted into the resume.
    #[test]
    fn prompt_pins_the_output_shape_and_carries_the_instruction() {
        let req = DraftRequest {
            section: "summary".into(),
            prompt: "Make it AWS focused".into(),
            current: "Existing summary.".into(),
            context: "Job description here.".into(),
            ..Default::default()
        };
        let p = build_prompt(section("summary").unwrap(), &req);
        assert!(p.contains("Make it AWS focused"));
        assert!(p.contains("Existing summary."));
        assert!(p.contains("Job description here."));
        assert!(p.contains("Return ONLY the replacement text"));
        assert!(p.contains("no markdown fences"));
    }

    #[test]
    fn claude_text_skips_the_thinking_block() {
        let body = serde_json::json!({
            "stop_reason": "end_turn",
            "content": [
                { "type": "thinking", "thinking": "" },
                { "type": "text", "text": "Architected a lakehouse" }
            ]
        });
        assert_eq!(super::anthropic_text(&body).unwrap(), "Architected a lakehouse");
        let refused = serde_json::json!({ "stop_reason": "refusal", "content": [] });
        assert!(super::anthropic_text(&refused).is_err());
    }

    #[test]
    fn claude_stream_forwards_only_text() {
        use super::{anthropic_stream_step as step, StreamStep};
        assert_eq!(
            step(r#"{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"Led"}}"#),
            StreamStep::Text("Led".into())
        );
        assert_eq!(
            step(r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"x"}}"#),
            StreamStep::Skip
        );
        assert_eq!(step(r#"{"type":"message_stop"}"#), StreamStep::Done);
        assert!(matches!(
            step(r#"{"type":"message_delta","delta":{"stop_reason":"refusal"}}"#),
            StreamStep::Failed(_)
        ));
        assert!(matches!(
            step(r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#),
            StreamStep::Failed(_)
        ));
    }

    #[test]
    fn claude_code_output_is_read_like_the_api_stream() {
        use super::{claude_code_step as step, StreamStep};
        assert_eq!(
            step(r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Engineered"}}}"#),
            StreamStep::Text("Engineered".into())
        );
        // The inner message_stop is not the end; the result line is.
        assert_eq!(step(r#"{"type":"stream_event","event":{"type":"message_stop"}}"#), StreamStep::Skip);
        assert_eq!(step(r#"{"type":"result","subtype":"success","is_error":false,"result":"x"}"#), StreamStep::Done);
        assert_eq!(step(r#"{"type":"system","subtype":"init"}"#), StreamStep::Skip);
        match step(r#"{"type":"result","is_error":true,"result":"Not logged in · Please run /login"}"#) {
            StreamStep::Failed(msg) => assert!(msg.contains("/login")),
            other => panic!("expected a sign-in error, got {other:?}"),
        }
    }

    fn role(points: Option<u8>) -> DraftRequest {
        DraftRequest {
            section: "experience".into(),
            company: "Capital One".into(),
            position: "Senior Data Engineer".into(),
            timeline: "Mar 2021 – Present".into(),
            entry: Some(2),
            points,
            ..Default::default()
        }
    }

    /// Company, position and timeline are the whole instruction for a role.
    #[test]
    fn role_needs_all_three_details_but_no_prompt() {
        assert!(validate(&role(None)).is_ok());
        for blank in ["company", "position", "timeline"] {
            let mut r = role(None);
            match blank {
                "company" => r.company = "  ".into(),
                "position" => r.position.clear(),
                _ => r.timeline.clear(),
            }
            assert!(validate(&r).is_err(), "{blank} must be required");
        }
        // Non-role sections still need a prompt.
        let r = DraftRequest { section: "summary".into(), ..Default::default() };
        assert_eq!(validate(&r).err(), Some("Say what you want changed."));
    }

    #[test]
    fn role_prompt_carries_the_role_and_an_exact_count() {
        let p = build_prompt(section("experience").unwrap(), &role(Some(15)));
        assert!(p.contains("Company: Capital One"));
        assert!(p.contains("Position: Senior Data Engineer"));
        assert!(p.contains("Timeline: Mar 2021 – Present"));
        assert!(p.contains("EXACTLY 15 bullets"));
        assert!(p.contains("<bullet 15 of 15>"));
        assert!(!p.contains("<bullet 16"));
        assert!(!p.contains("EXTRA INSTRUCTION"), "no empty instruction block");
        assert!(p.find("REQUIRED OUTPUT FORMAT").unwrap() > p.find("WRITING RULES").unwrap());
    }

    #[test]
    fn points_are_clamped_to_ten_through_eighteen() {
        assert_eq!(role(None).points(), 12);
        assert_eq!(role(Some(3)).points(), 10);
        assert_eq!(role(Some(18)).points(), 18);
        assert_eq!(role(Some(40)).points(), 18);
        let p = build_prompt(section("experience").unwrap(), &role(Some(99)));
        assert!(p.contains("EXACTLY 18 bullets"));
    }

    #[test]
    fn role_prompt_includes_optional_extras() {
        let mut r = role(None);
        r.prompt = "Lean into Spark".into();
        r.current = "Built ETL jobs.".into();
        let p = build_prompt(section("experience").unwrap(), &r);
        assert!(p.contains("EXTRA INSTRUCTION:\nLean into Spark"));
        assert!(p.contains("Built ETL jobs."));
    }

    #[test]
    fn bullets_are_stripped_of_decoration_and_capped() {
        let raw = "Here are your bullets:\n\n- Architected a lakehouse\n• Led a team\n\
                   3. Migrated Hive to Spark\n12) **Cut costs 30%**\n* Automated QA\nExtra line";
        assert_eq!(
            clean_bullets(raw, 10),
            "Architected a lakehouse\nLed a team\nMigrated Hive to Spark\nCut costs 30%\nAutomated QA\nExtra line"
        );
        assert_eq!(clean_bullets(raw, 2), "Architected a lakehouse\nLed a team");
        // A leading number that is a metric, not numbering, must survive.
        assert_eq!(clean_bullets("40% faster pipelines", 10), "40% faster pipelines");
        assert_eq!(clean_bullets("3.5x faster queries", 10), "3.5x faster queries");
    }
}

// ————— Streaming ——————————————————————————————————————————————————

/// `POST /api/admin/ai/draft/stream` — Server-Sent Events.
///
/// Local models take seconds; watching text appear is the difference between
/// "is this working?" and a usable tool. Events:
///
/// * `{"t": "..."}`     — a text delta, append it
/// * `{"done": true}`   — generation finished
/// * `{"error": "..."}` — failed; the message is safe to show
///
/// Ollama streams natively. The hosted providers are requested whole and sent as
/// a single delta, so the client needs no second code path.
pub async fn draft_stream(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<DraftRequest>,
) -> Response {
    if let Err(res) = gate(&state, &headers) {
        return res;
    }
    let sec = match validate(&req) {
        Ok(sec) => sec,
        Err(msg) => return err(StatusCode::BAD_REQUEST, msg),
    };
    let prompt = build_prompt(sec, &req);
    let provider = provider();

    let body = async_stream::stream! {
        if provider == "claude-code" {
            let mut steps = match claude_code_stream(&prompt) {
                Ok(s) => s,
                Err(e) => { yield Ok::<_, std::convert::Infallible>(sse(&json!({ "error": e }))); return; }
            };
            use futures_util::StreamExt;
            while let Some(step) = steps.next().await {
                match step {
                    StreamStep::Text(t) if !t.is_empty() => yield Ok(sse(&json!({ "t": t }))),
                    StreamStep::Done => { yield Ok(sse(&json!({ "done": true }))); return; }
                    StreamStep::Failed(msg) => { yield Ok(sse(&json!({ "error": msg }))); return; }
                    _ => {}
                }
            }
            yield Ok(sse(&json!({ "done": true })));
            return;
        }

        if provider == "anthropic" {
            let res = match anthropic_request(&prompt, true) {
                Ok(r) => r.send().await,
                Err(e) => { yield Ok::<_, std::convert::Infallible>(sse(&json!({ "error": e }))); return; }
            };
            let res = match res {
                Ok(r) if r.status().is_success() => r,
                Ok(r) => { yield Ok(sse(&json!({ "error": anthropic_error(r).await }))); return; }
                Err(e) => { yield Ok(sse(&json!({ "error": format!("Anthropic request failed: {e}") }))); return; }
            };
            use futures_util::StreamExt;
            let mut stream = res.bytes_stream();
            let mut buf = String::new();
            while let Some(chunk) = stream.next().await {
                let Ok(chunk) = chunk else {
                    yield Ok(sse(&json!({ "error": "Connection to Claude dropped." })));
                    return;
                };
                buf.push_str(&String::from_utf8_lossy(&chunk));
                // Whole lines only — an event can be split across reads.
                while let Some(nl) = buf.find('\n') {
                    let line: String = buf.drain(..=nl).collect();
                    let Some(data) = line.trim().strip_prefix("data:") else { continue };
                    match anthropic_stream_step(data.trim()) {
                        StreamStep::Text(t) if !t.is_empty() => yield Ok(sse(&json!({ "t": t }))),
                        StreamStep::Done => { yield Ok(sse(&json!({ "done": true }))); return; }
                        StreamStep::Failed(msg) => { yield Ok(sse(&json!({ "error": msg }))); return; }
                        _ => {}
                    }
                }
            }
            yield Ok(sse(&json!({ "done": true })));
            return;
        }

        if provider != "ollama" {
            match generate(&prompt).await {
                Ok(text) => {
                    yield Ok::<_, std::convert::Infallible>(sse(&json!({ "t": text })));
                    yield Ok(sse(&json!({ "done": true })));
                }
                Err(msg) => yield Ok(sse(&json!({ "error": msg }))),
            }
            return;
        }

        let host = std::env::var("OLLAMA_HOST")
            .unwrap_or_else(|_| "http://127.0.0.1:11434".into())
            .trim_end_matches('/')
            .to_string();
        let model = std::env::var("OLLAMA_MODEL").unwrap_or_else(|_| "qwen2.5-coder:latest".into());

        let client = match client() {
            Ok(c) => c,
            Err(e) => { yield Ok(sse(&json!({ "error": e }))); return; }
        };
        let res = client
            .post(format!("{host}/api/generate"))
            .json(&json!({ "model": model, "prompt": prompt, "stream": true }))
            .send()
            .await;
        let res = match res {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => { yield Ok(sse(&json!({ "error": format!("Ollama returned {}", r.status()) }))); return; }
            Err(e) => { yield Ok(sse(&json!({ "error": format!("Ollama is not reachable at {host}: {e}") }))); return; }
        };

        // NDJSON arrives in arbitrary chunks, so hold a buffer and only parse
        // whole lines — a naive per-chunk parse drops tokens split across reads.
        let mut stream = res.bytes_stream();
        let mut buf = String::new();
        use futures_util::StreamExt;
        while let Some(chunk) = stream.next().await {
            let Ok(chunk) = chunk else {
                yield Ok(sse(&json!({ "error": "Connection to the model dropped." })));
                return;
            };
            buf.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(nl) = buf.find('\n') {
                let line: String = buf.drain(..=nl).collect();
                let line = line.trim();
                if line.is_empty() { continue; }
                let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
                if let Some(t) = v.get("response").and_then(|r| r.as_str()) {
                    if !t.is_empty() {
                        yield Ok(sse(&json!({ "t": t })));
                    }
                }
                if v.get("done").and_then(|d| d.as_bool()).unwrap_or(false) {
                    yield Ok(sse(&json!({ "done": true })));
                    return;
                }
            }
        }
        yield Ok(sse(&json!({ "done": true })));
    };

    Response::builder()
        .status(StatusCode::OK)
        .header(axum::http::header::CONTENT_TYPE, "text/event-stream")
        .header(axum::http::header::CACHE_CONTROL, "no-cache")
        // Proxies that buffer would defeat the point of streaming.
        .header("X-Accel-Buffering", "no")
        .body(axum::body::Body::from_stream(body))
        .unwrap_or_else(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "Could not open the stream."))
}

fn sse(value: &serde_json::Value) -> bytes::Bytes {
    bytes::Bytes::from(format!("data: {value}\n\n"))
}











