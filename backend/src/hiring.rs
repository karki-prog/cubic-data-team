use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::SystemTime;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{header, HeaderMap};
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::Mutex;
use url::Url;

use crate::auth::require_session;
use crate::error::AppError;
use crate::google::sheets::url_from_grid_cell;
use crate::jobs::assert_job_auth;
use crate::AppState;

const STORE_NAME: &str = "hiring-postings.json";
const HIRING_SHEET_NAME: &str = "Apply_Links ";
const HIRING_MAX_ROWS: i32 = 1000;
// Scraped job details (location / salary / real title) barely change once a
// posting is live, and nothing is re-scraping them on the Rust backend yet —
// so keep known-good page metadata for a good while rather than dropping it.
const PAGE_CACHE_TTL_MS: u64 = 60 * 24 * 60 * 60 * 1000;
const MISS_CACHE_TTL_MS: u64 = 2 * 60 * 60 * 1000;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JobPageMeta {
    #[serde(default)]
    company: String,
    #[serde(default)]
    job_title: String,
    #[serde(default)]
    location: String,
    #[serde(default)]
    salary: String,
    #[serde(default)]
    work_mode: String,
    #[serde(default)]
    source: String,
    #[serde(default)]
    fetched_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct HiringRow {
    title: String,
    #[serde(default)]
    url: String,
    #[serde(default, rename = "addedBy")]
    added_by: String,
}

#[derive(Clone)]
struct HiringSnapshot {
    file_mtime: Option<SystemTime>,
    body: Value,
}

#[derive(Clone)]
pub struct HiringCache {
    snapshot: Arc<Mutex<Option<HiringSnapshot>>>,
    pages: Arc<Mutex<HashMap<String, JobPageMeta>>>,
    meta_loaded: Arc<AtomicBool>,
    enriching: Arc<AtomicBool>,
}

impl HiringCache {
    pub fn new() -> Self {
        Self {
            snapshot: Arc::new(Mutex::new(None)),
            pages: Arc::new(Mutex::new(HashMap::new())),
            meta_loaded: Arc::new(AtomicBool::new(false)),
            enriching: Arc::new(AtomicBool::new(false)),
        }
    }
}

#[derive(Deserialize, Default)]
pub struct HiringPushBody {
    rows: Option<Vec<HiringRow>>,
    #[serde(default)]
    source: String,
}

const GENERIC_ROLES: &[&str] = &[
    "apply",
    "home",
    "job detail",
    "job posting",
    "opportunitydetail",
    "recruiting",
    "job openings",
    "careers home",
    "en us",
    "candidate",
];

fn title_case(text: &str) -> String {
    text.split_whitespace()
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => format!("{}{}", f.to_uppercase(), c.as_str().to_lowercase()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_opaque_segment(segment: &str) -> bool {
    let lowered = segment.to_lowercase();
    let compact: String = lowered.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    if regex::Regex::new(r"^[a-f0-9]{8}[- ]?[a-f0-9]{4}[- ]?[a-f0-9]{4}[- ]?[a-f0-9]{4}[- ]?[a-f0-9]{12}$")
        .unwrap()
        .is_match(&lowered)
    {
        return true;
    }
    if regex::Regex::new(r"^[a-f0-9-]{16,}$").unwrap().is_match(&lowered) {
        return true;
    }
    // Bare numeric / EIN-like IDs pulled from apply URLs ("272003049a", "40561234").
    if regex::Regex::new(r"^\d{5,}[a-z]{0,3}$").unwrap().is_match(&lowered) {
        return true;
    }
    if regex::Regex::new(r"^[a-z]{1,4}[a-f0-9]{20,}$").unwrap().is_match(&lowered) {
        return true;
    }
    if regex::Regex::new(r"^[a-f0-9]{1,4}\.[a-f0-9]{2,8}$")
        .unwrap()
        .is_match(&lowered)
    {
        return true;
    }
    if compact.len() >= 20 {
        let hexish = compact
            .chars()
            .filter(|c| matches!(c, '0'..='9' | 'a'..='f'))
            .count();
        if (hexish as f32) / (compact.len() as f32) >= 0.8 {
            return true;
        }
    }
    false
}

fn is_ats_brand(name: &str) -> bool {
    matches!(
        name.to_lowercase().replace(['-', '_', ' '], "").as_str(),
        "greenhouse"
            | "jobboards"
            | "lever"
            | "smartrecruiters"
            | "workday"
            | "myworkdayjobs"
            | "ultipro"
            | "dayforce"
            | "dayforcehcm"
            | "oracle"
            | "oraclecloud"
            | "successfactors"
            | "icims"
            | "adp"
            | "workable"
            | "rippling"
            | "ats"
            | "ashby"
            | "ashbyhq"
            | "jobvite"
            | "paylocity"
            | "taleo"
            | "eightfold"
            | "linkedin"
            | "indeed"
            | "myjobs"
            | "workforcenow"
            | "theladders"
            | "applytojob"
            | "recruiterflow"
            | "acquiretm"
            | "candidateportal"
            | "applylink"
            | "jobboard"
            | "recruiting2"
    )
}

fn looks_like_location(text: &str) -> bool {
    let t = text.trim();
    regex::Regex::new(r"(?i)^(remote|nationwide[- ]?remote|hybrid|onsite|united states|united kingdom)$")
        .unwrap()
        .is_match(t)
        || regex::Regex::new(r"(?i)[-,]\s*(al|ak|az|ar|ca|co|ct|dc|de|fl|ga|hi|ia|id|il|in|ks|ky|la|ma|md|me|mi|mn|mo|ms|mt|nc|nd|ne|nh|nj|nm|nv|ny|oh|ok|or|pa|ri|sc|sd|tn|tx|ut|va|vt|wa|wi|wv)$")
            .unwrap()
            .is_match(t)
        || regex::Regex::new(r"(?i)\b(united states|united kingdom)\b").unwrap().is_match(t)
}

fn strip_careers_suffix(name: &str) -> String {
    regex::Regex::new(r"(?i)[-_\s]*(careers?|jobs?|jobsite|recruiting|talent)$")
        .unwrap()
        .replace(name.trim(), "")
        .to_string()
}

fn strip_req_id(role: &str) -> String {
    let mut text = role.trim().to_string();
    text = regex::Regex::new(r"(?i)[-_\s]+(?:jr|rq|req|icp)?[-_\s]?[a-z]?\d{3,}$")
        .unwrap()
        .replace(&text, "")
        .to_string();
    text = regex::Regex::new(r"(?i)[-_\s]+(?:jr|rq|req|r)[-_\s]?\d{3,}$")
        .unwrap()
        .replace(&text, "")
        .to_string();
    text = regex::Regex::new(r"\s+[A-Za-z]{2}\s+\d{4,5}(?:-\d{4})?\s*$")
        .unwrap()
        .replace(&text, "")
        .to_string();
    text.trim().trim_matches(['-', '_', '—', '–', ',']).to_string()
}

fn pretty_label(raw: &str) -> String {
    let mut text = strip_careers_suffix(raw);
    text = text.replace(['-', '_'], " ");
    text = regex::Regex::new(r"(?i)^(\d{6,})[- ]+")
        .unwrap()
        .replace(&text, "")
        .to_string();
    text = squash(&text);
    if !text.contains(' ') {
        text = regex::Regex::new(r"(?i)^([a-z]{4,})\d+$")
            .unwrap()
            .replace(&text, "$1")
            .to_string();
    }
    title_case(&strip_req_id(&text))
}

fn skip_host_label(label: &str) -> bool {
    let l = label.to_lowercase();
    matches!(
        l.as_str(),
        "www"
            | "careers"
            | "career"
            | "jobs"
            | "job"
            | "apply"
            | "talent"
            | "recruiting"
            | "recruiting2"
            | "boards"
            | "board"
            | "job-boards"
            | "myworkdayjobs"
            | "greenhouse"
            | "lever"
            | "smartrecruiters"
            | "icims"
            | "successfactors"
            | "oraclecloud"
            | "ultipro"
            | "workable"
            | "eightfold"
            | "dayforcehcm"
            | "dayforce"
            | "rippling"
            | "ashbyhq"
            | "jobvite"
            | "paylocity"
            | "taleo"
            | "ats"
            | "myjobs"
            | "workforcenow"
            | "theladders"
            | "linkedin"
            | "indeed"
            | "applytojob"
            | "recruiterflow"
            | "acquiretm"
            | "ns2cloud"
            | "fa"
            | "ocs"
            | "hcmui"
            | "corporate"
            | "app"
            | "go"
            | "jobs2"
    ) || regex::Regex::new(r"^wd\d+$").unwrap().is_match(&l)
        || l.starts_with("saasfaprod")
        || l.starts_with("fa-")
}

fn company_from_host(host: &str) -> String {
    let lowered = host.to_lowercase();
    let clean = lowered.trim_start_matches("www.");
    let parts: Vec<&str> = clean.split('.').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return String::new();
    }
    let mut company = parts[0];
    if skip_host_label(company) && parts.len() > 1 {
        company = parts[1];
    }
    if skip_host_label(company) {
        return String::new();
    }
    let known = [
        ("jpmorgan", "JPMorgan"),
        ("jpmorganchase", "JPMorgan Chase"),
        ("citi", "Citi"),
        ("citigroup", "Citi"),
        ("bofa", "Bank of America"),
        ("wellsfargo", "Wells Fargo"),
        ("ibm", "IBM"),
        ("hsbc", "HSBC"),
    ];
    for (key, label) in known {
        if company == key {
            return label.into();
        }
    }
    pretty_label(company)
}

fn path_segments(path: &str) -> Vec<String> {
    path.split('/')
        .filter(|p| !p.is_empty())
        .map(|segment| {
            urlencoding::decode(segment)
                .unwrap_or(std::borrow::Cow::Borrowed(segment))
                .into_owned()
        })
        .collect()
}

fn skip_path_label(label: &str) -> bool {
    matches!(
        label.to_lowercase().as_str(),
        "job" | "jobs" | "careers" | "career" | "en" | "us" | "gb" | "en-us" | "en_us"
            | "apply" | "search" | "details" | "detail" | "position" | "opening" | "openings"
            | "requisition" | "home" | "candidate" | "recruiting" | "job-description"
            | "jobdescription" | "description" | "opportunitydetail" | "candidateportal"
            | "jobboard" | "cx" | "sites" | "hcmui" | "candidateexperience" | "job-details"
            | "recruitment" | "view" | "external" | "external_careers" | "opportunities"
            | "application" | "recruiting2"
    )
}

fn role_from_path(pathname: &str) -> String {
    let parts = path_segments(pathname);
    for segment in parts.iter().rev() {
        if skip_path_label(segment)
            || segment.chars().all(|c| c.is_ascii_digit())
            || is_opaque_segment(segment)
            || looks_like_location(segment)
        {
            continue;
        }
        let mut cleaned = regex::Regex::new(r"^\d{4,}[-_]")
            .unwrap()
            .replace(segment, "")
            .to_string();
        cleaned = regex::Regex::new(r"\.(html?|php|aspx?|ftl)$")
            .unwrap()
            .replace(&cleaned, "")
            .to_string();
        let pretty = pretty_label(&cleaned);
        if pretty.is_empty() || GENERIC_ROLES.contains(&pretty.to_lowercase().as_str()) {
            continue;
        }
        if is_opaque_segment(&pretty.replace(' ', "")) || junk_role(&pretty) {
            continue;
        }
        return pretty;
    }
    String::new()
}

fn query_val(url: &Url, key: &str) -> String {
    url.query_pairs()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v.into_owned())
        .unwrap_or_default()
}

fn first_company_seg(segs: &[String], skip: &[&str]) -> String {
    segs.iter()
        .find(|s| {
            let l = s.to_lowercase();
            !skip.iter().any(|x| x.eq_ignore_ascii_case(&l))
                && !s.chars().all(|c| c.is_ascii_digit())
                && !is_opaque_segment(s)
                && !skip_path_label(s)
        })
        .map(|s| pretty_label(s))
        .unwrap_or_default()
}

/// Which ATS / job board a posting lives on, from its URL host. Read fresh from
/// the sheet on every load — no caching. Falls back to the bare domain.
fn job_site_from_url(raw: &str) -> String {
    let Ok(url) = Url::parse(raw.trim()) else {
        return String::new();
    };
    let host = url
        .host_str()
        .unwrap_or("")
        .trim_start_matches("www.")
        .to_lowercase();
    if host.is_empty() {
        return String::new();
    }
    let table: &[(&str, &str)] = &[
        ("greenhouse.io", "Greenhouse"),
        ("boards.greenhouse", "Greenhouse"),
        ("lever.co", "Lever"),
        ("ashbyhq.com", "Ashby"),
        ("myworkdayjobs.com", "Workday"),
        ("myworkday.com", "Workday"),
        ("icims.com", "iCIMS"),
        ("smartrecruiters.com", "SmartRecruiters"),
        ("rippling.com", "Rippling"),
        ("jobvite.com", "Jobvite"),
        ("workable.com", "Workable"),
        ("breezy.hr", "Breezy"),
        ("bamboohr.com", "BambooHR"),
        ("recruitee.com", "Recruitee"),
        ("teamtailor.com", "Teamtailor"),
        ("jazzhr.com", "JazzHR"),
        ("applytojob.com", "JazzHR"),
        ("dayforce", "Dayforce"),
        ("successfactors", "SuccessFactors"),
        ("ns2cloud.com", "SuccessFactors"),
        ("taleo.net", "Taleo"),
        ("csod.com", "Cornerstone"),
        ("hr.cloud.sap", "SuccessFactors"),
        ("jobs.sap.com", "SAP"),
        ("oraclecloud.com", "Oracle"),
        ("brassring.com", "BrassRing"),
        ("paylocity.com", "Paylocity"),
        ("paycomonline.net", "Paycom"),
        ("ultipro.com", "UKG"),
        ("ukg.com", "UKG"),
        ("adp.com", "ADP"),
        ("workforcenow.adp.com", "ADP"),
        ("eightfold.ai", "Eightfold"),
        ("phenompeople.com", "Phenom"),
        ("avature.net", "Avature"),
        ("gr8people.com", "gr8people"),
        ("jobs.workablehr.com", "Workable"),
        ("linkedin.com", "LinkedIn"),
        ("indeed.com", "Indeed"),
        ("ziprecruiter.com", "ZipRecruiter"),
        ("glassdoor.com", "Glassdoor"),
        ("monster.com", "Monster"),
        ("dice.com", "Dice"),
        ("wellfound.com", "Wellfound"),
        ("angel.co", "Wellfound"),
        ("builtin.com", "Built In"),
    ];
    for (needle, label) in table {
        if host.contains(needle) {
            return (*label).to_string();
        }
    }
    // Fall back to the registrable-ish domain: last two labels.
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() >= 2 {
        let base = parts[parts.len() - 2];
        let mut c = base.chars();
        match c.next() {
            Some(first) => format!("{}{}", first.to_uppercase(), c.as_str()),
            None => host.clone(),
        }
    } else {
        host
    }
}

fn parse_apply_url(raw: &str) -> (String, String) {
    let Ok(url) = Url::parse(raw) else {
        return (String::new(), String::new());
    };
    let host = url
        .host_str()
        .unwrap_or("")
        .trim_start_matches("www.")
        .to_lowercase();
    let segs = path_segments(url.path());

    let from_greenhouse = host.contains("greenhouse.io");
    let from_lever = host.contains("lever.co");
    let from_smart = host.contains("smartrecruiters.com");
    let from_rippling = host.contains("rippling.com");
    let from_ashby = host.contains("ashbyhq.com");
    let from_dayforce = host.contains("dayforce");
    let from_successfactors = host.contains("successfactors") || host.contains("ns2cloud.com");
    let from_workday = host.contains("myworkdayjobs.com") || host.contains("myworkday.com");
    let from_icims = host.contains("icims.com");
    let from_oracle = host.contains("oraclecloud.com");
    let from_adp = host.contains("adp.com");
    let from_workable = host.contains("workable.com");
    let from_eightfold = host.contains("eightfold.ai");
    let from_jazz = host.contains("applytojob.com");
    let from_recruiterflow = host.contains("recruiterflow.com");
    let from_paylocity = host.contains("paylocity.com");
    let from_ultipro = host.contains("ultipro.com");

    let (mut company, mut role) = if from_greenhouse || from_lever || from_ashby || from_rippling
    {
        (first_company_seg(&segs, &[]), role_from_path(url.path()))
    } else if from_smart {
        let company = first_company_seg(&segs, &[]);
        let role = segs
            .last()
            .map(|s| pretty_label(s))
            .filter(|s| !is_opaque_segment(s) && !junk_role(s))
            .unwrap_or_default();
        (company, role)
    } else if from_dayforce {
        (
            first_company_seg(&segs, &["en-us", "en", "us", "jobs", "candidateportal"]),
            role_from_path(url.path()),
        )
    } else if from_successfactors {
        let q = query_val(&url, "company");
        (pretty_label(&q), String::new())
    } else if from_workday {
        (company_from_host(&host), role_from_path(url.path()))
    } else if from_icims || from_eightfold || from_jazz {
        (company_from_host(&host), role_from_path(url.path()))
    } else if from_oracle {
        let site = segs
            .iter()
            .skip_while(|s| !s.eq_ignore_ascii_case("sites"))
            .nth(1)
            .cloned()
            .unwrap_or_default();
        let company = if !site.is_empty() && !regex::Regex::new(r"(?i)^cx_?\d+$").unwrap().is_match(&site)
        {
            pretty_label(&site)
        } else {
            company_from_host(&host)
        };
        (company, role_from_path(url.path()))
    } else if from_adp && host.contains("myjobs") {
        (first_company_seg(&segs, &["cx", "job-details"]), String::new())
    } else if from_workable {
        let slug = segs.last().cloned().unwrap_or_default();
        let lower = slug.to_lowercase();
        if let Some(idx) = lower.rfind("-at-").or_else(|| lower.rfind("-in-")) {
            if lower[idx..].starts_with("-at-") {
                let company = pretty_label(&slug[idx + 4..]);
                let role = pretty_label(&slug[..idx]);
                (company, role)
            } else {
                (String::new(), pretty_label(&slug))
            }
        } else {
            (String::new(), pretty_label(&slug))
        }
    } else if from_recruiterflow {
        (first_company_seg(&segs, &["jobs"]), role_from_path(url.path()))
    } else if from_paylocity || from_ultipro {
        (String::new(), String::new())
    } else {
        (company_from_host(&host), role_from_path(url.path()))
    };

    if is_ats_brand(&company) {
        company.clear();
    }
    if is_ats_brand(&role) || is_opaque_segment(&role) || junk_role(&role) {
        role.clear();
    }
    (company, role)
}

fn label_from_apply_url(url: &str) -> (String, Option<String>) {
    let (company, role) = parse_apply_url(url);
    if !role.is_empty() {
        (role, if company.is_empty() { None } else { Some(company) })
    } else if !company.is_empty() {
        (String::new(), Some(company))
    } else {
        (String::new(), None)
    }
}

fn hiring_job_lines(title: &str, url: &str) -> (String, Option<String>) {
    let raw = title.trim();
    if raw.starts_with("http://") || raw.starts_with("https://") {
        return label_from_apply_url(if url.is_empty() { raw } else { url });
    }
    let parts: Vec<&str> = regex::Regex::new(r"\s+[—–-]\s+")
        .unwrap()
        .split(raw)
        .collect();
    if parts.len() >= 2 {
        let left = parts[0].trim();
        let right = parts[1..].join(" — ");
        let right = right.trim();
        let left_key = left.to_lowercase().replace(['-', ' ', '_'], "");
        let ats_left = matches!(
            left_key.as_str(),
            "ashbyhq"
                | "jobboards"
                | "greenhouse"
                | "lever"
                | "gem"
                | "ats"
                | "paylocity"
                | "workforcenow"
                | "icims"
                | "join"
                | "rippling"
        );
        if ats_left
            || GENERIC_ROLES.contains(&right.to_lowercase().as_str())
            || junk_role(right)
            || is_opaque_segment(right)
            || right.to_lowercase().ends_with("apply")
            || right.len() < 8
        {
            if !url.is_empty() {
                return label_from_apply_url(url);
            }
            return (left.to_string(), None);
        }
        return (right.to_string(), Some(left.to_string()));
    }
    match Url::parse(url) {
        Ok(u) => {
            let host = u.host_str().unwrap_or("").trim_start_matches("www.");
            (raw.to_string(), if host.is_empty() { None } else { Some(host.to_string()) })
        }
        Err(_) => (raw.to_string(), None),
    }
}

fn squash(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn clean_company(raw: &str) -> String {
    let mut text = squash(raw);
    text = regex::Regex::new(r"^\d{2}-\d{6,8}\s+")
        .unwrap()
        .replace(&text, "")
        .to_string();
    text = regex::Regex::new(r",?\s*National Association$")
        .unwrap()
        .replace(&text, ", NA")
        .to_string();
    squash(&text)
}

fn clean_role(raw: &str) -> String {
    let mut text = squash(raw);
    text = regex::Regex::new(r"(?i)\s*[|\-–—]\s*(careers?|jobs?|job search)\s*$")
        .unwrap()
        .replace(&text, "")
        .to_string();
    squash(&text)
}

fn junk_role(role: &str) -> bool {
    regex::Regex::new(r"(?i)^(apply(\s+now)?|home|opportunities|jobs?|job\s*posting|job\s*\d+|pages?|listing|(job|career)\s*search|search\s+jobs|(\w+\s+)?careers?(\s+(portal|page|site|home|center|centre))?|(career|job)\s+opportunities|view\s+job|job\s+details?|career\s*\d+|career\s*hcm\d+|cx\s*_?\d+|jobboard|application|candidateportal|recruiting\d*|opportunitydetail)$")
        .unwrap()
        .is_match(role)
        || regex::Regex::new(r"(?i)saasfaprod").unwrap().is_match(role)
}

fn fix_company_role(raw_company: &str, raw_role: &str) -> (String, String) {
    let mut company = clean_company(raw_company);
    let mut role = clean_role(raw_role);
    let role_word = regex::Regex::new(
        r"(?i)\b(engineer|developer|analyst|scientist|manager|architect|administrator|consultant|specialist|lead|director|intern)\b",
    )
    .unwrap();
    if role_word.is_match(&company) && !role_word.is_match(&role) && !role.is_empty() && !junk_role(&role)
    {
        let swapped = clean_company(&role);
        role = clean_role(&company);
        company = swapped;
    }
    if role.is_empty() || junk_role(&role) {
        role.clear();
    }
    (company, role)
}

fn clean_location(raw: &str) -> String {
    let text = squash(raw);
    if text.is_empty() {
        return String::new();
    }
    if text.len() <= 70 && text.split_whitespace().count() <= 10 {
        return text;
    }
    if text.split_whitespace().count() >= 14 || (text.len() > 90 && regex::Regex::new(r"(?i)\b(is|are|will|responsible|experience|position|candidate|team|role)\b").unwrap().is_match(&text)) {
        return String::new();
    }
    let first = text.split(" · ").next().unwrap_or("").trim();
    if first.len() > 58 {
        format!("{}…", &first[..55])
    } else {
        first.to_string()
    }
}

fn clean_salary(raw: &str) -> String {
    let text = squash(raw);
    if text.is_empty() || !text.chars().any(|c| ('1'..='9').contains(&c)) {
        return String::new();
    }
    text
}

fn is_day_header(text: &str) -> bool {
    regex::Regex::new(r"(?i)^(mon|tues|wednes|thurs|fri|satur|sun)day,\s+\d{1,2}\s+(jan|feb|mar|apr|may|jun|jul|aug|sep|oct|nov|dec)[a-z]*\s+20\d{2}(\s*\((today|yesterday)\))?$")
        .unwrap()
        .is_match(text.trim())
}

fn extract_url(formula: &str, formatted: &str) -> String {
    let title = strip_zwsp(formatted);
    // Empty / ZWSP placeholders keep leftover =HYPERLINK formulas after a
    // day rollover. Do not treat those as live postings.
    if title.is_empty() {
        let text = formatted.trim();
        if text.starts_with("http://") || text.starts_with("https://") {
            return text.to_string();
        }
        return String::new();
    }
    if let Some(caps) = regex::Regex::new(r#"(?i)HYPERLINK\s*\(\s*"([^"]+)""#)
        .unwrap()
        .captures(formula)
        .or_else(|| {
            regex::Regex::new(r"(?i)HYPERLINK\s*\(\s*'([^']+)'")
                .unwrap()
                .captures(formula)
        })
    {
        return caps[1].trim().to_string();
    }
    let text = formatted.trim();
    if text.starts_with("http://") || text.starts_with("https://") {
        text.to_string()
    } else if formula.trim().starts_with("http") {
        formula.trim().to_string()
    } else {
        String::new()
    }
}

fn store_path(state: &AppState) -> PathBuf {
    state.cfg.data_dir.join(STORE_NAME)
}

fn file_mtime(path: &PathBuf) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

async fn load_disk_cache(state: &AppState) {
    if state.hiring.meta_loaded.swap(true, Ordering::SeqCst) {
        return;
    }
    if let Some(body) = state.db.get_json("hiring-job-meta").await {
        if let Ok(map) = serde_json::from_value::<HashMap<String, JobPageMeta>>(body) {
            let now = chrono::Utc::now().timestamp_millis() as u64;
            let mut pages = state.hiring.pages.lock().await;
            for (url, entry) in map {
                let ttl = if entry.source == "page" {
                    PAGE_CACHE_TTL_MS
                } else {
                    MISS_CACHE_TTL_MS
                };
                if now.saturating_sub(entry.fetched_at) < ttl {
                    pages.insert(url, entry);
                }
            }
        }
        return;
    }
    let path = state.cfg.data_dir.join("hiring-job-meta-cache.json");
    if let Ok(raw) = tokio::fs::read(&path).await {
        if let Ok(map) = serde_json::from_slice::<HashMap<String, JobPageMeta>>(&raw) {
            let now = chrono::Utc::now().timestamp_millis() as u64;
            let mut pages = state.hiring.pages.lock().await;
            for (url, entry) in map {
                let ttl = if entry.source == "page" {
                    PAGE_CACHE_TTL_MS
                } else {
                    MISS_CACHE_TTL_MS
                };
                if now.saturating_sub(entry.fetched_at) < ttl {
                    pages.insert(url, entry);
                }
            }
        }
    }
}

fn meta_cache_path(state: &AppState) -> PathBuf {
    state.cfg.data_dir.join("hiring-job-meta-cache.json")
}

async fn persist_meta_cache(state: &AppState) {
    let pages = state.hiring.pages.lock().await.clone();
    if let Ok(body) = serde_json::to_value(&pages) {
        if state.db.is_connected() {
            let _ = state.db.put_json("hiring-job-meta", &body).await;
            return;
        }
        let path = meta_cache_path(state);
        if let Some(parent) = path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        if let Ok(raw) = serde_json::to_vec_pretty(&pages) {
            let _ = tokio::fs::write(&path, raw).await;
        }
    }
}

fn scrape_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(12))
        .redirect(reqwest::redirect::Policy::limited(6))
        .user_agent(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36",
        )
        .build()
        .expect("scrape http client")
}

fn urls_needing_scrape(urls: &[String], pages: &HashMap<String, JobPageMeta>) -> Vec<String> {
    let now = chrono::Utc::now().timestamp_millis() as u64;
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for raw in urls {
        let url = raw.trim();
        if url.is_empty() || !seen.insert(url.to_string()) {
            continue;
        }
        if !crate::job_page::should_scrape_url(url) {
            continue;
        }
        if let Some(hit) = pages.get(url) {
            let ttl = if hit.source == "page" {
                PAGE_CACHE_TTL_MS
            } else {
                MISS_CACHE_TTL_MS
            };
            if now.saturating_sub(hit.fetched_at) < ttl {
                continue;
            }
        }
        out.push(url.to_string());
        if out.len() >= 80 {
            break;
        }
    }
    out
}

fn urls_from_hiring_rows(rows: &[HiringRow]) -> Vec<String> {
    rows.iter()
        .map(|row| row.url.trim().to_string())
        .filter(|url| !url.is_empty())
        .collect()
}

fn urls_from_store_body(body: &Value) -> Vec<String> {
    body.get("rows")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|row| {
            row.get("url")
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string())
        })
        .filter(|url| !url.is_empty())
        .collect()
}

