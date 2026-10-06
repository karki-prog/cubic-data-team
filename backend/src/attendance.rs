//! Otter & Pronunciation class attendance: Google Meet → "Otter Attendance - Data".
//!
//! After each class, Meet's conference records list who joined. For every ended
//! call not yet logged, the job:
//!   1. totals each participant's minutes (both daily sessions count toward the day),
//!   2. ticks that person's checkbox for the day on the month tab (`October`, …),
//!      creating the month tab from the latest roster if it does not exist yet,
//!   3. appends one row per participant to `Attendance Log` (the audit trail, and
//!      the record of which calls are already done).
//!
//! Once a day the month tab's roster (name + email) is refreshed from the same
//! month's tab on the Data application tracking sheet (`October_2026`, …).
//!
//! It only ever sets checkboxes to TRUE, so a manual tick is never undone, and a
//! call that is already in the log is never processed twice (manual un-ticks stick).
//! Meet data comes from the calendar OAuth user (karki@), who organizes the class;
//! that token needs the `meetings.space.readonly` scope.

use std::collections::{BTreeMap, HashSet};
use std::time::Duration;

use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::Json;
use chrono::{DateTime, Datelike, NaiveDate, Timelike, Utc, Weekday};
use chrono_tz::America::Chicago;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::AppError;
use crate::jobs::assert_job_auth;
use crate::AppState;

const MEET_API: &str = "https://meet.googleapis.com/v2";
const MEET_SCOPE: &str = "https://www.googleapis.com/auth/meetings.space.readonly";
const LOG_TAB: &str = "Attendance Log";
const LOG_HEADER: [&str; 9] = [
    "Date",
    "Session",
    "Meet name",
    "Minutes",
    "Present",
    "Matched to",
    "Recorded at",
    "Meet record",
    "Join times",
];
const MONTHS: [&str; 12] = [
    "January", "February", "March", "April", "May", "June", "July", "August", "September",
    "October", "November", "December",
];
/// Background pass cadence on the live server.
const POLL: Duration = Duration::from_secs(5 * 60);
const DEFAULT_LOOKBACK_DAYS: i64 = 3;

