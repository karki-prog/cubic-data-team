use chrono::{Datelike, Duration, NaiveDate, Utc};
use chrono_tz::America::Chicago;

pub fn is_phone_call_stage(stage: &str) -> bool {
    let t = stage.trim();
    regex::Regex::new(r"(?i)^phone\s*call$")
        .ok()
        .map(|re| re.is_match(t))
        .unwrap_or(false)
}

pub fn normalize_key_part(value: &str) -> String {
    value
        .trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn today_date_key() -> String {
    let now = Utc::now().with_timezone(&Chicago);
    format!("{:04}-{:02}-{:02}", now.year(), now.month(), now.day())
}

pub fn add_days_to_date_key(date_key: &str, days: i64) -> String {
    let Some(date) = NaiveDate::parse_from_str(date_key, "%Y-%m-%d").ok() else {
        return String::new();
    };
    let next = date + Duration::days(days);
    format!("{:04}-{:02}-{:02}", next.year(), next.month(), next.day())
}

pub fn retention_cutoff_date_key(days: i64) -> String {
    add_days_to_date_key(&today_date_key(), -days)
}

pub fn parse_date_key(raw: &str) -> String {
    let text = raw.trim();
    if text.is_empty() {
        return String::new();
    }
    if let Some(caps) = regex::Regex::new(r"^(\d{4})-(\d{2})-(\d{2})")
        .unwrap()
        .captures(text)
    {
        return format!("{}-{}-{}", &caps[1], &caps[2], &caps[3]);
    }
    if let Some(caps) = regex::Regex::new(r"^(\d{1,2})[/-](\d{1,2})[/-](\d{2,4})")
        .unwrap()
        .captures(text)
    {
        let month: i32 = caps[1].parse().unwrap_or(0);
        let day: i32 = caps[2].parse().unwrap_or(0);
        let mut year: i32 = caps[3].parse().unwrap_or(0);
        if year < 100 {
            year += 2000;
        }
        return format!("{year:04}-{month:02}-{day:02}");
    }
    String::new()
}

pub fn format_mdy(date_key: &str) -> String {
    if date_key.is_empty() {
        return String::new();
    }
    let parts: Vec<&str> = date_key.split('-').collect();
    if parts.len() != 3 {
        return String::new();
    }
    format!(
        "{}/{}/{}",
        parts[1].parse::<u32>().unwrap_or(0),
        parts[2].parse::<u32>().unwrap_or(0),
        parts[0]
    )
}

pub fn parse_time_minutes(raw: &str) -> Option<i32> {
    let text = raw.trim().to_uppercase();
    let text = regex::Regex::new(r"\b(EST|EDT|CST|CDT|MST|MDT|PST|PDT)\b")
        .unwrap()
        .replace_all(&text, " ")
        .to_string();
    let text = regex::Regex::new(r"\s+")
        .unwrap()
        .replace_all(text.trim(), " ")
        .to_string();
    if text.is_empty() {
        return None;
    }
    let is_pm = text.contains("PM");
    let is_am = text.contains("AM");
    let cleaned = text.replace("PM", "").replace("AM", "");
    let cleaned = cleaned.trim();
    let (hours, mins) = if cleaned.contains(':') {
        let mut parts = cleaned.split(':');
        (
            parts.next()?.parse::<i32>().ok()?,
            parts
                .next()
                .and_then(|p| p.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().ok())
                .unwrap_or(0),
        )
    } else {
        (cleaned.parse::<i32>().ok()?, 0)
    };
    let mut hours = hours;
    if is_pm && hours != 12 {
        hours += 12;
    }
    if is_am && hours == 12 {
        hours = 0;
    }
    if !is_am && !is_pm && (1..=8).contains(&hours) {
        // A sheet cell typed as "1:30 CST" is the afternoon slot, not 1:30 AM.
        // Read literally it lands outside every booking window, so the slot it
        // occupies keeps showing as free and can be double booked.
        hours += 12;
    }
    Some(hours * 60 + mins)
}

pub fn wrap_mins(total_mins: i32) -> i32 {
    let mut wrapped = total_mins % (24 * 60);
    if wrapped < 0 {
        wrapped += 24 * 60;
    }
    wrapped
}

/// Minutes ahead of CST for a US zone label.
pub fn zone_offset_from_cst(zone: &str) -> i32 {
    match zone.trim().to_ascii_uppercase().as_str() {
        "EST" | "EDT" => 60,
        "MST" | "MDT" => -60,
        "PST" | "PDT" => -120,
        _ => 0,
    }
}

fn zone_in_label(raw: &str) -> Option<&'static str> {
    let u = raw.to_ascii_uppercase();
    if u.contains("EST") || u.contains("EDT") {
        Some("EST")
    } else if u.contains("PST") || u.contains("PDT") {
        Some("PST")
    } else if u.contains("MST") || u.contains("MDT") {
        Some("MST")
    } else if u.contains("CST") || u.contains("CDT") {
        Some("CST")
    } else {
        None
    }
}