fn kick_title_enrichment(state: AppState, urls: Vec<String>) {
    if urls.is_empty() {
        return;
    }
    if state.hiring.enriching.swap(true, Ordering::SeqCst) {
        return;
    }
    tokio::spawn(async move {
        if let Err(err) = enrich_job_titles(state.clone(), urls).await {
            tracing::warn!(error = %err, "hiring title scrape failed");
        }
        state.hiring.enriching.store(false, Ordering::SeqCst);
    });
}

async fn enrich_job_titles(state: AppState, urls: Vec<String>) -> anyhow::Result<()> {
    load_disk_cache(&state).await;
    let wanted = {
        let pages = state.hiring.pages.lock().await;
        urls_needing_scrape(&urls, &pages)
    };
    if wanted.is_empty() {
        // Cache already has titles — still rebuild the public copy so the UI
        // picks up jobTitle fields that a previous snake_case load dropped.
        if let Some(body) = cached_body(&state).await {
            if let Some(rows) = body.get("rows").cloned() {
                if let Ok(rows) = serde_json::from_value::<Vec<HiringRow>>(rows) {
                    if !rows.is_empty() {
                        let _ = refresh_from_rows(&state, rows, "page-meta").await;
                    }
                }
            }
        }
        return Ok(());
    }
    let client = scrape_http_client();
    let mut changed = false;
    for url in wanted {
        let scraped = fetch_job_page(&client, &url).await;
        let now = chrono::Utc::now().timestamp_millis() as u64;
        let mut pages = state.hiring.pages.lock().await;
        if scraped.job_title.is_empty() && scraped.company.is_empty() {
            pages.insert(
                url,
                JobPageMeta {
                    company: String::new(),
                    job_title: String::new(),
                    location: String::new(),
                    salary: String::new(),
                    work_mode: String::new(),
                    source: "miss".into(),
                    fetched_at: now,
                },
            );
        } else {
            pages.insert(
                url,
                JobPageMeta {
                    company: scraped.company,
                    job_title: scraped.job_title,
                    location: scraped.location,
                    salary: scraped.salary,
                    work_mode: String::new(),
                    source: "page".into(),
                    fetched_at: now,
                },
            );
            changed = true;
        }
        drop(pages);
        tokio::time::sleep(std::time::Duration::from_millis(120)).await;
    }
    persist_meta_cache(&state).await;
    if changed {
        if let Some(body) = cached_body(&state).await {
            if let Some(rows) = body.get("rows").cloned() {
                if let Ok(rows) = serde_json::from_value::<Vec<HiringRow>>(rows) {
                    let _ = refresh_from_rows(&state, rows, "page-meta").await;
                }
            }
        }
    }
    Ok(())
}

