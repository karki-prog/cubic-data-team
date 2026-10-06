//! Pull the real job title / company from an apply-page HTML snapshot.
//! URL path slugs are a last resort — Ashby/Paylocity/etc. hide the role in og:title.

use regex::Regex;
use serde_json::Value;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScrapedJob {
    pub company: String,
    pub job_title: String,
    pub location: String,
    pub salary: String,
}

pub fn should_scrape_url(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url.trim()) else {
        return false;
    };
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return false;
    }
    let host = parsed
        .host_str()
        .unwrap_or("")
        .trim_start_matches("www.")
        .to_lowercase();
    if host.is_empty() {
        return false;
    }
    let path = parsed.path().to_lowercase();
    // Search / listing pages, not a single posting.
    if host.contains("indeed.com") && path.starts_with("/jobs") {
        let q: std::collections::HashMap<_, _> = parsed.query_pairs().collect();
        if !q.contains_key("jk") && !q.contains_key("vjk") && !q.contains_key("advn") {
            return false;
        }
    }
    if path.contains("/jobs/search") || path.contains("/jobsearch") {
        return false;
    }
    true
}

pub fn extract_from_html(html: &str) -> ScrapedJob {
    let jsonld = json_ld_job(html);
    let og_title = meta_content(html, "og:title");
    let page_title = html_title(html);
    let og_site = meta_content(html, "og:site_name");

    let mut job_title = first_nonempty(&[
        &jsonld.job_title,
        &clean_page_title(&og_title),
        &clean_page_title(&page_title),
    ]);
    let mut company = first_nonempty(&[&jsonld.company, &og_site]);

    if job_title.is_empty() && company.is_empty() {
        return ScrapedJob::default();
    }

    // "Senior Analytics Engineer @ Aleph" / "Role | Company"
    if let Some((role, org)) = split_role_company(&job_title) {
        if company.is_empty() {
            company = org;
        }
        job_title = role;
    }

    if junk_page_title(&job_title) {
        job_title.clear();
    }
    if is_ats_brand_name(&company) {
        company.clear();
    }

    ScrapedJob {
        company: squash(&company),
        job_title: squash(&job_title),
        location: squash(&jsonld.location),
        salary: squash(&jsonld.salary),
    }
}

fn first_nonempty(parts: &[&str]) -> String {
    parts
        .iter()
        .map(|s| s.trim())
        .find(|s| !s.is_empty() && !junk_page_title(s))
        .unwrap_or("")
        .to_string()
}

fn squash(value: &str) -> String {
    decode_entities(value)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
}

fn junk_page_title(title: &str) -> bool {
    let t = title.trim().to_lowercase();
    t.is_empty()
        || t == "job posting"
        || t == "apply link"
        || t.starts_with("now hiring")
        || t.contains("just a moment")
        || t.contains("access denied")
        || t.contains("attention required")
        || t.contains("cloudflare")
        || t.contains("enable javascript")
        || (t.contains("jobs in ") && t.contains("indeed"))
}

fn is_ats_brand_name(name: &str) -> bool {
    matches!(
        name.to_lowercase().replace(['-', '_', ' '], "").as_str(),
        "greenhouse"
            | "lever"
            | "ashby"
            | "ashbyhq"
            | "smartrecruiters"
            | "workday"
            | "icims"
            | "workable"
            | "paylocity"
            | "indeed"
            | "linkedin"
            | "glassdoor"
            | "ziprecruiter"
    )
}

fn clean_page_title(raw: &str) -> String {
    let mut text = squash(raw);
    text = Regex::new(r"(?i)\s*[|\-–—]\s*(greenhouse|lever|ashby|ashbyhq|smartrecruiters|workday|icims|linkedin|indeed|glassdoor|ziprecruiter|workable|jobvite|bamboohr)\s*$")
        .unwrap()
        .replace(&text, "")
        .to_string();
    text = Regex::new(r"(?i)\s*[|\-–—]\s*(careers?|jobs?|job application)\s*$")
        .unwrap()
        .replace(&text, "")
        .to_string();
    squash(&text)
}