/// Convert a posted clock time in EST/CST/MST/PST to a CST sheet label.
pub fn normalize_meeting_time_cst(raw: &str, posted_zone: &str) -> String {
    let text = raw.trim();
    if text.is_empty() {
        return String::new();
    }
    let zone = zone_in_label(text).unwrap_or_else(|| {
        let z = posted_zone.trim().to_ascii_uppercase();
        match z.as_str() {
            "EST" | "EDT" => "EST",
            "MST" | "MDT" => "MST",
            "PST" | "PDT" => "PST",
            _ => "CST",
        }
    });
    let Some(mins) = parse_time_minutes(text) else {
        return format!("{} CST", text);
    };
    format_time_display(wrap_mins(mins - zone_offset_from_cst(zone)))
}

pub fn format_time_display(minutes: i32) -> String {
    format!("{} CST", mins_to_clock(minutes))
}

/// 12-hour clock without timezone, e.g. `9 AM` or `9:30 AM`.
pub fn mins_to_clock(total_mins: i32) -> String {
    let mut wrapped = total_mins % (24 * 60);
    if wrapped < 0 {
        wrapped += 24 * 60;
    }
    let h24 = wrapped / 60;
    let m = wrapped % 60;
    let ap = if h24 >= 12 { "PM" } else { "AM" };
    let mut h12 = h24 % 12;
    if h12 == 0 {
        h12 = 12;
    }
    if m == 0 {
        format!("{h12} {ap}")
    } else {
        format!("{h12}:{m:02} {ap}")
    }
}

pub fn mins_to_hhmm(total_mins: i32) -> String {
    let mut wrapped = total_mins % (24 * 60);
    if wrapped < 0 {
        wrapped += 24 * 60;
    }
    format!("{:02}:{:02}", wrapped / 60, wrapped % 60)
}

pub fn duration_minutes(raw: &str) -> i32 {
    let text = raw.trim().to_lowercase();
    if text.is_empty() {
        return 30;
    }
    if let Some(caps) = regex::Regex::new(r"(\d+)\s*hr(?:s)?\s*(\d+)")
        .unwrap()
        .captures(&text)
    {
        return caps[1].parse::<i32>().unwrap_or(0) * 60 + caps[2].parse::<i32>().unwrap_or(0);
    }
    if let Some(caps) = regex::Regex::new(r"(\d+)\s*hr").unwrap().captures(&text) {
        return caps[1].parse::<i32>().unwrap_or(0) * 60;
    }
    if let Some(caps) = regex::Regex::new(r"(\d+)\s*min").unwrap().captures(&text) {
        return caps[1].parse::<i32>().unwrap_or(0);
    }
    match text.parse::<i32>() {
        Ok(n) if n >= 24 => n,
        Ok(n) => n * 60,
        Err(_) => 30,
    }
}

pub fn duration_label(mins: i32) -> String {
    match mins {
        15 => "15 min".into(),
        30 => "30 min".into(),
        45 => "45 min".into(),
        60 => "1 Hr".into(),
        90 => "1 Hr 30 min".into(),
        120 => "2 Hr".into(),
        180 => "3 Hr".into(),
        240 => "4 Hr".into(),
        n if n > 240 => "4 Hr +".into(),
        n if n > 0 => format!("{n} min"),
        _ => "30 min".into(),
    }
}

pub fn meeting_time_key(raw: &str) -> String {
    match parse_time_minutes(raw) {
        Some(mins) => format_time_display(mins),
        None => normalize_key_part(raw),
    }
}