async fn fetch_job_page(client: &reqwest::Client, url: &str) -> crate::job_page::ScrapedJob {
    let Ok(resp) = client
        .get(url)
        .header("Accept", "text/html,application/xhtml+xml")
        .header("Accept-Language", "en-US,en;q=0.9")
        .send()
        .await
    else {
        return crate::job_page::ScrapedJob::default();
    };
    if !resp.status().is_success() {
        return crate::job_page::ScrapedJob::default();
    }
    let Ok(bytes) = resp.bytes().await else {
        return crate::job_page::ScrapedJob::default();
    };
    let html = String::from_utf8_lossy(&bytes[..bytes.len().min(400_000)]);
    crate::job_page::extract_from_html(&html)
}

async fn meta_for_url(cache: &HiringCache, url: &str) -> (String, String, String, String, String) {
    let (url_company, url_role) = parse_apply_url(url);
    let pages = cache.pages.lock().await;
    if let Some(hit) = pages.get(url) {
        if hit.source == "page" {
            let company = if hit.company.trim().is_empty() {
                url_company
            } else {
                hit.company.clone()
            };
            let title = if hit.job_title.trim().is_empty() {
                url_role
            } else {
                hit.job_title.clone()
            };
            return (
                company,
                title,
                clean_location(&hit.location),
                clean_salary(&hit.salary),
                hit.work_mode.clone(),
            );
        }
    }
    drop(pages);
    (url_company, url_role, String::new(), String::new(), String::new())
}

