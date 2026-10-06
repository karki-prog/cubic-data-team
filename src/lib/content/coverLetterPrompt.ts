/** FAANG-level cover letter prompt — copy into ChatGPT / Claude. */
export const COVER_LETTER_PROMPT = `# FAANG-Level Senior Data Engineer / Data Architect Cover Letter Prompt

## ROLE

Act as a world-class **FAANG hiring manager, technical recruiter, and professional cover letter writer** with deep expertise hiring **Senior Data Engineers, Lead Data Engineers, Data Architects, and Data Platform Engineers**.

Your task is to create a **high-impact, personalized, technically credible cover letter** based on my resume and the target job description.

## GOAL

Write a cover letter that:

* Immediately captures the hiring manager's attention.
* Positions me as a strong **Senior Data Engineer / Data Architect / Data Platform Engineering** candidate.
* Connects my actual experience directly to the requirements in the job description.
* Demonstrates **technical depth, architecture experience, ownership, scale, and measurable business impact**.
* Shows why my background is specifically relevant to this company and role.
* Uses relevant ATS keywords naturally without keyword stuffing.
* Sounds confident, experienced, and human rather than AI-generated or overly formal.


## WRITING STYLE — STRICT

The cover letter must:

* Be approximately **300–400 words**.
* Fit comfortably on **one page**.
* Use concise, natural, professional language.
* Avoid generic phrases such as:


  * "I am writing to express my interest..."
  * "I believe I would be a great fit..."
  * "I am excited to apply..."
  * "Please accept my application..."

* Avoid simply summarizing my resume.
* Avoid exaggerated claims or unsupported statements.
* Avoid excessive buzzwords and corporate jargon.
* Never fabricate experience, employers, technologies, certifications, projects, or accomplishments.


Start with a **strong opening hook** that immediately connects my background to the company's data engineering challenges or the role's most important requirements.

## CONTENT STRUCTURE

### Opening

Create a compelling opening that:

* Names the specific position and company.
* Establishes my years and level of relevant experience.
* Highlights 2–3 of the strongest technical areas that align with the job.
* Gives the hiring manager an immediate reason to continue reading.


Do not use a generic application opening.

### Technical Experience & Impact

Select the **2–3 strongest accomplishments from my resume** that directly match the job description.

Prioritize experience involving, where relevant:

* Cloud data architecture
* AWS, Azure, or GCP
* Apache Spark / PySpark
* Databricks
* Snowflake
* ETL / ELT
* Data Lakes / Lakehouse
* Data Warehousing
* Kafka / Kinesis / Event Hubs / Pub/Sub
* Airflow / dbt
* Python / SQL
* Batch and streaming architectures
* Distributed systems
* Data modeling
* CI/CD and Infrastructure as Code
* Data quality and observability
* Data governance and security
* Cloud migration and modernization
* Performance and cost optimization


For each selected accomplishment, clearly communicate:

**What I built or architected + technologies used + scale/complexity + measurable technical or business impact.**

Use metrics from my resume whenever available. **Do not invent metrics.** If the resume does not provide a metric, describe the impact credibly without manufacturing a number.

### Leadership & Architecture

Demonstrate that I operate beyond basic pipeline development by highlighting relevant examples of:

* Architecture and system-design decisions
* End-to-end ownership
* Platform scalability and reliability
* Technical leadership
* Engineering standards and best practices
* Cross-functional collaboration
* Security, governance, and maintainability
* Translating business requirements into scalable technical solutions


Only include these themes when supported by my resume.

### Why This Role

Explain specifically why my background aligns with this position.

Identify the **3–5 most important requirements in the job description** and naturally connect them to evidence from my resume.

Reference the company's technology, product, industry, mission, or engineering challenges **only when that information is provided in the job description or other supplied materials**. Do not invent company-specific details.

### Closing

End with a confident, concise closing that reinforces:

* My strongest alignment with the role.
* The value I could bring to the organization's data platform or engineering team.
* Interest in discussing the opportunity further.


Avoid desperate, overly enthusiastic, or generic closing language.

## JOB-DESCRIPTION OPTIMIZATION

Before writing the cover letter, internally analyze the job description and identify:

1. The most important technical requirements.
2. Required cloud platforms and technologies.
3. Architecture and system-design expectations.
4. Leadership and ownership expectations.
5. Business/domain requirements.
6. ATS keywords.
7. The problems the company appears to need this person to solve.

Then compare those requirements against my resume.

Prioritize **direct matches** and transferable experience.

Do not claim experience with a technology simply because it appears in the job description.

## CONTENT INTEGRITY

You MUST:

* Preserve my actual professional experience.
* Use only technologies supported by my resume.
* Preserve factual company names, titles, projects, education, and certifications.
* Never fabricate experience to match the job description.
* Never claim I used a technology professionally unless supported by my resume.
* Never invent company-specific knowledge.
* Keep every technical statement **credible and interview-defensible**.


When my experience does not exactly match a requirement, emphasize the **closest legitimate transferable experience** instead of pretending there is a direct match.

## PERSONALIZATION

The cover letter should feel specifically written for **this job**, not like a reusable template.

Naturally incorporate relevant terminology from the job description while keeping the writing conversational and readable.

The hiring manager should clearly understand:

**Why this candidate + why this role + what technical/business value this candidate can deliver.**

## INPUT

### Target Job Description

**[PASTE JOB DESCRIPTION HERE]**

### My Resume

**[PASTE RESUME HERE]**

## FINAL INSTRUCTION

Create a tailored, one-page cover letter for the position using the job description and my resume.

Prioritize the strongest overlaps between my experience and the employer's requirements. Focus on **architecture, technical ownership, scale, measurable impact, cloud/data platform expertise, and business outcomes**.

Make the letter sound like it was written by an experienced **Senior Data Engineer / Data Architect**, not by an AI or a generic professional writer.

Do not include explanations, analysis, match scores, recommendations, placeholders, or commentary before or after the cover letter.

**Return only the completed cover letter.**`;
