use crate::config::Config;

fn staff_key(text: &str) -> String {
    text.trim().to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn resolve_support_email(cfg: &Config, name: &str) -> String {
    let key = staff_key(name);
    if key.is_empty() || key == "self" {
        return String::new();
    }
    let aliases: &[(&str, &str)] = &[
        ("iamshreya", "IAMShreya"),
        ("saksham karki", "Saksham"),
        ("shaksham karki", "Saksham"),
        ("shaksham", "Saksham"),
        ("saksham - nt", "Saksham"),
        ("sakcham nt", "Saksham"),
        ("sakshyam", "Saksham"),
        ("saksham sarki", "Saksham"),
        ("subham", "Shubham"),
        ("subham shah", "Shubham"),
        ("shubham shah", "Shubham"),
        ("nitesh jha", "Nitesh"),
        ("nitish", "Nitesh"),
    ];
    let first = key.split(' ').next().unwrap_or("");
    let alias = aliases
        .iter()
        .find(|(k, _)| *k == key || *k == first)
        .map(|(_, v)| *v)
        .unwrap_or("");
    if !alias.is_empty() {
        if let Some(email) = cfg.support_staff_emails.get(alias) {
            return email.clone();
        }
    }
    for (staff, email) in &cfg.support_staff_emails {
        if staff_key(staff) == key || staff_key(staff) == first {
            return email.clone();
        }
    }
    String::new()
}

pub fn resolve_poc_sheet_name(poc: &str) -> String {
    let text = staff_key(poc);
    if text.starts_with("pras") || text == "prasanna" {
        "Prasanna".into()
    } else if text.starts_with("sajit") {
        "Sajit".into()
    } else if text.starts_with("saksham") || text.starts_with("shaksham") {
        "Saksham".into()
    } else {
        "Prasanna".into()
    }
}

pub fn resolve_poc_email(cfg: &Config, poc_name: &str) -> String {
    let text = poc_name.trim().to_lowercase();
    if text.is_empty() {
        return String::new();
    }
    for (name, email) in &cfg.poc_emails {
        if name.to_lowercase() == text {
            return email.clone();
        }
    }
    if text.starts_with("pras") {
        return cfg.poc_emails.get("Prasanna").cloned().unwrap_or_default();
    }
    if text.starts_with("sajit") {
        return cfg.poc_emails.get("Sajit").cloned().unwrap_or_default();
    }
    if text.starts_with("saksham") || text.starts_with("shaksham") {
        return cfg.poc_emails.get("Saksham").cloned().unwrap_or_default();
    }
    String::new()
}

pub fn normalize_email(raw: &str) -> String {
    let email = raw.trim().to_lowercase();
    if email.contains('@') {
        email
    } else {
        String::new()
    }
}