fn normalize_row(row: HiringRow) -> Option<HiringRow> {
    let title = strip_zwsp(&row.title).trim_matches('\u{200b}').trim().to_string();
    if title.is_empty() || title.eq_ignore_ascii_case("Apply Link") {
        return None;
    }
    Some(HiringRow {
        title,
        url: row.url.trim().to_string(),
        added_by: row.added_by.trim().to_string(),
    })
}

async fn groups_from_rows(state: &AppState, rows: &[HiringRow]) -> (Vec<Value>, usize, String) {
    load_disk_cache(state).await;
    let mut groups: Vec<Value> = Vec::new();
    let mut current_jobs: Option<Vec<Value>> = None;
    let mut current_label = String::new();
    let mut fingerprint = Vec::new();
    let mut job_count = 0usize;

    for row in rows {
        if is_day_header(&row.title) {
            if let Some(jobs) = current_jobs.take() {
                groups.push(json!({ "label": current_label, "jobs": jobs }));
            }
            current_label = row.title.clone();
            current_jobs = Some(Vec::new());
            fingerprint.push(format!("D:{}", row.title));
            continue;
        }
        if row.url.is_empty() {
            continue;
        }
        if current_jobs.is_none() {
            current_label = "Latest postings".into();
            current_jobs = Some(Vec::new());
        }
        let (display_title, subtitle) = hiring_job_lines(&row.title, &row.url);
        let (url_company, url_role) = parse_apply_url(&row.url);
        let (c, t, l, s, w) = meta_for_url(&state.hiring, &row.url).await;
        let company_in = [c.as_str(), url_company.as_str(), subtitle.as_deref().unwrap_or("")]
            .into_iter()
            .find(|s| !s.is_empty() && !is_ats_brand(s))
            .unwrap_or("")
            .to_string();
        let role_in = [t.as_str(), display_title.as_str(), url_role.as_str()]
            .into_iter()
            .find(|s| {
                !s.is_empty()
                    && !is_ats_brand(s)
                    && !is_opaque_segment(s)
                    && !junk_role(s)
                    && !s.eq_ignore_ascii_case(&company_in)
            })
            .unwrap_or("")
            .to_string();
        let (company, job_title) = fix_company_role(&company_in, &role_in);
        let title = if !company.is_empty() && !job_title.is_empty() {
            format!("{company} — {job_title}")
        } else if !job_title.is_empty() {
            job_title.clone()
        } else {
            company.clone()
        };
        let display_title = if job_title.is_empty() {
            display_title
        } else {
            job_title.clone()
        };
        let subtitle = if company.is_empty() {
            subtitle
        } else {
            Some(company.clone())
        };
        if let Some(jobs) = current_jobs.as_mut() {
            jobs.push(json!({
                "title": title,
                "displayTitle": display_title,
                "displaySubtitle": subtitle,
                "url": row.url,
                "addedBy": row.added_by,
                "company": company,
                "jobTitle": job_title,
                "jobSite": job_site_from_url(&row.url),
                "location": clean_location(&l),
                "salary": clean_salary(&s),
                "workMode": w
            }));
            job_count += 1;
            fingerprint.push(format!("U:{}|{}|{}", row.url, row.added_by, job_title));
        }
    }
    if let Some(jobs) = current_jobs {
        groups.push(json!({ "label": current_label, "jobs": jobs }));
    }
    let signature = format!("{job_count}|{}", fingerprint.join(";"));
    (groups, job_count, signature)
}