pub fn combine_date_time_iso(date_key: &str, time_raw: &str) -> String {
    if date_key.is_empty() {
        return String::new();
    }
    let mins = parse_time_minutes(time_raw).unwrap_or(9 * 60);
    let h = mins / 60;
    let m = mins % 60;
    format!("{date_key}T{h:02}:{m:02}:00")
}

pub fn normalize_status(raw: &str) -> String {
    let s = raw.trim();
    if regex::Regex::new(r"(?i)^approv|^accept").unwrap().is_match(s) {
        "Accepted".into()
    } else if regex::Regex::new(r"(?i)^declin").unwrap().is_match(s) {
        "Declined".into()
    } else if regex::Regex::new(r"(?i)^pend").unwrap().is_match(s) {
        "Pending".into()
    } else {
        s.to_string()
    }
}

pub fn chicago_today_parts() -> (i32, u32, u32) {
    let now = Utc::now().with_timezone(&Chicago);
    (now.year(), now.month(), now.day())
}

pub fn add_days_local(year: i32, month: u32, day: u32, add: i64) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).unwrap() + Duration::days(add)
}

pub fn format_date_key_mdy(date: NaiveDate) -> String {
    format!("{}/{}/{}", date.month(), date.day(), date.year())
}

pub fn format_day_label(date: NaiveDate) -> String {
    let days = [
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
        "Sunday",
    ];
    let months = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let weekday = date.weekday().num_days_from_monday() as usize;
    format!(
        "{} {} {}",
        days[weekday],
        months[date.month0() as usize],
        date.day()
    )
}

/// Connector column A timestamp. Must match the Apps Script writer
/// (`formatSubmittedTimestamp_`): `"MMM d, yyyy h:mm a"` + `" CST"`, e.g.
/// `"Sep 2, 2026 3:03 PM CST"`. The trailing " CST" also stops Sheets from
/// re-parsing the cell into a locale date like `9/2/2026, 3:03 PM`.
pub fn chicago_timestamp() -> String {
    let now = Utc::now().with_timezone(&Chicago);
    format!("{} CST", now.format("%b %-d, %Y %-I:%M %p"))
}

pub fn format_meeting_time(time: &str) -> String {
    normalize_meeting_time_cst(time, "CST")
}

#[cfg(test)]
mod time_parse_tests {
    use super::parse_time_minutes;

    #[test]
    fn explicit_am_pm_is_respected() {
        assert_eq!(parse_time_minutes("2:00 PM CST"), Some(14 * 60));
        assert_eq!(parse_time_minutes("10 AM CST"), Some(10 * 60));
        assert_eq!(parse_time_minutes("12:30 PM"), Some(12 * 60 + 30));
        assert_eq!(parse_time_minutes("12:15 AM"), Some(15));
    }

    #[test]
    fn bare_morning_hours_stay_morning() {
        // 9–12 without a meridiem are already inside the morning windows.
        assert_eq!(parse_time_minutes("11:30 CST"), Some(11 * 60 + 30));
        assert_eq!(parse_time_minutes("10:00 CST"), Some(10 * 60));
    }

    #[test]
    fn bare_afternoon_hours_land_in_the_afternoon_window() {
        // Phone windows are 10:00–12:00 and 13:00–15:00 CST. Read literally,
        // "1:30" would be 90 minutes and match no slot at all.
        assert_eq!(parse_time_minutes("1:00 CST"), Some(13 * 60));
        assert_eq!(parse_time_minutes("1:30 CST"), Some(13 * 60 + 30));
        assert_eq!(parse_time_minutes("2:30 CST"), Some(14 * 60 + 30));
    }
}

#[cfg(test)]
mod timestamp_tests {
    use super::chicago_timestamp;
    use regex::Regex;

    #[test]
    fn connector_timestamp_matches_apps_script_format() {
        // Must look like "Sep 2, 2026 3:03 PM CST" — no leading zeros on day/hour,
        // trailing " CST" so Sheets keeps it as text.
        let ts = chicago_timestamp();
        let re = Regex::new(
            r"^[A-Z][a-z]{2} [1-9][0-9]?, \d{4} (1[0-2]|[1-9]):[0-5][0-9] (AM|PM) CST$",
        )
        .unwrap();
        assert!(re.is_match(&ts), "unexpected timestamp format: {ts:?}");
    }
}