fn split_role_company(title: &str) -> Option<(String, String)> {
    let text = title.trim();
    if let Some((left, right)) = text.rsplit_once(" @ ") {
        let role = left.trim();
        let company = right.trim();
        if role.len() >= 4 && !company.is_empty() {
            return Some((role.to_string(), company.to_string()));
        }
    }
    let splitter = Regex::new(r"\s+[|–—]\s+").unwrap();
    let parts: Vec<&str> = splitter.split(text).collect();
    if parts.len() == 2 {
        let left = parts[0].trim();
        let right = parts[1].trim();
        if looks_like_role(left) && !looks_like_role(right) {
            return Some((left.to_string(), right.to_string()));
        }
        if looks_like_role(right) && !looks_like_role(left) {
            return Some((right.to_string(), left.to_string()));
        }
    }
    if let Some((left, right)) = text.rsplit_once(" - ") {
        let left = left.trim();
        let right = right.trim();
        if looks_like_role(right) && !looks_like_role(left) && right.len() >= 8 {
            return Some((right.to_string(), left.to_string()));
        }
        if looks_like_role(left) && !looks_like_role(right) && left.len() >= 8 {
            return Some((left.to_string(), right.to_string()));
        }
    }
    None
}

fn looks_like_role(text: &str) -> bool {
    Regex::new(r"(?i)\b(engineer|developer|analyst|scientist|manager|architect|administrator|consultant|specialist|lead|director|intern|coordinator|designer|product|data)\b")
        .unwrap()
        .is_match(text)
}

fn html_title(html: &str) -> String {
    Regex::new(r"(?is)<title[^>]*>(.*?)</title>")
        .unwrap()
        .captures(html)
        .map(|c| squash(&c[1]))
        .unwrap_or_default()
}

fn meta_content(html: &str, key: &str) -> String {
    let key_re = regex::escape(key);
    let patterns = [
        format!(
            r#"(?is)<meta[^>]+(?:property|name)=["']{key_re}["'][^>]+content=["']([^"']+)["']"#
        ),
        format!(
            r#"(?is)<meta[^>]+content=["']([^"']+)["'][^>]+(?:property|name)=["']{key_re}["']"#
        ),
    ];
    for pat in patterns {
        if let Some(caps) = Regex::new(&pat).ok().and_then(|re| re.captures(html)) {
            let value = squash(&caps[1]);
            if !value.is_empty() {
                return value;
            }
        }
    }
    String::new()
}

fn json_ld_job(html: &str) -> ScrapedJob {
    let re = Regex::new(r#"(?is)<script[^>]+type=["']application/ld\+json["'][^>]*>(.*?)</script>"#).unwrap();
    let mut best = ScrapedJob::default();
    for caps in re.captures_iter(html) {
        let raw = caps[1].trim();
        if raw.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(raw) else {
            continue;
        };
        let mut found = Vec::new();
        collect_job_postings(&value, &mut found);
        for job in found {
            if best.job_title.is_empty() && !job.job_title.is_empty() {
                best = job;
            }
        }
    }
    best
}

fn collect_job_postings(value: &Value, out: &mut Vec<ScrapedJob>) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_job_postings(item, out);
            }
        }
        Value::Object(map) => {
            if is_job_posting_type(map.get("@type")) {
                out.push(job_from_ld(map));
            }
            if let Some(graph) = map.get("@graph") {
                collect_job_postings(graph, out);
            }
            for (key, nested) in map {
                if key == "@graph" {
                    continue;
                }
                collect_job_postings(nested, out);
            }
        }
        _ => {}
    }
}

fn is_job_posting_type(value: Option<&Value>) -> bool {
    match value {
        Some(Value::String(s)) => s.eq_ignore_ascii_case("JobPosting"),
        Some(Value::Array(items)) => items.iter().any(|item| {
            item.as_str()
                .map(|s| s.eq_ignore_ascii_case("JobPosting"))
                .unwrap_or(false)
        }),
        _ => false,
    }
}