fn strip_zwsp(text: &str) -> String {
    text.replace('\u{200b}', "").trim().to_string()
}

fn hiring_looks_like_poc(value: &str) -> bool {
    let key = value.trim().to_lowercase();
    let first = key.split_whitespace().next().unwrap_or("");
    matches!(first, "prasanna" | "sajit" | "saksham" | "shaksham")
}

/// 0-based index into a values row. New layout uses B (1); previous 3-col used C (2).
fn hiring_added_by_col_index(header: &[String]) -> usize {
    let b = header
        .get(1)
        .map(|s| s.trim().to_lowercase())
        .unwrap_or_default();
    if b == "job site" {
        2
    } else {
        1
    }
}

fn hiring_added_by_from_cells(cells: &[String]) -> String {
    hiring_added_by_from_cells_at(cells, 1)
}

fn hiring_added_by_from_cells_at(cells: &[String], added_by_idx: usize) -> String {
    let other_idx = if added_by_idx == 1 { 2 } else { 1 };
    let primary = cells.get(added_by_idx).map(|s| s.trim()).unwrap_or("");
    let other = cells.get(other_idx).map(|s| s.trim()).unwrap_or("");
    if hiring_looks_like_poc(primary) {
        return primary.to_string();
    }
    if hiring_looks_like_poc(other) {
        return other.to_string();
    }
    // New layout: B is Added By even when it is not a known POC name.
    // Never treat a leftover Job Site label as Added By.
    primary.to_string()
}