/* ------------------------------------------------------------------ Meet API */

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ConferenceList {
    #[serde(default)]
    conference_records: Vec<ConferenceRecord>,
    next_page_token: Option<String>,
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ConferenceRecord {
    name: String,
    start_time: Option<String>,
    end_time: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ParticipantList {
    #[serde(default)]
    participants: Vec<Participant>,
    next_page_token: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Participant {
    name: String,
    signedin_user: Option<NamedUser>,
    anonymous_user: Option<NamedUser>,
    phone_user: Option<NamedUser>,
    earliest_start_time: Option<String>,
    latest_end_time: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NamedUser {
    display_name: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct SessionList {
    /// One person rarely rejoins 100+ times in an hour, so a single page is enough.
    #[serde(default)]
    participant_sessions: Vec<ParticipantSession>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ParticipantSession {
    start_time: Option<String>,
    end_time: Option<String>,
}

fn ts(raw: &Option<String>) -> Option<DateTime<Utc>> {
    raw.as_deref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.with_timezone(&Utc))
}

/// Minutes between the clock times we display (both truncated to the minute), so
/// "11:30 AM–12:17 PM" is always exactly 47 and the per-join numbers add up to the total.
fn clock_minutes(start: DateTime<Utc>, end: DateTime<Utc>) -> f64 {
    let a = start.timestamp().div_euclid(60);
    let b = end.timestamp().div_euclid(60);
    (b - a).max(0) as f64
}

/// Scheduled class a call belongs to (CST): morning call → 11:30 AM, afternoon → 2:00 PM.
fn session_label(local_start: &DateTime<chrono_tz::Tz>) -> String {
    if local_start.hour() < 13 { "11:30 AM".into() } else { "2:00 PM".into() }
}


/// Meet access, in order of preference:
///   1. the service account acting as the class organizer (domain-wide delegation — never expires),
///   2. the organizer's own OAuth login (needs the meetings.space.readonly scope).
async fn meet_token(state: &AppState) -> anyhow::Result<String> {
    let organizer = state.cfg.calendar_owner_email.clone();
    match state.google.sa_token_as(&organizer, MEET_SCOPE).await {
        Ok(t) => Ok(t),
        Err(sa_err) => {
            tracing::debug!(error = %sa_err, "Meet via service account unavailable; trying organizer login");
            state.google.user_token("calendar").await
        }
    }
}

async fn meet_get<T: for<'de> Deserialize<'de>>(state: &AppState, url: &str) -> anyhow::Result<T> {
    let token = meet_token(state).await?;
    let res = state.google.http().get(url).bearer_auth(token).send().await?;
    let status = res.status();
    let body = res.text().await?;
    if !status.is_success() {
        if status.as_u16() == 403 && body.contains("insufficient") {
            anyhow::bail!(
                "Google Meet access is not approved yet. Either grant the service account domain-wide \
                 delegation for meetings.space.readonly (Workspace Admin), or run \
                 `node scripts/mint-google-token.mjs` as karki@cubicit.net and update the server tokens."
            );
        }
        if status.as_u16() == 403 && body.contains("SERVICE_DISABLED") {
            anyhow::bail!("Google Meet REST API is disabled for this Google Cloud project. Enable it in the console.");
        }
        anyhow::bail!("Meet API {status}: {body}");
    }
    Ok(serde_json::from_str(&body)?)
}

async fn ended_records(state: &AppState, since: DateTime<Utc>) -> anyhow::Result<Vec<ConferenceRecord>> {
    let filter = format!(
        "space.meeting_code = \"{}\" AND start_time >= \"{}\"",
        state.cfg.otter_meet_code,
        since.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    );
    let mut out = Vec::new();
    let mut page: Option<String> = None;
    loop {
        let mut url = format!(
            "{MEET_API}/conferenceRecords?pageSize=100&filter={}",
            urlencoding::encode(&filter)
        );
        if let Some(p) = &page {
            url.push_str(&format!("&pageToken={}", urlencoding::encode(p)));
        }
        let list: ConferenceList = meet_get(state, &url).await?;
        out.extend(list.conference_records.into_iter().filter(|r| r.end_time.is_some()));
        match list.next_page_token.filter(|t| !t.is_empty()) {
            Some(t) => page = Some(t),
            None => break,
        }
    }
    out.sort_by(|a, b| a.start_time.cmp(&b.start_time));
    Ok(out)
}

/// One participant of one call: name, total minutes, and each join → leave interval.
struct Attendee {
    name: String,
    minutes: f64,
    joins: Vec<(DateTime<Utc>, DateTime<Utc>)>,
}

/// "11:31 AM–11:52 AM (21m); 11:55 AM–12:30 PM (35m)" in Chicago time.
fn joins_text(joins: &[(DateTime<Utc>, DateTime<Utc>)]) -> String {
    joins
        .iter()
        .map(|(a, b)| {
            format!(
                "{}–{} ({}m)",
                a.with_timezone(&Chicago).format("%-I:%M %p"),
                b.with_timezone(&Chicago).format("%-I:%M %p"),
                clock_minutes(*a, *b)
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// Everyone in one call.
async fn record_attendees(state: &AppState, record: &str) -> anyhow::Result<Vec<Attendee>> {
    let mut people = Vec::new();
    let mut page: Option<String> = None;
    loop {
        let mut url = format!("{MEET_API}/{record}/participants?pageSize=250");
        if let Some(p) = &page {
            url.push_str(&format!("&pageToken={}", urlencoding::encode(p)));
        }
        let list: ParticipantList = meet_get(state, &url).await?;
        people.extend(list.participants);
        match list.next_page_token.filter(|t| !t.is_empty()) {
            Some(t) => page = Some(t),
            None => break,
        }
    }
    let mut out = Vec::new();
    for p in people {
        let display = p
            .signedin_user
            .as_ref()
            .or(p.anonymous_user.as_ref())
            .or(p.phone_user.as_ref())
            .and_then(|u| u.display_name.clone())
            .unwrap_or_default()
            .trim()
            .to_string();
        if display.is_empty() {
            continue;
        }
        // Every join → leave (people drop and rejoin); fall back to first-in/last-out.
        let url = format!("{MEET_API}/{}/participantSessions?pageSize=100", p.name);
        let mut joins: Vec<(DateTime<Utc>, DateTime<Utc>)> = match meet_get::<SessionList>(state, &url).await {
            Ok(s) => s
                .participant_sessions
                .iter()
                .filter_map(|x| Some((ts(&x.start_time)?, ts(&x.end_time)?)))
                .collect(),
            Err(_) => Vec::new(),
        };
        if joins.is_empty() {
            if let (Some(a), Some(b)) = (ts(&p.earliest_start_time), ts(&p.latest_end_time)) {
                joins.push((a, b));
            }
        }
        joins.sort();
        let minutes = joins.iter().map(|(a, b)| clock_minutes(*a, *b)).sum();
        out.push(Attendee { name: display, minutes, joins });
    }
    Ok(out)
}

/* --------------------------------------------------------------- name match */

fn norm_name(raw: &str) -> String {
    raw.chars()
        .map(|c| if c.is_alphabetic() { c.to_ascii_lowercase() } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Roster rows (0-based sheet row index) that are the same person as `meet`.
/// Exact → joined-letters (`SheetalRajPrasai`) → same first name with every token
/// of the shorter name present in the longer (`Rashmi` vs `Rashmi Karki`).
fn match_rows(meet: &str, roster: &[(usize, String)]) -> Vec<usize> {
    let m = norm_name(meet);
    if m.is_empty() {
        return Vec::new();
    }
    let squash = |s: &str| s.replace(' ', "");
    let exact: Vec<usize> = roster.iter().filter(|(_, n)| norm_name(n) == m).map(|(i, _)| *i).collect();
    if !exact.is_empty() {
        return exact;
    }
    let joined: Vec<usize> = roster
        .iter()
        .filter(|(_, n)| squash(&norm_name(n)) == squash(&m))
        .map(|(i, _)| *i)
        .collect();
    if !joined.is_empty() {
        return joined;
    }
    let mt: Vec<&str> = m.split(' ').collect();
    let partial: Vec<usize> = roster
        .iter()
        .filter(|(_, n)| {
            let rn = norm_name(n);
            let rt: Vec<&str> = rn.split(' ').collect();
            if rt.is_empty() || rt[0] != mt[0] {
                return false;
            }
            let (short, long) = if rt.len() <= mt.len() { (&rt, &mt) } else { (&mt, &rt) };
            short.iter().all(|t| long.contains(t))
        })
        .map(|(i, _)| *i)
        .collect();
    // A bare first name shared by two different people is ambiguous; don't guess.
    let distinct: HashSet<String> = partial.iter().map(|i| norm_name(&roster.iter().find(|(r, _)| r == i).unwrap().1)).collect();
    if distinct.len() == 1 {
        partial
    } else {
        Vec::new()
    }
}

/* ---------------------------------------------------------------- sheet I/O */

fn col_letter(idx: usize) -> String {
    let mut n = idx + 1;
    let mut s = String::new();
    while n > 0 {
        let r = (n - 1) % 26;
        s.insert(0, (b'A' + r as u8) as char);
        n = (n - 1) / 26;
    }
    s
}

/// Weekday columns for a month, with a blank column between weeks (the sheet's layout).
fn month_header(year: i32, month: u32) -> Vec<String> {
    let mut header = vec![String::new(), String::new()];
    let mut day = NaiveDate::from_ymd_opt(year, month, 1).unwrap();
    let mut started = false;
    while day.month() == month {
        match day.weekday() {
            Weekday::Sat | Weekday::Sun => {}
            wd => {
                if wd == Weekday::Mon && started {
                    header.push(String::new());
                }
                header.push(day.day().to_string());
                started = true;
            }
        }
        day = day.succ_opt().unwrap();
    }
    header
}

async fn sa_post(state: &AppState, url: &str, body: Value) -> anyhow::Result<Value> {
    let token = state.google.sa_token().await?;
    let res = state.google.http().post(url).bearer_auth(token).json(&body).send().await?;
    let status = res.status();
    let text = res.text().await?;
    if !status.is_success() {
        anyhow::bail!("Sheets {status}: {text}");
    }
    Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
}

/// Write roster rows (name, email, unticked checkbox per weekday) starting at 0-based `start_row`.
async fn write_roster_rows(
    state: &AppState,
    tab: &str,
    sheet_id: i64,
    header: &[String],
    start_row: usize,
    people: &[(String, String)],
) -> anyhow::Result<()> {
    if people.is_empty() {
        return Ok(());
    }
    let sid = &state.cfg.attendance_spreadsheet_id;
    let values: Vec<Vec<Value>> = people
        .iter()
        .map(|(name, email)| {
            let mut row = vec![json!(name), json!(email)];
            for h in header.iter().skip(2) {
                row.push(if h.trim().is_empty() { json!("") } else { json!(false) });
            }
            row
        })
        .collect();
    state
        .google
        .sheets_values_update(sid, &format!("'{tab}'!A{}", start_row + 1), values, "USER_ENTERED")
        .await?;
    let requests: Vec<Value> = header
        .iter()
        .enumerate()
        .filter(|(ci, h)| *ci >= 2 && !h.trim().is_empty())
        .map(|(ci, _)| {
            json!({ "setDataValidation": {
                "range": { "sheetId": sheet_id, "startRowIndex": start_row, "endRowIndex": start_row + people.len(),
                           "startColumnIndex": ci, "endColumnIndex": ci + 1 },
                "rule": { "condition": { "type": "BOOLEAN" }, "strict": true }
            }})
        })
        .collect();
    state.google.batch_update(sid, requests).await
}

/// Make sure the month tab exists. A new tab is seeded from `seed` (the tracking
/// sheet roster); with no seed it copies the names of the newest month tab.
async fn ensure_month_tab(
    state: &AppState,
    tab: &str,
    year: i32,
    month: u32,
    seed: &[(String, String)],
) -> anyhow::Result<bool> {
    let sid = &state.cfg.attendance_spreadsheet_id;
    let titles = state.google.sheet_titles(sid).await?;
    if titles.iter().any(|t| t == tab) {
        return Ok(false);
    }
    let people: Vec<(String, String)> = if !seed.is_empty() {
        seed.to_vec()
    } else {
        let template = titles
            .iter()
            .filter_map(|t| MONTHS.iter().position(|m| m.eq_ignore_ascii_case(t.trim())).map(|i| (i, t)))
            .min_by_key(|(i, _)| (month as usize + 12 - 1 - i) % 12)
            .map(|(_, t)| t.clone());
        match &template {
            Some(t) => state
                .google
                .sheets_values_get(sid, &format!("'{t}'!A2:B500"), Some("FORMATTED_VALUE"))
                .await?
                .into_iter()
                .filter_map(|r| {
                    let name = r.first().map(|s| s.trim().to_string()).unwrap_or_default();
                    (!name.is_empty()).then(|| (name, r.get(1).cloned().unwrap_or_default()))
                })
                .collect(),
            None => Vec::new(),
        }
    };
    let mut header = month_header(year, month);
    header[0] = "Candidate".into();
    header[1] = "Email".into();
    let cols = header.len().max(26);
    let rows = (people.len() + 1).max(200);
    let added = sa_post(
        state,
        &format!("https://sheets.googleapis.com/v4/spreadsheets/{sid}:batchUpdate"),
        json!({ "requests": [{ "addSheet": { "properties": {
            "title": tab, "index": 0,
            "gridProperties": { "rowCount": rows, "columnCount": cols, "frozenRowCount": 1, "frozenColumnCount": 1 }
        }}}]}),
    )
    .await?;
    let sheet_id = added["replies"][0]["addSheet"]["properties"]["sheetId"].as_i64().unwrap_or(0);
    state
        .google
        .sheets_values_update(
            sid,
            &format!("'{tab}'!A1"),
            vec![header.iter().map(|h| json!(h)).collect()],
            "USER_ENTERED",
        )
        .await?;
    state
        .google
        .batch_update(
            sid,
            vec![json!({ "repeatCell": {
                "range": { "sheetId": sheet_id, "startRowIndex": 0, "endRowIndex": 1 },
                "cell": { "userEnteredFormat": { "textFormat": { "bold": true }, "horizontalAlignment": "CENTER" } },
                "fields": "userEnteredFormat(textFormat,horizontalAlignment)"
            }})],
        )
        .await?;
    write_roster_rows(state, tab, sheet_id, &header, 1, &people).await?;
    tracing::info!(tab, roster = people.len(), "attendance: created month tab");
    Ok(true)
}

fn norm_email(raw: &str) -> String {
    raw.trim().to_lowercase()
}

/// Candidates (name, email) on this month's tab of the Data application tracking sheet.
async fn tracking_roster(state: &AppState, year: i32, month: u32) -> Vec<(String, String)> {
    let tab = format!("{}_{}", MONTHS[month as usize - 1], year);
    let mut seen = HashSet::new();
    crate::tracker::tracking_name_email_rows(state, &tab)
        .await
        .into_iter()
        .map(|(n, e)| (n.trim().to_string(), norm_email(&e)))
        .filter(|(n, _)| !n.is_empty() && !n.eq_ignore_ascii_case("candidate name"))
        .filter(|(n, e)| seen.insert(if e.is_empty() { norm_name(n) } else { e.clone() }))
        .collect()
}

/// Daily: keep this month's attendance roster in step with the tracking sheet.
/// New candidates are added (with unticked boxes), emails are filled in, and a
/// renamed candidate (same email) gets the new name. Rows are never deleted, so
/// attendance already recorded for someone who left the roster is kept.
pub async fn sync_roster(state: &AppState) -> anyhow::Result<Value> {
    let sid = state.cfg.attendance_spreadsheet_id.clone();
    let today = Utc::now().with_timezone(&Chicago).date_naive();
    let (year, month) = (today.year(), today.month());
    let tab = MONTHS[month as usize - 1];
    let people = tracking_roster(state, year, month).await;
    if people.is_empty() {
        anyhow::bail!("{}_{year} on the tracking sheet has no candidates; attendance roster left unchanged", tab);
    }
    if ensure_month_tab(state, tab, year, month, &people).await? {
        return Ok(json!({ "ok": true, "tab": tab, "created": true, "added": people.len() }));
    }

    let grid = state
        .google
        .sheets_values_get(&sid, &format!("'{tab}'!A1:BZ500"), Some("FORMATTED_VALUE"))
        .await?;
    let header = grid.first().cloned().unwrap_or_default();
    let mut header_full = header.clone();
    header_full.resize(header_full.len().max(2), String::new());
    let existing: Vec<(usize, String, String)> = grid
        .iter()
        .enumerate()
        .skip(1)
        .map(|(i, r)| {
            (
                i,
                r.first().map(|s| s.trim().to_string()).unwrap_or_default(),
                norm_email(r.get(1).map(String::as_str).unwrap_or("")),
            )
        })
        .collect();
    let roster: Vec<(usize, String)> = existing
        .iter()
        .filter(|(_, n, _)| !n.is_empty())
        .map(|(i, n, _)| (*i, n.clone()))
        .collect();

    let mut updates: Vec<Value> = Vec::new();
    let mut to_add: Vec<(String, String)> = Vec::new();
    let (mut renamed, mut emails_filled) = (0usize, 0usize);
    for (name, email) in &people {
        let by_email = (!email.is_empty())
            .then(|| existing.iter().find(|(_, _, e)| e == email))
            .flatten();
        if let Some((row, current, _)) = by_email {
            if norm_name(current) != norm_name(name) {
                updates.push(json!({ "range": format!("'{tab}'!A{}", row + 1), "values": [[name]] }));
                renamed += 1;
            }
            continue;
        }
        let rows = match_rows(name, &roster);
        if let Some(row) = rows.first() {
            let current_email = &existing.iter().find(|(r, _, _)| r == row).map(|x| x.2.clone()).unwrap_or_default();
            if current_email.is_empty() && !email.is_empty() {
                updates.push(json!({ "range": format!("'{tab}'!B{}", row + 1), "values": [[email]] }));
                emails_filled += 1;
            }
            continue;
        }
        to_add.push((name.clone(), email.clone()));
    }
    if header_full[0].trim().is_empty() && header_full[1].trim().is_empty() {
        updates.push(json!({ "range": format!("'{tab}'!A1:B1"), "values": [["Candidate", "Email"]] }));
    }
    if !updates.is_empty() {
        sa_post(
            state,
            &format!("https://sheets.googleapis.com/v4/spreadsheets/{sid}/values:batchUpdate"),
            json!({ "valueInputOption": "RAW", "data": updates }),
        )
        .await?;
    }
    if !to_add.is_empty() {
        let last = existing.iter().filter(|(_, n, e)| !n.is_empty() || !e.is_empty()).map(|(i, _, _)| *i).max().unwrap_or(0);
        let sheet_id = state.google.sheet_id(&sid, tab).await?;
        write_roster_rows(state, tab, sheet_id, &header_full, last + 1, &to_add).await?;
    }
    let tracked: HashSet<String> = people.iter().map(|(n, _)| norm_name(n)).collect();
    let not_in_tracking: Vec<String> = roster
        .iter()
        .filter(|(_, n)| !tracked.contains(&norm_name(n)) && match_rows(n, &people.iter().enumerate().map(|(i, (pn, _))| (i, pn.clone())).collect::<Vec<_>>()).is_empty())
        .map(|(_, n)| n.clone())
        .collect();
    Ok(json!({
        "ok": true,
        "tab": tab,
        "created": false,
        "added": to_add.iter().map(|(n, _)| n).collect::<Vec<_>>(),
        "renamed": renamed,
        "emailsFilled": emails_filled,
        "notInTracking": not_in_tracking,
    }))
}
async fn ensure_log_tab(state: &AppState) -> anyhow::Result<HashSet<String>> {
    let sid = &state.cfg.attendance_spreadsheet_id;
    let titles = state.google.sheet_titles(sid).await?;
    if !titles.iter().any(|t| t == LOG_TAB) {
        sa_post(
            state,
            &format!("https://sheets.googleapis.com/v4/spreadsheets/{sid}:batchUpdate"),
            json!({ "requests": [{ "addSheet": { "properties": {
                "title": LOG_TAB, "gridProperties": { "rowCount": 1000, "columnCount": 9, "frozenRowCount": 1 }
            }}}]}),
        )
        .await?;
        state
            .google
            .sheets_values_update(
                sid,
                &format!("'{LOG_TAB}'!A1"),
                vec![LOG_HEADER.iter().map(|h| json!(h)).collect()],
                "RAW",
            )
            .await?;
        return Ok(HashSet::new());
    }
    let done = state
        .google
        .sheets_values_get(sid, &format!("'{LOG_TAB}'!H2:H"), Some("FORMATTED_VALUE"))
        .await?;
    Ok(done.into_iter().filter_map(|r| r.into_iter().next()).filter(|s| !s.is_empty()).collect())
}

/* ------------------------------------------------------------------- the job */

struct DayTotal {
    /// Meet display name → total minutes that day (both sessions).
    minutes: BTreeMap<String, f64>,
}

/// One pass: process every ended class call from the last `days` days not yet logged.
pub async fn sync(state: &AppState, days: i64) -> anyhow::Result<Value> {
    let sid = state.cfg.attendance_spreadsheet_id.clone();
    let min_minutes = state.cfg.attendance_min_minutes as f64;
    let since = Utc::now() - chrono::Duration::days(days.max(1));
    let records = ended_records(state, since).await?;
    let done = ensure_log_tab(state).await?;
    let fresh: Vec<ConferenceRecord> = records.into_iter().filter(|r| !done.contains(&r.name)).collect();
    if fresh.is_empty() {
        return Ok(json!({ "ok": true, "calls": 0, "ticked": 0, "note": "no new class calls" }));
    }

    let mut by_day: BTreeMap<NaiveDate, DayTotal> = BTreeMap::new();
    let mut log_rows: Vec<Vec<Value>> = Vec::new();
    let now_local = Utc::now().with_timezone(&Chicago).format("%Y-%m-%d %H:%M").to_string();
    let mut pending_log: Vec<(NaiveDate, String, String, f64, String, String)> = Vec::new();

    for rec in &fresh {
        let Some(start) = ts(&rec.start_time) else { continue };
        let local = start.with_timezone(&Chicago);
        let day = local.date_naive();
        let session = session_label(&local);
        let attendees = record_attendees(state, &rec.name).await?;
        let total = by_day.entry(day).or_insert_with(|| DayTotal { minutes: BTreeMap::new() });
        for a in attendees {
            *total.minutes.entry(a.name.clone()).or_insert(0.0) += a.minutes;
            pending_log.push((day, session.clone(), a.name, a.minutes, rec.name.clone(), joins_text(&a.joins)));
        }
    }

    let mut ticked = 0usize;
    let mut unmatched: Vec<String> = Vec::new();
    let mut matched_label: BTreeMap<(NaiveDate, String), String> = BTreeMap::new();
    let mut created_tabs = Vec::new();

    for (day, total) in &by_day {
        let tab = MONTHS[day.month0() as usize];
        let seed = tracking_roster(state, day.year(), day.month()).await;
        if ensure_month_tab(state, tab, day.year(), day.month(), &seed).await? {
            created_tabs.push(tab.to_string());
        }
        let grid = state
            .google
            .sheets_values_get(&sid, &format!("'{tab}'!A1:BZ500"), Some("FORMATTED_VALUE"))
            .await?;
        let header = grid.first().cloned().unwrap_or_default();
        let day_col = header.iter().position(|h| h.trim() == day.day().to_string());
        let roster: Vec<(usize, String)> = grid
            .iter()
            .enumerate()
            .skip(1)
            .filter_map(|(i, r)| r.first().filter(|n| !n.trim().is_empty()).map(|n| (i, n.clone())))
            .collect();

        let mut cells: Vec<Value> = Vec::new();
        for (name, minutes) in &total.minutes {
            let rows = match_rows(name, &roster);
            if rows.is_empty() {
                unmatched.push(name.clone());
                continue;
            }
            let label = rows
                .iter()
                .filter_map(|i| roster.iter().find(|(r, _)| r == i).map(|(_, n)| n.trim().to_string()))
                .next()
                .unwrap_or_default();
            matched_label.insert((*day, name.clone()), format!("{label} (row {})", rows[0] + 1));
            if *minutes < min_minutes {
                continue;
            }
            let Some(col) = day_col else { continue };
            for r in rows {
                cells.push(json!({ "range": format!("'{tab}'!{}{}", col_letter(col), r + 1), "values": [[true]] }));
                ticked += 1;
            }
        }
        if !cells.is_empty() {
            sa_post(
                state,
                &format!("https://sheets.googleapis.com/v4/spreadsheets/{sid}/values:batchUpdate"),
                json!({ "valueInputOption": "USER_ENTERED", "data": cells }),
            )
            .await?;
        }
        if day_col.is_none() {
            tracing::warn!(%day, tab, "attendance: no date column for this day (weekend?) — logged only");
        }
    }

    for (day, session, name, minutes, record, joins) in pending_log {
        let present = by_day
            .get(&day)
            .and_then(|t| t.minutes.get(&name))
            .map(|m| *m >= min_minutes)
            .unwrap_or(false);
        let matched = matched_label.get(&(day, name.clone())).cloned().unwrap_or_else(|| "— not on roster".into());
        log_rows.push(vec![
            json!(day.format("%Y-%m-%d").to_string()),
            json!(session),
            json!(name),
            json!((minutes * 10.0).round() / 10.0),
            json!(if present { "Yes" } else { "No" }),
            json!(matched),
            json!(now_local),
            json!(record),
            json!(joins),
        ]);
    }
    if !log_rows.is_empty() {
        sa_post(
            state,
            &format!(
                "https://sheets.googleapis.com/v4/spreadsheets/{sid}/values/{}:append?valueInputOption=RAW&insertDataOption=INSERT_ROWS",
                urlencoding::encode(&format!("'{LOG_TAB}'!A1"))
            ),
            json!({ "values": log_rows }),
        )
        .await?;
    }
    unmatched.sort();
    unmatched.dedup();
    Ok(json!({
        "ok": true,
        "calls": fresh.len(),
        "days": by_day.keys().map(|d| d.to_string()).collect::<Vec<_>>(),
        "ticked": ticked,
        "createdTabs": created_tabs,
        "unmatched": unmatched,
        "minMinutes": min_minutes,
    }))
}

/// Background pass every 5 minutes, live server only (a local dev box must never
/// write to the real attendance sheet).
pub fn spawn(state: AppState) {
    let base = state.cfg.site_url.trim().to_lowercase();
    let public = base.starts_with("https://") && !base.contains("localhost") && !base.contains("127.0.0.1");
    if !public || std::env::var("ATTENDANCE_AUTO").ok().as_deref() == Some("0") {
        tracing::info!("Otter attendance auto-sync off (not the public site or ATTENDANCE_AUTO=0)");
        return;
    }
    tracing::info!("Otter attendance auto-sync every {}m", POLL.as_secs() / 60);
    tokio::spawn(async move {
        let mut roster_day: Option<NaiveDate> = None;
        loop {
            // Roster from the tracking sheet: once per Chicago day (first pass after boot too).
            let today = Utc::now().with_timezone(&Chicago).date_naive();
            if roster_day != Some(today) {
                match sync_roster(&state).await {
                    Ok(v) => {
                        tracing::info!(result = %v, "attendance roster synced");
                        roster_day = Some(today);
                    }
                    Err(err) => tracing::warn!(error = %err, "attendance roster sync failed"),
                }
            }
            match sync(&state, DEFAULT_LOOKBACK_DAYS).await {
                Ok(v) if v["calls"].as_u64().unwrap_or(0) > 0 => tracing::info!(result = %v, "attendance synced"),
                Ok(_) => {}
                Err(err) => tracing::warn!(error = %err, "attendance sync failed"),
            }
            // Picks up new Meet data and any ticks staff changed by hand on the sheet.
            refresh_snapshots(&state).await;
            tokio::time::sleep(POLL).await;
        }
    });
}

#[derive(Deserialize)]
pub struct SyncQuery {
    days: Option<i64>,
}

/// Manual / cron: `GET /api/jobs/otter-roster-sync` — refresh this month's roster now.
pub async fn roster_route(State(state): State<AppState>, headers: HeaderMap) -> Result<impl IntoResponse, AppError> {
    assert_job_auth(&state, &headers).await?;
    let result = sync_roster(&state).await.map_err(AppError::from)?;
    refresh_snapshots(&state).await;
    Ok(Json(result))
}

/// Manual / cron: `GET /api/jobs/otter-attendance?days=7`.
pub async fn sync_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<SyncQuery>,
) -> Result<impl IntoResponse, AppError> {
    assert_job_auth(&state, &headers).await?;
    let result = sync(&state, q.days.unwrap_or(DEFAULT_LOOKBACK_DAYS).clamp(1, 60))
        .await
        .map_err(AppError::from)?;
    refresh_snapshots(&state).await;
    Ok(Json(result))
}

/* ------------------------------------------------------------- site view */

/// Short-lived read cache so a page full of staff doesn't re-read the sheet per click.
fn view_cache() -> &'static tokio::sync::Mutex<BTreeMap<String, (std::time::Instant, Value)>> {
    static CACHE: std::sync::OnceLock<tokio::sync::Mutex<BTreeMap<String, (std::time::Instant, Value)>>> =
        std::sync::OnceLock::new();
    CACHE.get_or_init(|| tokio::sync::Mutex::new(BTreeMap::new()))
}
const VIEW_TTL: Duration = Duration::from_secs(60);
/// A stored view older than this is rebuilt on read (the background job refreshes far sooner).
const SNAPSHOT_MAX_AGE_SECS: i64 = 6 * 60 * 60;

fn snapshot_key(month: Option<&str>) -> String {
    match month {
        Some(m) => format!("otter-attendance:{}", m.trim().to_lowercase()),
        None => "otter-attendance:current".into(),
    }
}

fn snapshot_fresh(body: &Value) -> bool {
    body["generatedAt"]
        .as_str()
        .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
        .map(|t| (Utc::now() - t.with_timezone(&Utc)).num_seconds() < SNAPSHOT_MAX_AGE_SECS)
        .unwrap_or(false)
}

/// Save a built view to Mongo (and memory) under its own month key; the newest month
/// is also saved as "current", which is what the page asks for by default.
async fn store_snapshot(state: &AppState, body: &Value, also_current: bool) {
    let mut keys = Vec::new();
    if let Some(m) = body["month"].as_str() {
        keys.push(snapshot_key(Some(m)));
    }
    if also_current {
        keys.push(snapshot_key(None));
    }
    for key in keys {
        if state.db.is_connected() {
            if let Err(err) = state.db.put_json(&key, body).await {
                tracing::warn!(error = %err, key, "attendance snapshot not saved to MongoDB");
            }
        }
        view_cache().lock().await.insert(key, (std::time::Instant::now(), body.clone()));
    }
}

/// Rebuild the current month's view from the sheet and store it. Run after every sync
/// and on each background pass, so page loads never have to touch Google Sheets.
pub async fn refresh_snapshots(state: &AppState) {
    match build_month_view(state, None).await {
        Ok(body) => store_snapshot(state, &body, true).await,
        Err(err) => tracing::warn!(error = %err, "attendance snapshot refresh failed"),
    }
}

/// Page data for a month: the MongoDB snapshot when it's fresh, otherwise built from the
/// sheet once and stored. Falls back to a short in-memory cache when Mongo is down.
async fn month_view(state: &AppState, month: Option<&str>) -> anyhow::Result<Value> {
    let key = snapshot_key(month);
    if let Some(body) = state.db.get_json(&key).await {
        if snapshot_fresh(&body) {
            return Ok(body);
        }
    } else if let Some((at, body)) = view_cache().lock().await.get(&key) {
        if at.elapsed() < VIEW_TTL {
            return Ok(body.clone());
        }
    }
    let body = build_month_view(state, month).await?;
    store_snapshot(state, &body, month.is_none()).await;
    Ok(body)
}

/// Month tabs present on the workbook, newest first, with the year each belongs to.
fn month_tabs(titles: &[String], today: NaiveDate) -> Vec<(String, i32, u32)> {
    let mut out: Vec<(String, i32, u32)> = titles
        .iter()
        .filter_map(|t| {
            let m = MONTHS.iter().position(|m| m.eq_ignore_ascii_case(t.trim()))? as u32 + 1;
            // Tabs carry no year: a month later than this one is last year's.
            let year = if m > today.month() { today.year() - 1 } else { today.year() };
            Some((t.clone(), year, m))
        })
        .collect();
    out.sort_by(|a, b| (b.1, b.2).cmp(&(a.1, a.2)));
    out
}

/// Everything the attendance page shows for one month: every candidate, every class day,
/// present flag and minutes per session (from `Attendance Log`).
async fn build_month_view(state: &AppState, month: Option<&str>) -> anyhow::Result<Value> {
    let sid = &state.cfg.attendance_spreadsheet_id;
    let today = Utc::now().with_timezone(&Chicago).date_naive();
    let titles = state.google.sheet_titles(sid).await?;
    let tabs = month_tabs(&titles, today);
    let Some((tab, year, mon)) = (match month {
        Some(m) => tabs.iter().find(|(t, _, _)| t.eq_ignore_ascii_case(m)).cloned(),
        None => tabs.first().cloned(),
    }) else {
        return Ok(json!({ "ok": true, "months": [], "days": [], "candidates": [] }));
    };
    // The log tab only exists after the first Meet sync; until then there are no minutes.
    let mut ranges = vec![format!("'{tab}'!A1:BZ500")];
    if titles.iter().any(|t| t == LOG_TAB) {
        ranges.push(format!("'{LOG_TAB}'!A2:I"));
    }
    let mut got = state.google.sheets_values_batch_get(sid, &ranges, Some("FORMATTED_VALUE")).await?;
    let log = if got.len() > 1 { got.remove(1) } else { Vec::new() };
    let grid = got.into_iter().next().unwrap_or_default();
    let header = grid.first().cloned().unwrap_or_default();

    // Class days = date columns on the tab.
    let days: Vec<(usize, NaiveDate)> = header
        .iter()
        .enumerate()
        .filter_map(|(ci, h)| {
            let d: u32 = h.trim().parse().ok()?;
            NaiveDate::from_ymd_opt(year, mon, d).map(|date| (ci, date))
        })
        .collect();

    // Per (sheet row, date): session label → (minutes, join intervals).
    let row_re = regex::Regex::new(r"\(row (\d+)\)\s*$").unwrap();
    let join_re = regex::Regex::new(r"(\d{1,2}:\d{2} [AP]M)\s*[–-]\s*(\d{1,2}:\d{2} [AP]M)\s*\((\d+)m\)").unwrap();
    let mut minutes: BTreeMap<(usize, NaiveDate), BTreeMap<String, (f64, Vec<Value>)>> = BTreeMap::new();
    for r in &log {
        let Some(date) = r.first().and_then(|d| NaiveDate::parse_from_str(d.trim(), "%Y-%m-%d").ok()) else {
            continue;
        };
        if date.year() != year || date.month() != mon {
            continue;
        }
        let Some(row) = r.get(5).and_then(|m| row_re.captures(m)).and_then(|c| c[1].parse::<usize>().ok()) else {
            continue;
        };
        let session = r.get(1).cloned().unwrap_or_default();
        let mins: f64 = r.get(3).and_then(|m| m.trim().parse().ok()).unwrap_or(0.0);
        let joins: Vec<Value> = r
            .get(8)
            .map(|j| {
                join_re
                    .captures_iter(j)
                    .map(|c| json!({ "from": &c[1], "to": &c[2], "minutes": c[3].parse::<f64>().unwrap_or(0.0) }))
                    .collect()
            })
            .unwrap_or_default();
        let slot = minutes.entry((row, date)).or_default().entry(session).or_insert((0.0, Vec::new()));
        slot.0 += mins;
        slot.1.extend(joins);
    }

    let mut candidates = Vec::new();
    for (i, row) in grid.iter().enumerate().skip(1) {
        let name = row.first().map(|s| s.trim().to_string()).unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        let email = norm_email(row.get(1).map(String::as_str).unwrap_or(""));
        let sheet_row = i + 1;
        let mut day_cells = serde_json::Map::new();
        let (mut present_days, mut total_minutes) = (0u32, 0f64);
        for (ci, date) in &days {
            let ticked = row.get(*ci).map(|v| v.trim().eq_ignore_ascii_case("true")).unwrap_or(false);
            let sessions = minutes.get(&(sheet_row, *date)).cloned().unwrap_or_default();
            let day_minutes: f64 = sessions.values().map(|(m, _)| *m).sum();
            if !ticked && sessions.is_empty() {
                continue;
            }
            if ticked {
                present_days += 1;
            }
            total_minutes += day_minutes;
            day_cells.insert(
                date.format("%Y-%m-%d").to_string(),
                json!({
                    "present": ticked,
                    "minutes": day_minutes.round(),
                    "sessions": sessions
                        .iter()
                        .map(|(s, (m, joins))| json!({ "session": s, "minutes": m.round(), "joins": joins }))
                        .collect::<Vec<_>>(),
                }),
            );
        }
        candidates.push(json!({
            "name": name,
            "email": email,
            "row": sheet_row,
            "presentDays": present_days,
            "totalMinutes": total_minutes.round(),
            "days": day_cells,
        }));
    }

    let body = json!({
        "ok": true,
        "generatedAt": Utc::now().to_rfc3339(),
        "month": tab,
        "year": year,
        "months": tabs.iter().map(|(t, y, _)| json!({ "key": t, "label": format!("{t} {y}") })).collect::<Vec<_>>(),
        "today": today.format("%Y-%m-%d").to_string(),
        "minMinutes": state.cfg.attendance_min_minutes,
        "days": days.iter().map(|(_, d)| json!({
            "key": d.format("%Y-%m-%d").to_string(),
            "day": d.day(),
            "dow": d.format("%a").to_string(),
        })).collect::<Vec<_>>(),
        "candidates": candidates,
    });
    Ok(body)
}

#[derive(Deserialize)]
pub struct ViewQuery {
    month: Option<String>,
}

/// Attendance page data. Staff (same rule as the Application Tracker) see every
/// candidate; a candidate sees only their own row, found by sign-in email, then name.
/// Adds each candidate's "Otter Rating" from Current_Market (Data candidate sheet) as
/// `grade`. Read at view time (5-minute cache) so a changed grade shows without waiting
/// for the attendance snapshot. Match by email (Gmail dots ignored), else by a unique
/// exact full name. Best effort: the page still loads if the sheet can't be read.
async fn attach_otter_grades(state: &AppState, body: &mut Value) {
    let Ok(market) = crate::current_market::load(state, Some(Duration::from_secs(300))).await else {
        return;
    };
    let Some(col) = market.otter else { return };
    let mut by_email: BTreeMap<String, String> = BTreeMap::new();
    let mut by_name: BTreeMap<String, Option<String>> = BTreeMap::new();
    for row in &market.rows {
        let grade = market.cell(row, Some(col)).to_string();
        if grade.is_empty() {
            continue;
        }
        let email = norm_email(market.cell(row, market.email));
        if !email.is_empty() {
            by_email.insert(crate::access::gmail_canonical(&email), grade.clone());
        }
        let name = norm_name(market.cell(row, market.name));
        if !name.is_empty() {
            // Two people with the same name: never guess.
            by_name.entry(name).and_modify(|g| *g = None).or_insert(Some(grade));
        }
    }
    let Some(list) = body["candidates"].as_array_mut() else { return };
    for c in list.iter_mut() {
        let email = norm_email(c["email"].as_str().unwrap_or(""));
        let by_mail = if email.is_empty() {
            None
        } else {
            by_email.get(&crate::access::gmail_canonical(&email)).cloned()
        };
        let grade = by_mail.or_else(|| {
            by_name
                .get(&norm_name(c["name"].as_str().unwrap_or("")))
                .cloned()
                .flatten()
        });
        if let Some(g) = grade {
            c["grade"] = json!(g);
        }
    }
}

pub async fn view_route(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<ViewQuery>,
) -> Result<impl IntoResponse, AppError> {
    let user = crate::auth::require_session(&state.cfg, &headers)?;
    let staff = crate::admin::is_admin(&state.cfg, &user.email);
    let mut body = month_view(&state, q.month.as_deref().filter(|m| !m.trim().is_empty()))
        .await
        .map_err(AppError::from)?;
    body["isStaff"] = json!(staff);
    attach_otter_grades(&state, &mut body).await;
    if !staff {
        let me = norm_email(&user.email);
        let list = body["candidates"].as_array().cloned().unwrap_or_default();
        // Privacy: match by sign-in email. A name is only used for a row that has no
        // email yet, and only on an exact full-name match that is unique on the roster,
        // so a candidate can never be handed somebody else's attendance.
        let mut mine: Vec<Value> = if me.is_empty() {
            Vec::new()
        } else {
            list.iter().filter(|c| c["email"].as_str() == Some(me.as_str())).take(1).cloned().collect()
        };
        let want = norm_name(&user.name);
        if mine.is_empty() && want.split(' ').count() >= 2 {
            let same: Vec<&Value> = list
                .iter()
                .filter(|c| norm_name(c["name"].as_str().unwrap_or("")) == want)
                .collect();
            if same.len() == 1 && same[0]["email"].as_str().unwrap_or("").is_empty() {
                mine = vec![same[0].clone()];
            }
        }
        body["candidates"] = json!(mine);
        // Candidates keep the month list: switching months still only ever returns their own row.
    }
    let mut out = HeaderMap::new();
    out.insert(axum::http::header::CACHE_CONTROL, "private, max-age=0, must-revalidate".parse().unwrap());
    Ok((out, Json(body)))
}
#[cfg(test)]
mod tests {
    use super::*;

    fn roster(names: &[&str]) -> Vec<(usize, String)> {
        names.iter().enumerate().map(|(i, n)| (i + 1, n.to_string())).collect()
    }

    #[test]
    fn matches_exact_case_and_spacing() {
        let r = roster(&["Sabina Gurung", "yubraj Shrestha ", "Asmin Neupane"]);
        assert_eq!(match_rows("sabina  gurung", &r), vec![1]);
        assert_eq!(match_rows("Yubraj Shrestha", &r), vec![2]);
    }

    #[test]
    fn matches_joined_and_partial_names() {
        let r = roster(&["SheetalRajPrasai", "Rashmi", "Suman Raman Poudel"]);
        assert_eq!(match_rows("Sheetal Raj Prasai", &r), vec![1]);
        assert_eq!(match_rows("Rashmi Karki", &r), vec![2]);
        assert_eq!(match_rows("Suman Poudel", &r), vec![3]);
    }

    #[test]
    fn duplicate_rows_both_ticked_but_ambiguous_first_name_skipped() {
        let r = roster(&["Badal Shrestha", "Anup Regmi", "Badal Shrestha", "Anjan Regmi"]);
        assert_eq!(match_rows("Badal Shrestha", &r), vec![1, 3]);
        let r2 = roster(&["Anusha Khadka", "Anusha Shrestha"]);
        assert!(match_rows("Anusha", &r2).is_empty());
    }

    #[test]
    fn join_times_are_chicago_clock_with_minutes() {
        let at = |h: u32, m: u32| {
            chrono::TimeZone::with_ymd_and_hms(&Chicago, 2026, 10, 5, h, m, 0)
                .unwrap()
                .with_timezone(&Utc)
        };
        let text = joins_text(&[(at(11, 31), at(11, 52)), (at(11, 55), at(12, 30))]);
        assert_eq!(text, "11:31 AM–11:52 AM (21m); 11:55 AM–12:30 PM (35m)");
    }

    #[test]
    fn clock_minutes_match_the_shown_times() {
        let at = |h: u32, m: u32, sec: u32| {
            chrono::TimeZone::with_ymd_and_hms(&Chicago, 2026, 10, 5, h, m, sec)
                .unwrap()
                .with_timezone(&Utc)
        };
        // 11:30:50 → 12:17:10 is shown as 11:30–12:17, so it must count as 47, not 46.
        assert_eq!(clock_minutes(at(11, 30, 50), at(12, 17, 10)), 47.0);
        assert_eq!(clock_minutes(at(14, 7, 0), at(14, 47, 59)), 40.0);
    }

    #[test]
    fn calls_are_labelled_by_scheduled_class() {
        let local = |h: u32, m: u32| chrono::TimeZone::with_ymd_and_hms(&Chicago, 2026, 10, 5, h, m, 0).unwrap();
        assert_eq!(session_label(&local(11, 26)), "11:30 AM");
        assert_eq!(session_label(&local(11, 31)), "11:30 AM");
        assert_eq!(session_label(&local(13, 58)), "2:00 PM");
    }

    #[test]
    fn month_header_has_week_gaps() {
        // October 2026 starts on a Thursday.
        let h = month_header(2026, 10);
        assert_eq!(&h[..6], &["", "", "1", "2", "", "5"]);
        assert!(!h.contains(&"3".to_string()));
    }
}