fn job_from_ld(map: &serde_json::Map<String, Value>) -> ScrapedJob {
    let job_title = string_field(map.get("title"));
    let company = org_name(map.get("hiringOrganization"));
    let location = location_from_ld(map.get("jobLocation"));
    let salary = salary_from_ld(map.get("baseSalary"));
    ScrapedJob {
        company,
        job_title,
        location,
        salary,
    }
}

fn string_field(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(s)) => squash(s),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

fn org_name(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(s)) => squash(s),
        Some(Value::Object(map)) => string_field(map.get("name")),
        Some(Value::Array(items)) => items
            .iter()
            .find_map(|item| {
                let name = org_name(Some(item));
                (!name.is_empty()).then_some(name)
            })
            .unwrap_or_default(),
        _ => String::new(),
    }
}

fn location_from_ld(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(s)) => squash(s),
        Some(Value::Object(map)) => {
            if let Some(addr) = map.get("address").and_then(|v| v.as_object()) {
                let city = string_field(addr.get("addressLocality"));
                let region = string_field(addr.get("addressRegion"));
                match (city.is_empty(), region.is_empty()) {
                    (false, false) => format!("{city}, {region}"),
                    (false, true) => city,
                    (true, false) => region,
                    (true, true) => string_field(map.get("name")),
                }
            } else {
                string_field(map.get("name"))
            }
        }
        Some(Value::Array(items)) => items
            .iter()
            .find_map(|item| {
                let loc = location_from_ld(Some(item));
                (!loc.is_empty()).then_some(loc)
            })
            .unwrap_or_default(),
        _ => String::new(),
    }
}

fn salary_from_ld(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(s)) => squash(s),
        Some(Value::Object(map)) => {
            if let Some(val) = map.get("value") {
                match val {
                    Value::String(s) => squash(s),
                    Value::Number(n) => n.to_string(),
                    Value::Object(inner) => {
                        let min = string_field(inner.get("minValue"));
                        let max = string_field(inner.get("maxValue"));
                        match (min.is_empty(), max.is_empty()) {
                            (false, false) => format!("{min}–{max}"),
                            (false, true) => min,
                            (true, false) => max,
                            _ => string_field(inner.get("value")),
                        }
                    }
                    _ => String::new(),
                }
            } else {
                string_field(map.get("currency"))
            }
        }
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ashby_og_title() {
        let html = r#"
            <title>Senior Analytics Engineer @ Aleph</title>
            <meta property="og:title" content="Senior Analytics Engineer">
            <script type="application/ld+json">
            {"@type":"JobPosting","title":"Senior Analytics Engineer","hiringOrganization":{"name":"Aleph"}}
            </script>
        "#;
        let job = extract_from_html(html);
        assert_eq!(job.job_title, "Senior Analytics Engineer");
        assert_eq!(job.company, "Aleph");
    }

    #[test]
    fn paylocity_company_dash_role() {
        let html = r#"
            <title>Evans Transportation Services, LLC - Data Engineer</title>
            <meta property="og:title" content="Evans Transportation Services, LLC - Data Engineer">
            <script type="application/ld+json">
            {"@type":"JobPosting","title":"Data Engineer","hiringOrganization":{"name":"Evans Transportation Services, LLC"}}
            </script>
        "#;
        let job = extract_from_html(html);
        assert_eq!(job.job_title, "Data Engineer");
        assert_eq!(job.company, "Evans Transportation Services, LLC");
    }

    #[test]
    fn strips_smartrecruiters_suffix() {
        let html = r#"<title>Nexthink Data Engineer | SmartRecruiters</title>
            <meta property="og:title" content="Data Engineer">"#;
        let job = extract_from_html(html);
        assert_eq!(job.job_title, "Data Engineer");
    }

    #[test]
    fn skips_indeed_search_listings() {
        assert!(!should_scrape_url(
            "https://www.indeed.com/jobs?q=data+engineer&l=United+States"
        ));
        assert!(should_scrape_url(
            "https://www.indeed.com/viewjob?jk=321ab464ff5c7185"
        ));
        assert!(should_scrape_url(
            "https://jobs.ashbyhq.com/aleph/a0c00505-eb26-4f61-ab30-062ff38cfef4"
        ));
    }
}