async fn fetch_hiring_rows(state: &AppState) -> anyhow::Result<Vec<HiringRow>> {
    let range = format!("'{HIRING_SHEET_NAME}'!A1:Z{HIRING_MAX_ROWS}");
    let formatted = state
        .google
        .sheets_values_get(
            &state.cfg.hiring_spreadsheet_id,
            &range,
            Some("FORMATTED_VALUE"),
        )
        .await?;
    let formulas = state
        .google
        .sheets_values_get(&state.cfg.hiring_spreadsheet_id, &range, Some("FORMULA"))
        .await?;
    let headers = crate::headers::SheetHeaders::detect(&formatted);
    let Some(link_idx) = headers.idx(crate::headers::col::APPLY_LINK) else {
        anyhow::bail!("Apply_Links header row has no Apply Link / URL column");
    };
    let added_by_idx = headers.idx(crate::headers::col::ADDED_BY);
    let row_count = formatted.len().max(formulas.len());
    let mut url_by_row = HashMap::new();
    let last = (row_count as i32 + 5).min(HIRING_MAX_ROWS);
    let link_letter = crate::headers::a1_col(link_idx);
    if let Ok(grid) = state
        .google
        .grid_data(
            &state.cfg.hiring_spreadsheet_id,
            &format!("'{HIRING_SHEET_NAME}'!{link_letter}1:{link_letter}{last}"),
            "sheets(data(rowData(values(formattedValue,hyperlink,userEnteredValue,textFormatRuns))))",
        )
        .await
    {
        for (i, row) in grid.iter().enumerate() {
            let cell = row.values.as_ref().and_then(|v| v.first());
            let entered = cell
                .and_then(|c| c.user_entered_value.as_ref())
                .and_then(|u| u.string_value.clone())
                .unwrap_or_default();
            if strip_zwsp(&entered).is_empty() {
                continue;
            }
            let url = url_from_grid_cell(cell);
            if !url.is_empty() {
                url_by_row.insert(i, url);
            }
        }
    }

    let mut rows = Vec::new();
    let header_idx = (headers.row_1 as usize).saturating_sub(1);
    for i in 0..row_count {
        if i == header_idx {
            continue;
        }
        let cells = formatted.get(i).cloned().unwrap_or_default();
        let title_raw = cells.get(link_idx).cloned().unwrap_or_default();
        let added_by = added_by_idx
            .map(|i| hiring_added_by_from_cells_at(&cells, i))
            .unwrap_or_default();
        let formula = formulas
            .get(i)
            .and_then(|r| r.get(link_idx))
            .cloned()
            .unwrap_or_default();
        let url = url_by_row
            .get(&i)
            .cloned()
            .unwrap_or_else(|| extract_url(&formula, &title_raw));
        if let Some(row) = normalize_row(HiringRow {
            title: title_raw,
            url,
            added_by,
        }) {
            rows.push(row);
        }
    }
    Ok(rows)
}

fn store_body(groups: Vec<Value>, job_count: usize, signature: String, source: &str, rows: &[HiringRow]) -> Value {
    json!({
        "ok": true,
        "source": source,
        "generatedAt": chrono::Utc::now().to_rfc3339(),
        "jobCount": job_count,
        "signature": signature,
        "groups": groups,
        "rows": rows,
    })
}

async fn write_store(state: &AppState, body: Value) -> anyhow::Result<Value> {
    if state.db.is_connected() {
        state.db.put_json("hiring-postings", &body).await?;
    } else {
        let path = store_path(state);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&path, serde_json::to_vec_pretty(&body)?).await?;
    }
    *state.hiring.snapshot.lock().await = Some(HiringSnapshot {
        file_mtime: None,
        body: body.clone(),
    });
    Ok(body)
}

fn read_store_file(path: &PathBuf) -> Option<(SystemTime, Value)> {
    let raw = std::fs::read(path).ok()?;
    let body: Value = serde_json::from_slice(&raw).ok()?;
    if body.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        return None;
    }
    if !body.get("groups").map(|g| g.is_array()).unwrap_or(false) {
        return None;
    }
    Some((file_mtime(path).unwrap_or(SystemTime::UNIX_EPOCH), body))
}

async fn cached_body(state: &AppState) -> Option<Value> {
    if let Some(body) = state.db.get_json("hiring-postings").await {
        if body.get("ok").and_then(|v| v.as_bool()) == Some(true) {
            *state.hiring.snapshot.lock().await = Some(HiringSnapshot {
                file_mtime: None,
                body: body.clone(),
            });
            return Some(body);
        }
    }
    {
        let snap = state.hiring.snapshot.lock().await;
        if let Some(current) = snap.as_ref() {
            return Some(current.body.clone());
        }
    }
    let path = store_path(state);
    if let Some((_mtime, body)) = read_store_file(&path) {
        *state.hiring.snapshot.lock().await = Some(HiringSnapshot {
            file_mtime: None,
            body: body.clone(),
        });
        return Some(body);
    }
    None
}

async fn refresh_from_rows(state: &AppState, rows: Vec<HiringRow>, source: &str) -> anyhow::Result<Value> {
    let rows: Vec<HiringRow> = rows.into_iter().filter_map(normalize_row).collect();
    let (groups, job_count, signature) = groups_from_rows(state, &rows).await;
    write_store(state, store_body(groups, job_count, signature, source, &rows)).await
}

pub(crate) async fn pull_and_store(state: &AppState, source: &str) -> anyhow::Result<Value> {
    let rows = fetch_hiring_rows(state).await?;
    let link_count = rows.iter().filter(|row| !row.url.trim().is_empty()).count();
    if link_count == 0 {
        if let Some(existing) = cached_body(state).await {
            let current = existing
                .get("jobCount")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            if current > 0 {
                tracing::warn!(
                    source,
                    "refusing empty Apply Links pull; keeping {current} cached posting(s)"
                );
                return Ok(existing);
            }
        }
    }
    refresh_from_rows(state, rows, source).await
}

fn response_json(body: Value) -> impl IntoResponse {
    let mut out = HeaderMap::new();
    out.insert(
        header::CACHE_CONTROL,
        "private, max-age=0, must-revalidate"
            .parse()
            .unwrap(),
    );
    (out, Json(body))
}

/// UI reads the Mongo snapshot of the sheet. The watcher keeps that copy current.
pub async fn job_postings(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, AppError> {
    require_session(&state.cfg, &headers)?;
    let body = if let Some(body) = cached_body(&state).await {
        body
    } else {
        pull_and_store(&state, "sheet-bootstrap")
            .await
            .map_err(AppError::from)?
    };
    kick_title_enrichment(state.clone(), urls_from_store_body(&body));
    Ok(response_json(body))
}

/// Sheet changed → ping here (empty body) or POST rows. Empty body re-reads the sheet.
pub async fn webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, AppError> {
    assert_job_auth(&state, &headers).await?;
    let parsed: HiringPushBody = if body.is_empty() {
        HiringPushBody::default()
    } else {
        serde_json::from_slice(&body).unwrap_or_default()
    };
    let source = if parsed.source.trim().is_empty() {
        "webhook"
    } else {
        parsed.source.trim()
    };
    let stored = match parsed.rows.filter(|rows| !rows.is_empty()) {
        Some(incoming) => {
            let urls = urls_from_hiring_rows(&incoming);
            let stored = refresh_from_rows(&state, incoming, source)
                .await
                .map_err(AppError::from)?;
            kick_title_enrichment(state.clone(), urls);
            stored
        }
        None => {
            let stored = pull_and_store(&state, source)
                .await
                .map_err(AppError::from)?;
            kick_title_enrichment(state.clone(), urls_from_store_body(&stored));
            stored
        }
    };
    Ok(Json(json!({
        "ok": true,
        "updated": true,
        "jobCount": stored.get("jobCount"),
        "generatedAt": stored.get("generatedAt"),
        "source": stored.get("source"),
    })))
}

/// Manual / cron: pull Apply_Links once and replace the local copy.
pub async fn sync(State(state): State<AppState>, headers: HeaderMap) -> Result<impl IntoResponse, AppError> {
    assert_job_auth(&state, &headers).await?;
    let stored = pull_and_store(&state, "sheet-sync")
        .await
        .map_err(AppError::from)?;
    kick_title_enrichment(state.clone(), urls_from_store_body(&stored));
    Ok(Json(json!({
        "ok": true,
        "updated": true,
        "jobCount": stored.get("jobCount"),
        "generatedAt": stored.get("generatedAt"),
        "source": stored.get("source"),
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn added_by_col_follows_header() {
        let new_layout = vec![
            "Apply Link".into(),
            "Added By".into(),
            "Job Site".into(),
        ];
        let old_layout = vec![
            "Apply Link".into(),
            "Job Site".into(),
            "Added By".into(),
        ];
        assert_eq!(hiring_added_by_col_index(&new_layout), 1);
        assert_eq!(hiring_added_by_col_index(&old_layout), 2);
        assert_eq!(hiring_added_by_col_index(&[]), 1);
    }

    #[test]
    fn added_by_from_new_layout_row() {
        let cells = vec!["Aleph".into(), "Sajit".into(), "Ashby".into()];
        assert_eq!(hiring_added_by_from_cells_at(&cells, 1), "Sajit");
    }

    #[test]
    fn added_by_from_old_layout_row() {
        let cells = vec!["Aleph".into(), "Ashby".into(), "Sajit".into()];
        assert_eq!(hiring_added_by_from_cells_at(&cells, 2), "Sajit");
        assert_eq!(hiring_added_by_from_cells(&cells), "Sajit");
    }

    #[test]
    fn blank_added_by_does_not_use_job_site() {
        let cells = vec!["Aleph".into(), "".into(), "Ashby".into()];
        assert_eq!(hiring_added_by_from_cells_at(&cells, 1), "");
    }

    #[test]
    fn job_page_meta_reads_camel_case_cache() {
        let raw = r#"{
            "company": "Aleph",
            "jobTitle": "Senior Analytics Engineer",
            "location": "Remote",
            "salary": "",
            "workMode": "",
            "source": "page",
            "fetchedAt": 1787331130411
        }"#;
        let meta: JobPageMeta = serde_json::from_str(raw).unwrap();
        assert_eq!(meta.job_title, "Senior Analytics Engineer");
        assert_eq!(meta.company, "Aleph");
        assert_eq!(meta.fetched_at, 1787331130411);
        let dumped = serde_json::to_value(&meta).unwrap();
        assert_eq!(dumped["jobTitle"], "Senior Analytics Engineer");
    }
}
