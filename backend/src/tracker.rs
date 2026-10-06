use std::collections::{BTreeMap, HashSet};

use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::Json;
use chrono::{Datelike, Duration, NaiveDate};
use chrono_tz::America::Chicago;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::require_session;
use crate::dates::today_date_key;
use crate::error::AppError;
use crate::jobs::readers::{
    read_data_interview_history, read_interview_records, read_phone_call_records,
};
use crate::staff::normalize_email;
use crate::types::{BookingRecord, HistoryRecord, SheetKind};
use crate::AppState;

/// Months of Data Interview Sheet history to read. A 12-month window keeps the
/// bare month-tab names (`December`) unambiguous — see `recent_month_tab_names`.
const HISTORY_MONTHS: u32 = 12;

/// The month tabs are large and change slowly; re-read them at most this often.
const HISTORY_TTL: std::time::Duration = std::time::Duration::from_secs(15 * 60);
/// The admin roster (Current_Market) changes rarely — a short cache turns three
/// Sheets round-trips per tracker open into one every couple of minutes.
const CANDIDATES_TTL: std::time::Duration = std::time::Duration::from_secs(120);

/// How far back to walk apply-tracker month tabs (`September_2026` and bare `March`).
const APPLY_HISTORY_MONTHS: u32 = 24;

#[derive(Clone)]
struct HistorySnapshot {
    fetched_at: std::time::Instant,
    records: std::sync::Arc<Vec<HistoryRecord>>,
}

#[derive(Clone)]
struct ApplyTab {
    title: String,
    year: i32,
    month: u32,
    label: String,
    rows: Vec<Vec<String>>,
}

#[derive(Clone)]
struct ApplyTabsSnapshot {
    fetched_at: std::time::Instant,
    tabs: std::sync::Arc<Vec<ApplyTab>>,
}

#[derive(Clone)]
struct CandidatesSnapshot {
    fetched_at: std::time::Instant,
    people: std::sync::Arc<Vec<Person>>,
}

#[derive(Clone)]
pub struct HistoryCache {
    snapshot: std::sync::Arc<tokio::sync::Mutex<Option<HistorySnapshot>>>,
    apply_tabs: std::sync::Arc<tokio::sync::Mutex<Option<ApplyTabsSnapshot>>>,
    candidates: std::sync::Arc<tokio::sync::Mutex<Option<CandidatesSnapshot>>>,
}

impl HistoryCache {
    pub fn new() -> Self {
        Self {
            snapshot: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
            apply_tabs: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
            candidates: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    pub async fn invalidate_apply_tabs(&self) {
        *self.apply_tabs.lock().await = None;
    }
}

/// Cached wrapper over `list_candidates` — serves a stale roster on a transient
/// Sheets error rather than failing the whole tracker.
async fn cached_candidates(state: &AppState) -> anyhow::Result<std::sync::Arc<Vec<Person>>> {
    {
        let snap = state.history.candidates.lock().await;
        if let Some(current) = snap.as_ref() {
            if current.fetched_at.elapsed() < CANDIDATES_TTL {
                return Ok(current.people.clone());
            }
        }
    }
    match list_candidates(state).await {
        Ok(rows) => {
            let people = std::sync::Arc::new(rows);
            *state.history.candidates.lock().await = Some(CandidatesSnapshot {
                fetched_at: std::time::Instant::now(),
                people: people.clone(),
            });
            Ok(people)
        }
        Err(err) => {
            let snap = state.history.candidates.lock().await;
            if let Some(current) = snap.as_ref() {
                tracing::warn!(error = %err, "candidate roster read failed — serving cached copy");
                Ok(current.people.clone())
            } else {
                Err(err)
            }
        }
    }
}

async fn history_records(state: &AppState) -> std::sync::Arc<Vec<HistoryRecord>> {
    {
        let snap = state.history.snapshot.lock().await;
        if let Some(current) = snap.as_ref() {
            if current.fetched_at.elapsed() < HISTORY_TTL {
                return current.records.clone();
            }
        }
    }
    match read_data_interview_history(state, HISTORY_MONTHS).await {
        Ok(rows) => {
            let records = std::sync::Arc::new(rows);
            *state.history.snapshot.lock().await = Some(HistorySnapshot {
                fetched_at: std::time::Instant::now(),
                records: records.clone(),
            });
            records
        }
        Err(err) => {
            tracing::warn!(error = %err, "application tracker history read failed");
            // Serve the stale copy rather than dropping history on a transient error.
            let snap = state.history.snapshot.lock().await;
            snap.as_ref()
                .map(|c| c.records.clone())
                .unwrap_or_else(|| std::sync::Arc::new(Vec::new()))
        }
    }
}

fn cell_at(row: &[String], index: i32) -> &str {
    if index < 0 {
        return "";
    }
    row.get(index as usize).map(|s| s.as_str()).unwrap_or("")
}

fn normalize_name(name: &str) -> String {
    let base = name
        .trim()
        .split('(')
        .next()
        .unwrap_or(name)
        .trim()
        .to_lowercase()
        .replace('_', " ")
        .replace('\u{00a0}', " ");
    let cleaned: String = base
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c.is_whitespace() {
                c
            } else {
                ' '
            }
        })
        .collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn display_name(name: &str) -> String {
    name.trim()
        .split('(')
        .next()
        .unwrap_or(name)
        .trim()
        .to_string()
}

fn month_tab_name() -> String {
    let now = chrono::Utc::now().with_timezone(&Chicago);
    format!("{}_{}", now.format("%B"), now.year())
}

fn parse_count(raw: &str) -> i32 {
    let t = raw.trim();
    if t.is_empty() {
        return 0;
    }
    t.parse::<f64>().ok().map(|n| n as i32).unwrap_or(0).max(0)
}

/// Current_Market roster columns shown to admins alongside each candidate.
#[derive(Clone, Default)]
struct CandidateDetail {
    mkt_start_date: String,
    ead_end_date: String,
    /// Visa / work-auth status from the Status column (STEM, OPT EAD, …).
    visa_status: String,
    /// Marketing stage (Active - Marketing, …).
    market_stage: String,
    marketing_location: String,
    nepal_poc: String,
    current_location: String,
    phone: String,
}

struct Person {
    name: String,
    /// Preferred Sign In / Current_Market email for display.
    email: String,
    /// All known emails (personal + marketing / tracker) for matching.
    emails: Vec<String>,
    /// Roster detail, empty for people known only from the month tracker tab.
    detail: CandidateDetail,
}

fn candidate_json(p: &Person) -> Value {
    json!({
        "name": p.name,
        "email": p.email,
        "mktStartDate": p.detail.mkt_start_date,
        "eadEndDate": p.detail.ead_end_date,
        "visaStatus": p.detail.visa_status,
        "marketStage": p.detail.market_stage,
        "marketingLocation": p.detail.marketing_location,
        "nepalPoc": p.detail.nepal_poc,
        "currentLocation": p.detail.current_location,
        "phone": p.detail.phone,
    })
}

fn detail_is_filled(detail: &CandidateDetail) -> bool {
    !detail.mkt_start_date.is_empty()
        || !detail.market_stage.is_empty()
        || !detail.marketing_location.is_empty()
        || !detail.phone.is_empty()
}

fn absorb_person(into: &mut Person, other: Person) {
    for email in other.emails {
        if !email.is_empty() && !into.emails.iter().any(|e| e == &email) {
            into.emails.push(email);
        }
    }
    if into.email.is_empty() {
        into.email = other.email.clone();
    }
    if !detail_is_filled(&into.detail) && detail_is_filled(&other.detail) {
        into.detail = other.detail;
        if !other.name.is_empty() {
            into.name = other.name;
        }
    }
}

/// Same person can land twice when Current_Market and the month tab spell the
/// name differently, or when two emails belong to one roster row.
fn merge_duplicate_people(people: Vec<Person>) -> Vec<Person> {
    let mut out: Vec<Person> = Vec::new();
    for person in people {
        let name_key = normalize_name(&person.name);
        let idx = out.iter().position(|existing| {
            let same_name = !name_key.is_empty() && normalize_name(&existing.name) == name_key;
            let share_email = person.emails.iter().any(|e| {
                !e.is_empty() && existing.emails.iter().any(|known| known == e)
            });
            same_name || share_email
        });
        match idx {
            Some(i) => absorb_person(&mut out[i], person),
            None => out.push(person),
        }
    }
    out
}

fn person_with_emails(name: String, primary: String, mut emails: Vec<String>) -> Person {
    let primary = normalize_email(&primary);
    if !primary.is_empty() && !emails.iter().any(|e| e == &primary) {
        emails.insert(0, primary.clone());
    }
    emails.sort();
    emails.dedup();
    Person {
        name,
        email: primary,
        emails,
        detail: CandidateDetail::default(),
    }
}

fn email_matches(person: &Person, raw: &str) -> bool {
    let e = normalize_email(raw);
    if e.is_empty() {
        return false;
    }
    if person.emails.iter().any(|known| known == &e) {
        return true;
    }
    let canon = crate::access::gmail_canonical(&e);
    person.emails.iter().any(|known| crate::access::gmail_canonical(known) == canon)
}

/// Delegates to `admin::is_admin` so the tracker's roster view and the `/admin`
/// pages can never disagree about who is staff.
fn is_tracker_admin(cfg: &crate::config::Config, email: &str) -> bool {
    crate::admin::is_admin(cfg, email)
}

pub(crate) async fn tracking_name_email_rows(state: &AppState, month_tab: &str) -> Vec<(String, String)> {
    let rows = state
        .google
        .sheets_values_get(
            &state.cfg.hiring_spreadsheet_id,
            &format!("'{month_tab}'!A1:Z200"),
            Some("FORMATTED_VALUE"),
        )
        .await
        .unwrap_or_default();
    let headers = apply_headers(&rows);
    let header_idx = (headers.row_1 as usize).saturating_sub(1);
    rows.iter()
        .enumerate()
        .filter_map(|(i, row)| {
            if i == header_idx || i < 2 {
                return None;
            }
            let name = display_name(headers.get(row, crate::headers::col::APPLY_NAME));
            let email = normalize_email(headers.get(row, crate::headers::col::APPLY_MAIL));
            Some((name, email))
        })
        .collect()
}

async fn resolve_person_from_tracking(state: &AppState, email: &str) -> anyhow::Result<Option<Person>> {
    let email = normalize_email(email);
    if email.is_empty() {
        return Ok(None);
    }
    let month_tab = month_tab_name();
    let rows = tracking_name_email_rows(state, &month_tab).await;
    let canon = crate::access::gmail_canonical(&email);
    for (name, row_email) in rows {
        if row_email != email && crate::access::gmail_canonical(&row_email) != canon {
            continue;
        }
        if name.is_empty() {
            continue;
        }
        return Ok(Some(person_with_emails(name, email.clone(), vec![email])));
    }
    Ok(None)
}

async fn resolve_person(state: &AppState, email: &str) -> anyhow::Result<Option<Person>> {
    let email = normalize_email(email);
    if email.is_empty() {
        return Ok(None);
    }
    let market = crate::current_market::load(state, None).await?;
    for row in &market.rows {
        let row_email = normalize_email(market.cell(row, market.email));
        if row_email != email && crate::access::gmail_canonical(&row_email) != crate::access::gmail_canonical(&email) {
            continue;
        }
        let name = display_name(market.cell(row, market.name));
        if name.is_empty() {
            continue;
        }
        let mut person = person_with_emails(name, row_email.clone(), vec![row_email]);
        person = enrich_person_emails(state, person).await?;
        return Ok(Some(person));
    }
    if let Some(person) = resolve_person_from_tracking(state, &email).await? {
        return Ok(Some(enrich_person_emails(state, person).await?));
    }
    Ok(None)
}

/// Attach every email seen for this candidate name (Current_Market + month tracker).
async fn enrich_person_emails(state: &AppState, mut person: Person) -> anyhow::Result<Person> {
    let key = normalize_name(&person.name);
    if key.is_empty() {
        return Ok(person);
    }
    let mut emails = person.emails.clone();

    if let Ok(market) = crate::current_market::load(state, None).await {
        for row in &market.rows {
            let name = display_name(market.cell(row, market.name));
            if normalize_name(&name) != key {
                continue;
            }
            let email = normalize_email(market.cell(row, market.email));
            if !email.is_empty() {
                emails.push(email);
            }
            if person.name.is_empty() {
                person.name = name;
            }
        }
    }

    let month_tab = month_tab_name();
    let tracking = tracking_name_email_rows(state, &month_tab).await;
    for (name, email) in tracking {
        if normalize_name(&name) != key {
            continue;
        }
        if !email.is_empty() {
            emails.push(email);
        }
    }

    emails.sort();
    emails.dedup();
    if person.email.is_empty() {
        person.email = emails.first().cloned().unwrap_or_default();
    }
    person.emails = emails;
    Ok(person)
}

async fn list_candidates(state: &AppState) -> anyhow::Result<Vec<Person>> {
    // One row per candidate name. Prefer Current_Market (personal) email over tracker/marketing.
    let mut by_name: BTreeMap<String, Person> = BTreeMap::new();

    // Header driven: Current_Market row 1 is the header, row 2 a spacer, data
    // from row 3. Admins see the roster columns, so a row without an email is
    // still listed — only its tracker link is unavailable.
    let market = state
        .google
        .sheets_values_get(
            &state.cfg.data_candidate_spreadsheet_id,
            "Current_Market!A1:Z",
            Some("FORMATTED_VALUE"),
        )
        .await
        .unwrap_or_default();
    // Candidates whose Current_Market row is filled red are off the roster, so
    // read the row fills alongside the values and skip those rows.
    let red_rows: std::collections::HashSet<usize> = match state
        .google
        .grid_data(
            &state.cfg.data_candidate_spreadsheet_id,
            "Current_Market!A1:C400",
            "sheets(data(rowData(values(effectiveFormat(backgroundColor)))))",
        )
        .await
    {
        Ok(rows) => rows
            .iter()
            .enumerate()
            .filter(|(_, row)| crate::google::sheets::row_is_red(row))
            .map(|(i, _)| i)
            .collect(),
        Err(err) => {
            tracing::warn!(error = %err, "current_market row colours unavailable");
            std::collections::HashSet::new()
        }
    };

    if market.len() > 2 {
        let headers: Vec<String> = market[0].iter().map(|h| h.trim().to_lowercase()).collect();
        let find = |needle: &str| -> i32 {
            headers
                .iter()
                .position(|h| h.contains(needle))
                .map(|i| i as i32)
                .unwrap_or(-1)
        };
        let col_name = find("full name");
        let col_email = find("email");
        let col_start = find("mkt start");
        let col_ead = find("ead end");
        let col_visa = find("status");
        let col_stage = find("stage");
        let col_mkt_loc = find("marketing location");
        let col_poc = find("nepal poc");
        let col_cur_loc = find("current location");
        let col_phone = find("phone");

        for (i, row) in market.iter().enumerate().skip(2) {
            if red_rows.contains(&i) {
                continue;
            }
            let name = display_name(cell_at(row, col_name));
            let key = normalize_name(&name);
            // "Total" and friends are summary rows, not people.
            if key.is_empty() || matches!(key.as_str(), "total" | "totals" | "grand total") {
                continue;
            }
            let email = normalize_email(cell_at(row, col_email));
            let detail = CandidateDetail {
                mkt_start_date: cell_at(row, col_start).trim().to_string(),
                ead_end_date: cell_at(row, col_ead).trim().to_string(),
                visa_status: cell_at(row, col_visa).trim().to_string(),
                market_stage: cell_at(row, col_stage).trim().to_string(),
                marketing_location: cell_at(row, col_mkt_loc).trim().to_string(),
                nepal_poc: cell_at(row, col_poc).trim().to_string(),
                current_location: cell_at(row, col_cur_loc).trim().to_string(),
                phone: cell_at(row, col_phone).trim().to_string(),
            };
            match by_name.get_mut(&key) {
                None => {
                    let emails = if email.is_empty() { Vec::new() } else { vec![email.clone()] };
                    let mut person = person_with_emails(name, email, emails);
                    person.detail = detail;
                    by_name.insert(key, person);
                }
                Some(existing) => {
                    if !email.is_empty() && !existing.emails.iter().any(|e| e == &email) {
                        existing.emails.push(email);
                    }
                    // Keep first Current_Market email as primary.
                }
            }
        }
    }

    let month_tab = month_tab_name();
    let tracking = tracking_name_email_rows(state, &month_tab).await;
    for (name, email) in tracking {
        let key = normalize_name(&name);
        if key.is_empty() || email.is_empty() {
            continue;
        }
        if by_name.values().any(|p| p.emails.iter().any(|e| e == &email)) {
            continue;
        }
        // Only attach this email to someone already on the Current_Market roster.
        // A name that appears only on a month tracking tab is not a roster
        // candidate and must not be listed in the admin candidate view.
        if let Some(existing) = by_name.get_mut(&key) {
            if !existing.emails.iter().any(|e| e == &email) {
                existing.emails.push(email);
            }
        }
    }

    let mut people: Vec<Person> = merge_duplicate_people(by_name.into_values().collect());
    for p in &mut people {
        p.emails.sort();
        p.emails.dedup();
        if p.email.is_empty() {
            p.email = p.emails.first().cloned().unwrap_or_default();
        }
    }
    people.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.email.cmp(&b.email))
    });
    Ok(people)
}

struct ApplyStats {
    month_tab: String,
    total: i32,
    this_week: i32,
    by_day: Vec<Value>,
    by_week: Vec<Value>,
    matched: bool,
    lifetime: i32,
    today: i32,
    by_month: Vec<Value>,
    months: Vec<Value>,
    weeks: Vec<Value>,
}

fn month_label(year: i32, month: u32) -> String {
    let short = NaiveDate::from_ymd_opt(year, month, 1)
        .map(|d| d.format("%b").to_string())
        .unwrap_or_else(|| month.to_string());
    format!("{short} {:02}", year.rem_euclid(100))
}

/// Dated tabs (`September_2026`) win for the current walk year; leftover bare
/// names (`March`, `Feb`) map to the first unmatched year going backwards.
fn apply_tab_plan(titles: &[String]) -> Vec<(String, i32, u32, String)> {
    let today = chrono::Utc::now().with_timezone(&Chicago).date_naive();
    let current_year = today.year();
    let mut year = current_year;
    let mut month = today.month();
    let mut used = HashSet::new();
    let mut out = Vec::new();
    for _ in 0..APPLY_HISTORY_MONTHS {
        let full = NaiveDate::from_ymd_opt(year, month, 1)
            .map(|d| d.format("%B").to_string())
            .unwrap_or_default();
        let short = NaiveDate::from_ymd_opt(year, month, 1)
            .map(|d| d.format("%b").to_string())
            .unwrap_or_default();
        if full.is_empty() {
            continue;
        }
        let dated = format!("{full}_{year}");
        // Bare month tabs (`March`, `Feb`) are the pre-dated-naming legacy and
        // always belong to an earlier year — never let one stand in for a
        // current-year month, or e.g. last year's `March` gets counted as this
        // year's March.
        let found = if year < current_year {
            titles
                .iter()
                .find(|t| t.trim() == dated)
                .or_else(|| titles.iter().find(|t| t.trim().eq_ignore_ascii_case(&full)))
                .or_else(|| titles.iter().find(|t| t.trim().eq_ignore_ascii_case(&short)))
                .cloned()
        } else {
            titles.iter().find(|t| t.trim() == dated).cloned()
        };
        if let Some(title) = found {
            if used.insert(title.clone()) {
                out.push((title, year, month, month_label(year, month)));
            }
        }
        if month == 1 {
            month = 12;
            year -= 1;
        } else {
            month -= 1;
        }
    }
    out
}

fn apply_headers(rows: &[Vec<String>]) -> crate::headers::SheetHeaders {
    crate::headers::SheetHeaders::detect(rows.get(..3).unwrap_or(rows))
}

fn find_apply_row<'a>(rows: &'a [Vec<String>], person: &Person) -> Option<&'a Vec<String>> {
    if rows.len() < 3 {
        return None;
    }
    let headers = apply_headers(rows);
    let target_name = normalize_name(&person.name);
    let header_idx = (headers.row_1 as usize).saturating_sub(1);
    for (i, row) in rows.iter().enumerate() {
        if i == header_idx || i < 2 {
            continue;
        }
        let name = normalize_name(headers.get(row, crate::headers::col::APPLY_NAME));
        let email = normalize_email(headers.get(row, crate::headers::col::APPLY_MAIL));
        if (!target_name.is_empty() && name == target_name) || email_matches(person, &email) {
            return Some(row);
        }
    }
    None
}

fn find_apply_row_index(rows: &[Vec<String>], person: &Person) -> Option<usize> {
    if rows.len() < 3 {
        return None;
    }
    let headers = apply_headers(rows);
    let target_name = normalize_name(&person.name);
    let header_idx = (headers.row_1 as usize).saturating_sub(1);
    for (i, row) in rows.iter().enumerate() {
        if i == header_idx || i < 2 {
            continue;
        }
        let name = normalize_name(headers.get(row, crate::headers::col::APPLY_NAME));
        let email = normalize_email(headers.get(row, crate::headers::col::APPLY_MAIL));
        if (!target_name.is_empty() && name == target_name) || email_matches(person, &email) {
            return Some(i);
        }
    }
    None
}

fn a1_col(index0: usize) -> String {
    let mut n = (index0 + 1) as i32;
    let mut out = String::new();
    while n > 0 {
        let rem = (n - 1) % 26;
        out.insert(0, char::from(b'A' + rem as u8));
        n = (n - 1) / 26;
    }
    out
}

fn day_column_total(header_day: &[String], row: &[String]) -> i32 {
    let max_cols = header_day.len().max(row.len());
    let mut total = 0i32;
    for col in 2..max_cols {
        let day_raw = header_day.get(col).map(|s| s.as_str()).unwrap_or("").trim();
        if day_raw.parse::<u32>().unwrap_or(0) == 0 {
            continue;
        }
        total += parse_count(row.get(col).map(|s| s.as_str()).unwrap_or(""));
    }
    total
}

/// Every dated apply-count column on a month tab, including weekends.
fn apply_counts_by_date(tab: &ApplyTab, person: &Person) -> BTreeMap<NaiveDate, i32> {
    let mut out = BTreeMap::new();
    if tab.rows.len() < 3 {
        return out;
    }
    let Some(row) = find_apply_row(&tab.rows, person) else {
        return out;
    };
    let header_day = &tab.rows[1];
    let max_cols = header_day.len().max(row.len());
    for col in 2..max_cols {
        let day_raw = header_day.get(col).map(|s| s.as_str()).unwrap_or("").trim();
        let day_num: u32 = day_raw.parse().unwrap_or(0);
        if day_num == 0 {
            continue;
        }
        let Some(date) = NaiveDate::from_ymd_opt(tab.year, tab.month, day_num) else {
            continue;
        };
        let count = parse_count(row.get(col).map(|s| s.as_str()).unwrap_or(""));
        *out.entry(date).or_insert(0) += count;
    }
    out
}

fn apply_day_json(date: NaiveDate, count: i32, today: NaiveDate, week_start: NaiveDate, week_end: NaiveDate) -> Value {
    json!({
        "day": date.day(),
        "label": date.format("%a").to_string(),
        "count": count,
        "isToday": date == today,
        "inThisWeek": date >= week_start && date <= week_end,
    })
}

fn month_period(
    counts: &BTreeMap<NaiveDate, i32>,
    year: i32,
    month: u32,
    label: &str,
    today: NaiveDate,
    week_start: NaiveDate,
) -> Value {
    let week_end = week_start + Duration::days(6);
    let last = days_in_month(year, month);
    let mut total = 0i32;
    let mut by_day = Vec::new();
    for day in 1..=last {
        let Some(date) = NaiveDate::from_ymd_opt(year, month, day) else {
            continue;
        };
        let count = *counts.get(&date).unwrap_or(&0);
        total += count;
        by_day.push(apply_day_json(date, count, today, week_start, week_end));
    }
    json!({
        "key": format!("{year}-{month:02}"),
        "label": label,
        "total": total,
        "isCurrent": year == today.year() && month == today.month(),
        "byDay": by_day,
    })
}

fn week_period(
    counts: &BTreeMap<NaiveDate, i32>,
    start: NaiveDate,
    today: NaiveDate,
    current_week_start: NaiveDate,
) -> Value {
    let end = start + Duration::days(6);
    let mut total = 0i32;
    let mut by_day = Vec::new();
    for i in 0..7 {
        let date = start + Duration::days(i);
        let count = *counts.get(&date).unwrap_or(&0);
        total += count;
        by_day.push(apply_day_json(date, count, today, start, end));
    }
    let label = if start.month() == end.month() && start.year() == end.year() {
        format!("{} {}–{}", start.format("%b"), start.day(), end.day())
    } else {
        format!(
            "{} {} – {} {}",
            start.format("%b"),
            start.day(),
            end.format("%b"),
            end.day()
        )
    };
    json!({
        "key": start.format("%Y-%m-%d").to_string(),
        "label": label,
        "total": total,
        "isCurrent": start == current_week_start,
        "byDay": by_day,
    })
}

fn week_periods(
    counts: &BTreeMap<NaiveDate, i32>,
    today: NaiveDate,
    week_start: NaiveDate,
    n: u32,
) -> Vec<Value> {
    let mut out = Vec::new();
    let mut start = week_start;
    for _ in 0..n {
        out.push(week_period(counts, start, today, week_start));
        start -= Duration::days(7);
    }
    out
}

fn lifetime_from_tabs(tabs: &[ApplyTab], person: &Person) -> (i32, Vec<Value>) {
    let mut lifetime = 0i32;
    let mut by_month = Vec::new();
    for tab in tabs.iter().rev() {
        let value = if tab.rows.len() < 3 {
            0
        } else if let Some(row) = find_apply_row(&tab.rows, person) {
            day_column_total(&tab.rows[1], row)
        } else {
            0
        };
        lifetime += value;
        by_month.push(json!({
            "label": tab.label,
            "tab": tab.title,
            "value": value,
            "isCurrent": tab.title == month_tab_name(),
        }));
    }
    (lifetime, by_month)
}

async fn fetch_apply_tabs(state: &AppState) -> anyhow::Result<Vec<ApplyTab>> {
    let titles = state
        .google
        .sheet_titles_cached(&state.cfg.hiring_spreadsheet_id)
        .await?;
    let plan = apply_tab_plan(&titles);
    if plan.is_empty() {
        return Ok(Vec::new());
    }
    let ranges: Vec<String> = plan
        .iter()
        .map(|(title, _, _, _)| format!("'{title}'!A1:AZ200"))
        .collect();
    let matrices = state
        .google
        .sheets_values_batch_get(
            &state.cfg.hiring_spreadsheet_id,
            &ranges,
            Some("FORMATTED_VALUE"),
        )
        .await?;
    let mut tabs = Vec::new();
    for (i, (title, year, month, label)) in plan.into_iter().enumerate() {
        let rows = matrices.get(i).cloned().unwrap_or_default();
        tabs.push(ApplyTab {
            title,
            year,
            month,
            label,
            rows,
        });
    }
    Ok(tabs)
}

async fn apply_tabs(state: &AppState) -> std::sync::Arc<Vec<ApplyTab>> {
    {
        let snap = state.history.apply_tabs.lock().await;
        if let Some(current) = snap.as_ref() {
            if current.fetched_at.elapsed() < HISTORY_TTL {
                return current.tabs.clone();
            }
        }
    }
    match fetch_apply_tabs(state).await {
        Ok(tabs) => {
            let tabs = std::sync::Arc::new(tabs);
            *state.history.apply_tabs.lock().await = Some(ApplyTabsSnapshot {
                fetched_at: std::time::Instant::now(),
                tabs: tabs.clone(),
            });
            tabs
        }
        Err(err) => {
            tracing::warn!(error = %err, "application tracker apply-tab read failed");
            let snap = state.history.apply_tabs.lock().await;
            snap.as_ref()
                .map(|c| c.tabs.clone())
                .unwrap_or_else(|| std::sync::Arc::new(Vec::new()))
        }
    }
}

async fn load_apply_stats(state: &AppState, person: &Person) -> anyhow::Result<ApplyStats> {
    let tabs = apply_tabs(state).await;
    let now = chrono::Utc::now().with_timezone(&Chicago);
    let year = now.year();
    let month = now.month();
    let today = now.date_naive();
    let week_start = today - Duration::days(today.weekday().num_days_from_monday() as i64);
    let week_end = week_start + Duration::days(6);
    let month_tab = tabs
        .iter()
        .find(|t| t.year == year && t.month == month)
        .map(|t| t.title.clone())
        .unwrap_or_else(month_tab_name);

    let mut extra_current: Option<ApplyTab> = None;
    let has_current = tabs.iter().any(|t| t.year == year && t.month == month);
    if !has_current {
        let rows = match state
            .google
            .sheets_values_get(
                &state.cfg.hiring_spreadsheet_id,
                &format!("'{month_tab}'!A1:AZ200"),
                Some("FORMATTED_VALUE"),
            )
            .await
        {
            Ok(rows) => rows,
            Err(err) => {
                tracing::warn!(error = %err, tab = %month_tab, "application tracker month tab missing");
                Vec::new()
            }
        };
        extra_current = Some(ApplyTab {
            title: month_tab.clone(),
            year,
            month,
            label: month_label(year, month),
            rows,
        });
    }

    let mut date_counts: BTreeMap<NaiveDate, i32> = BTreeMap::new();
    let mut matched = false;
    for tab in tabs.iter().chain(extra_current.iter()) {
        if tab.year == year && tab.month == month && find_apply_row(&tab.rows, person).is_some() {
            matched = true;
        }
        for (date, count) in apply_counts_by_date(tab, person) {
            *date_counts.entry(date).or_insert(0) += count;
        }
    }

    let last_day = days_in_month(year, month);
    let mut total = 0i32;
    let mut by_day = Vec::new();
    for day in 1..=last_day {
        let Some(date) = NaiveDate::from_ymd_opt(year, month, day) else {
            continue;
        };
        let count = *date_counts.get(&date).unwrap_or(&0);
        total += count;
        by_day.push(apply_day_json(date, count, today, week_start, week_end));
    }

    let mut this_week = 0i32;
    let mut by_week = Vec::new();
    for i in 0..7 {
        let date = week_start + Duration::days(i);
        // A Mon–Sun week that spills into the previous month must not make
        // "this week" larger than "this month" — only count days in this month.
        let in_month = date.year() == year && date.month() == month;
        let count = if in_month {
            *date_counts.get(&date).unwrap_or(&0)
        } else {
            0
        };
        this_week += count;
        by_week.push(apply_day_json(date, count, today, week_start, week_end));
    }

    let mut months = Vec::new();
    let mut seen_month = HashSet::new();
    for tab in tabs.iter().chain(extra_current.iter()) {
        let key = format!("{}-{:02}", tab.year, tab.month);
        if !seen_month.insert(key) {
            continue;
        }
        months.push(month_period(
            &date_counts,
            tab.year,
            tab.month,
            &tab.label,
            today,
            week_start,
        ));
    }
    if months.is_empty() {
        months.push(month_period(
            &date_counts,
            year,
            month,
            &month_label(year, month),
            today,
            week_start,
        ));
    }
    let weeks = week_periods(&date_counts, today, week_start, 16);

    let (lifetime, by_month) = lifetime_from_tabs(&tabs, person);
    let lifetime = if lifetime == 0 && total > 0 {
        total
    } else {
        lifetime
    };

    Ok(ApplyStats {
        month_tab,
        total,
        this_week,
        by_day,
        by_week,
        matched,
        lifetime,
        today: *date_counts.get(&today).unwrap_or(&0),
        by_month,
        months,
        weeks,
    })
}

/// Latest interview status per company → pie buckets.
/// Apply counts live on the lifetime chart; this pie is outcomes, not mixed units.
fn outcome_bucket(status: &str) -> &'static str {
    let s = status.trim().to_lowercase();
    if s.is_empty() {
        return "pending";
    }
    if s.contains("reject")
        || s.contains("declin")
        || s.contains("no show")
        || s.contains("noshow")
        || s.contains("withdraw")
        || s.contains("drop")
    {
        return "rejected";
    }
    if s.contains("offer") || s.contains("hired") || s.contains("accept") || s.contains("select")
    {
        return "offered";
    }
    "pending"
}

fn outcome_counts(companies: &[CompanyAgg]) -> (i32, i32, i32) {
    let mut pending = 0i32;
    let mut rejected = 0i32;
    let mut offered = 0i32;
    for c in companies {
        match outcome_bucket(&c.status) {
            "rejected" => rejected += 1,
            "offered" => offered += 1,
            _ => pending += 1,
        }
    }
    (pending, rejected, offered)
}

fn record_belongs(record: &BookingRecord, person: &Person) -> bool {
    let name = normalize_name(&person.name);
    let candidate = normalize_name(&record.candidate_name);
    let name_ok = !name.is_empty() && candidate == name;
    // Phone calls store the booker in Submitter Email, not the candidate.
    if record.kind == SheetKind::Phone {
        return name_ok;
    }
    email_matches(person, &record.submitter_email) || name_ok
}

fn month_short_label(year: i32, month: u32) -> String {
    NaiveDate::from_ymd_opt(year, month, 1)
        .map(|d| format!("{} {:02}", d.format("%b"), year.rem_euclid(100)))
        .unwrap_or_else(|| format!("{month:02} {year}"))
}

fn days_in_month(year: i32, month: u32) -> u32 {
    if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)
    }
    .and_then(|d| d.pred_opt())
    .map(|d| d.day())
    .unwrap_or(28)
}

struct PhoneStats {
    total: i32,
    this_month: i32,
    this_week: i32,
    by_day: Vec<Value>,
    by_week: Vec<Value>,
    by_month: Vec<Value>,
    months: Vec<Value>,
    weeks: Vec<Value>,
}

fn phone_stats(records: &[BookingRecord]) -> PhoneStats {
    let now = chrono::Utc::now().with_timezone(&Chicago);
    let year = now.year();
    let month = now.month();
    let today = now.date_naive();
    let week_start = today - Duration::days(today.weekday().num_days_from_monday() as i64);

    let mut date_counts: BTreeMap<NaiveDate, i32> = BTreeMap::new();
    let mut month_counts: BTreeMap<(i32, u32), i32> = BTreeMap::new();
    let mut y = year;
    let mut m = month;
    for _ in 0..12 {
        month_counts.insert((y, m), 0);
        if m == 1 {
            m = 12;
            y -= 1;
        } else {
            m -= 1;
        }
    }

    let mut total = 0i32;
    for r in records.iter().filter(|r| r.kind == SheetKind::Phone) {
        let Some(date) = NaiveDate::parse_from_str(&r.date_key, "%Y-%m-%d").ok() else {
            continue;
        };
        total += 1;
        *date_counts.entry(date).or_insert(0) += 1;
        *month_counts.entry((date.year(), date.month())).or_insert(0) += 1;
    }

    let current_month = month_period(
        &date_counts,
        year,
        month,
        &month_label(year, month),
        today,
        week_start,
    );
    let current_week = week_period(&date_counts, week_start, today, week_start);
    let this_month = current_month
        .get("total")
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i32;
    let this_week = current_week
        .get("total")
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i32;
    let by_day = current_month
        .get("byDay")
        .cloned()
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default();
    let by_week = current_week
        .get("byDay")
        .cloned()
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default();

    let mut months = Vec::new();
    let mut keys: Vec<(i32, u32)> = month_counts.keys().copied().collect();
    keys.sort_by(|a, b| b.cmp(a));
    for (y, m) in keys {
        months.push(month_period(
            &date_counts,
            y,
            m,
            &month_short_label(y, m),
            today,
            week_start,
        ));
    }
    let weeks = week_periods(&date_counts, today, week_start, 16);

    let mut by_month = Vec::new();
    let mut month_keys: Vec<(i32, u32)> = month_counts.keys().copied().collect();
    month_keys.sort();
    for (y, m) in month_keys {
        let value = *month_counts.get(&(y, m)).unwrap_or(&0);
        by_month.push(json!({
            "label": month_short_label(y, m),
            "value": value,
            "isCurrent": y == year && m == month,
        }));
    }

    PhoneStats {
        total,
        this_month,
        this_week,
        by_day,
        by_week,
        by_month,
        months,
        weeks,
    }
}

fn stage_sort_key(stage: &str) -> i32 {
    let t = stage.trim().to_lowercase();
    if t.contains("phone") {
        return 0;
    }
    if t.contains("recruiter") || t.contains("screening") {
        return 1;
    }
    if t.contains("initial") {
        return 2;
    }
    if t.contains("2nd") || t.contains("second") {
        return 3;
    }
    if t.contains("3rd") || t.contains("third") {
        return 4;
    }
    if t.contains("4th") || t.contains("fourth") {
        return 5;
    }
    if t.contains("final") {
        return 6;
    }
    if t.contains("video") {
        return 2;
    }
    50
}

fn stage_depth(stage: &str) -> i32 {
    let t = stage.trim().to_lowercase();
    if t.contains("final") {
        return 6;
    }
    if t.contains("4th") || t.contains("fourth") {
        return 5;
    }
    if t.contains("3rd") || t.contains("third") {
        return 4;
    }
    if t.contains("2nd") || t.contains("second") {
        return 3;
    }
    if t.contains("initial") || t.contains("video") {
        return 2;
    }
    if t.contains("recruiter") || t.contains("screening") || t.contains("phone") {
        return 1;
    }
    if t.is_empty() {
        return 0;
    }
    2
}

/// One row per company — deepest / latest round only, with resume + JD when available.

/// One row per company, merging booked interviews with the interviews logged on
/// the Data Interview Sheet before the booking tool existed. Both sources are
/// the same event type, so they are counted together rather than shown apart.
struct CompanyAgg {
    display: String,
    booked: i32,
    logged: i32,
    phone_calls: i32,
    /// Deepest labelled interview stage; 0 when no source supplied a stage.
    depth: i32,
    stage_label: String,
    last_key: String,
    last_date: String,
    last_time: String,
    status: String,
    resume: String,
    jd: String,
}

impl CompanyAgg {
    fn interviews(&self) -> i32 {
        self.booked + self.logged
    }

    fn type_label(&self) -> String {
        let has_interview = self.interviews() > 0;
        let has_phone = self.phone_calls > 0;
        match (has_interview, has_phone) {
            (true, true) => "Interview + Phone call".to_string(),
            (false, true) => "Phone call".to_string(),
            _ => "Interview".to_string(),
        }
    }

    /// Labelled interview stage when one exists, else Phone call when that is
    /// the only event, else the round implied by interview count.
    fn round_label(&self) -> String {
        if !self.stage_label.trim().is_empty() {
            return self.stage_label.trim().to_string();
        }
        if self.phone_calls > 0 && self.interviews() == 0 {
            return "Phone call".to_string();
        }
        match self.interviews() {
            0 => "—".to_string(),
            n => format!("Round {n}"),
        }
    }

    fn round_depth(&self) -> i32 {
        if self.depth > 0 {
            self.depth
        } else {
            self.interviews()
        }
    }
}

fn company_rows(mine: &[BookingRecord], past: &[HistoryRecord]) -> Vec<CompanyAgg> {
    let mut by_company: BTreeMap<String, CompanyAgg> = BTreeMap::new();

    let entry = |map: &mut BTreeMap<String, CompanyAgg>, name: &str| -> Option<String> {
        let company = name.trim();
        if company.is_empty() {
            return None;
        }
        let key = company.to_lowercase();
        map.entry(key.clone()).or_insert_with(|| CompanyAgg {
            display: company.to_string(),
            booked: 0,
            logged: 0,
            phone_calls: 0,
            depth: 0,
            stage_label: String::new(),
            last_key: String::new(),
            last_date: String::new(),
            last_time: String::new(),
            status: String::new(),
            resume: String::new(),
            jd: String::new(),
        });
        Some(key)
    };

    for r in mine {
        let Some(key) = entry(&mut by_company, &r.client) else { continue };
        let agg = by_company.get_mut(&key).expect("just inserted");
        if r.kind == SheetKind::Phone {
            agg.phone_calls += 1;
            if agg.stage_label.is_empty() {
                agg.stage_label = "Phone call".to_string();
                agg.depth = 1;
            }
        } else {
            agg.booked += 1;
            let stage = r.interview_stage.trim();
            let depth = stage_depth(stage);
            if !stage.is_empty()
                && (depth > agg.depth || agg.stage_label.eq_ignore_ascii_case("phone call"))
            {
                agg.depth = depth;
                agg.stage_label = stage.to_string();
            }
        }
        if agg.resume.is_empty() {
            agg.resume = r.resume_link.clone();
        }
        if agg.jd.is_empty() {
            agg.jd = r.jd_link.clone();
        }
        if r.date_key > agg.last_key {
            agg.last_key = r.date_key.clone();
            agg.last_date = r.meeting_date.clone();
            agg.last_time = r.meeting_time.clone();
            agg.status = r.status.clone();
        }
    }

    for r in past {
        let Some(key) = entry(&mut by_company, &r.client) else { continue };
        let agg = by_company.get_mut(&key).expect("just inserted");
        agg.logged += 1;
        // History rows carry no stage, status or links — only date and count.
        if r.date_key > agg.last_key {
            agg.last_key = r.date_key.clone();
            agg.last_date = r.meeting_date.clone();
            agg.last_time = r.meeting_time.clone();
        }
    }

    let mut rows: Vec<CompanyAgg> = by_company.into_values().collect();
    rows.sort_by(|a, b| {
        b.last_key
            .cmp(&a.last_key)
            .then_with(|| b.round_depth().cmp(&a.round_depth()))
            .then_with(|| a.display.to_lowercase().cmp(&b.display.to_lowercase()))
    });
    rows
}

fn empty_tracker_body(
    profile_email: &str,
    profile_name: &str,
    admin: bool,
    candidates: &[Person],
    message: &str,
) -> Value {
    json!({
        "ok": true,
        "admin": admin,
        "matched": false,
        "mocked": false,
        "message": message,
        "profile": { "email": profile_email, "name": profile_name },
        "candidates": candidates.iter().map(candidate_json).collect::<Vec<_>>(),
        "applies": {
            "monthTab": month_tab_name(),
            "total": 0,
            "thisWeek": 0,
            "byDay": [],
            "byWeek": [],
            "matched": false,
            "lifetime": 0,
            "today": 0,
            "byMonth": [],
            "months": [],
            "weeks": []
        },
        "phoneCalls": {
            "total": 0,
            "thisMonth": 0,
            "thisWeek": 0,
            "byDay": [],
            "byWeek": [],
            "byMonth": [],
            "months": [],
            "weeks": [],
            "recent": [],
            "retentionDays": 7
        },
        "outcomes": {
            "pending": 0,
            "rejected": 0,
            "offered": 0
        },
        "interviews": {
            "total": 0,
            "byRound": [],
            "analytics": {
                "companies": 0,
                "avgRoundsReached": 0,
                "maxRoundDepth": 0,
                "deepestRound": "None",
                "interviewCount": 0,
                "byStatus": [],
                "roundShare": []
            },
            "items": []
        }
    })
}

#[derive(Deserialize, Default)]
pub struct TrackerQuery {
    #[serde(default)]
    email: String,
}

/// Personal Application Tracker for the signed-in Google account.
/// Admins (@cubicit.net / AUTH_EXTRA_EMAILS) may pass ?email= to view any candidate.
pub async fn application_tracker(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<TrackerQuery>,
) -> Result<impl IntoResponse, AppError> {
    let user = require_session(&state.cfg, &headers)?;
    let admin = is_tracker_admin(&state.cfg, &user.email);

    let requested = normalize_email(&query.email);
    let lookup = if admin {
        // Admins must pick a candidate; no auto-mock open.
        requested
    } else {
        normalize_email(&user.email)
    };

    if admin && lookup.is_empty() {
        let candidates = cached_candidates(&state).await.map_err(AppError::from)?;
        return Ok(Json(empty_tracker_body(
            &user.email,
            &user.name,
            true,
            &candidates,
            "Select a candidate to view their Application Tracker.",
        )));
    }

    // Fan out every read that doesn't depend on the resolved person so the
    // tracker isn't paying for them one Sheets round-trip at a time.
    let (candidates_res, person_res, interview_res, phone_res) = tokio::join!(
        async {
            if admin {
                cached_candidates(&state).await
            } else {
                Ok(std::sync::Arc::new(Vec::new()))
            }
        },
        resolve_person(&state, &lookup),
        read_interview_records(&state),
        read_phone_call_records(&state),
    );
    let candidates = candidates_res.map_err(AppError::from)?;

    let viewing_other = admin && normalize_email(&lookup) != normalize_email(&user.email);
    let Some(person) = person_res.map_err(AppError::from)? else {
        return Ok(Json(empty_tracker_body(
            &lookup,
            if admin { "" } else { user.name.as_str() },
            admin,
            &candidates,
            "No candidate profile is linked to this email yet (Current_Market or month tracker tab).",
        )));
    };

    let applies = load_apply_stats(&state, &person)
        .await
        .map_err(AppError::from)?;

    let mut records = Vec::new();
    match interview_res {
        Ok(mut rows) => records.append(&mut rows),
        Err(err) => tracing::warn!(error = %err, "application tracker interview read failed"),
    }
    match phone_res {
        Ok(mut rows) => records.append(&mut rows),
        Err(err) => tracing::warn!(error = %err, "application tracker phone read failed"),
    }

    let mut mine: Vec<BookingRecord> = records
        .into_iter()
        .filter(|r| record_belongs(r, &person))
        .collect();
    mine.sort_by(|a, b| {
        b.date_key
            .cmp(&a.date_key)
            .then_with(|| b.meeting_time.cmp(&a.meeting_time))
            .then_with(|| a.client.to_lowercase().cmp(&b.client.to_lowercase()))
    });

    let mut round_map = BTreeMap::<String, i32>::new();
    for r in &mine {
        let stage = if r.interview_stage.trim().is_empty() {
            "Other".to_string()
        } else {
            r.interview_stage.trim().to_string()
        };
        *round_map.entry(stage).or_default() += 1;
    }
    let mut by_round: Vec<Value> = round_map
        .into_iter()
        .map(|(stage, count)| json!({ "stage": stage, "count": count }))
        .collect();
    by_round.sort_by(|a, b| {
        let sa = a.get("stage").and_then(|v| v.as_str()).unwrap_or("");
        let sb = b.get("stage").and_then(|v| v.as_str()).unwrap_or("");
        stage_sort_key(sa)
            .cmp(&stage_sort_key(sb))
            .then_with(|| sa.cmp(sb))
    });

    // Past interviews logged on the Data Interview Sheet before the booking tool
    // existed. Matched by candidate name only — those tabs carry no email column.
    let person_name = normalize_name(&person.name);
    let booked_keys: std::collections::HashSet<String> = mine
        .iter()
        .map(|r| format!("{}|{}", r.client.trim().to_lowercase(), r.date_key))
        .collect();
    let mut past: Vec<HistoryRecord> = if person_name.is_empty() {
        Vec::new()
    } else {
        history_records(&state)
            .await
            .iter()
            .filter(|r| normalize_name(&r.candidate_name) == person_name)
            .filter(|r| {
                !booked_keys.contains(&format!(
                    "{}|{}",
                    r.client.trim().to_lowercase(),
                    r.date_key
                ))
            })
            .cloned()
            .collect()
    };
    past.sort_by(|a, b| {
        b.date_key
            .cmp(&a.date_key)
            .then_with(|| b.meeting_time.cmp(&a.meeting_time))
            .then_with(|| a.client.to_lowercase().cmp(&b.client.to_lowercase()))
    });
    // One list: every company, with the round reached across both sources.
    let companies = company_rows(&mine, &past);
    let company_items: Vec<Value> = companies
        .iter()
        .map(|c| {
            json!({
                "company": c.display,
                "type": c.type_label(),
                "roundLabel": c.round_label(),
                "roundDepth": c.round_depth(),
                "interviews": c.interviews(),
                "phoneCalls": c.phone_calls,
                "booked": c.booked,
                "logged": c.logged,
                "lastDate": c.last_date,
                "lastDateKey": c.last_key,
                "lastTime": c.last_time,
                "status": c.status,
                "resumeUrl": c.resume,
                "jdUrl": c.jd,
            })
        })
        .collect();

    let total_interviews: i32 = companies.iter().map(|c| c.interviews()).sum();
    let total_phone: i32 = companies.iter().map(|c| c.phone_calls).sum();
    let deepest = companies.iter().map(|c| c.round_depth()).max().unwrap_or(0);
    let deepest_label = companies
        .iter()
        .max_by_key(|c| c.round_depth())
        .map(|c| c.round_label())
        .unwrap_or_else(|| "None".to_string());
    let avg_rounds = if companies.is_empty() {
        0.0
    } else {
        let sum: i32 = companies.iter().map(|c| c.round_depth()).sum();
        ((sum as f64) / (companies.len() as f64) * 10.0).round() / 10.0
    };
    let (pending, rejected, offered) = outcome_counts(&companies);
    let phones = phone_stats(&mine);
    let mut recent_rows: Vec<_> = mine.iter().filter(|r| r.kind == SheetKind::Phone).collect();
    recent_rows.sort_by(|a, b| {
        b.date_key
            .cmp(&a.date_key)
            .then(b.meeting_time.cmp(&a.meeting_time))
    });
    let recent_phones: Vec<Value> = recent_rows
        .iter()
        .map(|r| {
            json!({
                "company": r.client,
                "date": r.meeting_date,
                "dateKey": r.date_key,
                "time": r.meeting_time,
                "status": r.status,
            })
        })
        .collect();

    Ok(Json(json!({
        "ok": true,
        "admin": admin,
        "profile": {
            "name": person.name,
            "email": person.email,
        },
        "matched": true,
        "mocked": viewing_other,
        "candidates": candidates.iter().map(candidate_json).collect::<Vec<_>>(),
        "generatedAt": chrono::Utc::now().to_rfc3339(),
        "today": today_date_key(),
        "applies": {
            "monthTab": applies.month_tab,
            "total": applies.total,
            "thisWeek": applies.this_week,
            "byDay": applies.by_day,
            "byWeek": applies.by_week,
            "matched": applies.matched,
            "lifetime": applies.lifetime,
            "today": applies.today,
            "byMonth": applies.by_month,
            "months": applies.months,
            "weeks": applies.weeks,
        },
        "phoneCalls": {
            "total": phones.total,
            "thisMonth": phones.this_month,
            "thisWeek": phones.this_week,
            "byDay": phones.by_day,
            "byWeek": phones.by_week,
            "byMonth": phones.by_month,
            "months": phones.months,
            "weeks": phones.weeks,
            "recent": recent_phones,
            "retentionDays": state.cfg.phone_calls_retention_days,
        },
        "outcomes": {
            "pending": pending,
            "rejected": rejected,
            "offered": offered,
        },
        "interviews": {
            "total": total_interviews,
            "phoneCalls": total_phone,
            "companyCount": companies.len(),
            "booked": mine.iter().filter(|r| r.kind != SheetKind::Phone).count(),
            "logged": past.len(),
            "historyMonths": HISTORY_MONTHS,
            "avgRoundsReached": avg_rounds,
            "maxRoundDepth": deepest,
            "deepestRound": deepest_label,
        },
        "companies": company_items
    })))
}

#[derive(Deserialize)]
pub struct LogAppliesBody {
    #[serde(default)]
    email: String,
    count: i32,
}

/// Write today's apply count onto the current month tab of the tracking sheet.
pub async fn log_today_applies(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<LogAppliesBody>,
) -> Result<impl IntoResponse, AppError> {
    if body.count < 0 || body.count > 999 {
        return Err(AppError::BadRequest(
            "Enter a whole number of applies between 0 and 999.".into(),
        ));
    }
    let user = require_session(&state.cfg, &headers)?;
    let admin = is_tracker_admin(&state.cfg, &user.email);
    let lookup = if admin {
        normalize_email(&body.email)
    } else {
        normalize_email(&user.email)
    };
    if admin && lookup.is_empty() {
        return Err(AppError::BadRequest(
            "Select a candidate before logging applies.".into(),
        ));
    }
    let Some(person) = resolve_person(&state, &lookup)
        .await
        .map_err(AppError::from)?
    else {
        return Err(AppError::BadRequest(
            "No candidate profile is linked to this email yet.".into(),
        ));
    };

    let now = chrono::Utc::now().with_timezone(&Chicago);
    let today = now.date_naive();
    let month_tab = month_tab_name();
    let rows = state
        .google
        .sheets_values_get(
            &state.cfg.hiring_spreadsheet_id,
            &format!("'{month_tab}'!A1:AZ200"),
            Some("FORMATTED_VALUE"),
        )
        .await
        .map_err(AppError::from)?;
    if rows.len() < 2 {
        return Err(AppError::Upstream(format!(
            "Month tab {month_tab} is missing or empty."
        )));
    }
    let header_day = &rows[1];
    let day = today.day();
    let col = (2..header_day.len().max(40))
        .find(|&c| {
            header_day
                .get(c)
                .map(|s| s.trim().parse::<u32>().unwrap_or(0) == day)
                .unwrap_or(false)
        })
        .ok_or_else(|| {
            AppError::Upstream(format!(
                "Could not find today's column ({day}) on {month_tab}."
            ))
        })?;
    let col_letter = a1_col(col);

    const DATA_START_ROW: usize = 5;
    let sheet_row = if let Some(idx) = find_apply_row_index(&rows, &person) {
        idx + 1
    } else {
        let mut last = DATA_START_ROW - 1;
        for (i, row) in rows.iter().enumerate().skip(DATA_START_ROW - 1) {
            if !row.first().map(|s| s.trim()).unwrap_or("").is_empty() {
                last = i + 1;
            }
        }
        last + 1
    };

    let range = format!("'{month_tab}'!{col_letter}{sheet_row}");
    state
        .google
        .sheets_values_update(
            &state.cfg.hiring_spreadsheet_id,
            &range,
            vec![vec![json!(body.count)]],
            "USER_ENTERED",
        )
        .await
        .map_err(AppError::from)?;

    if find_apply_row_index(&rows, &person).is_none() {
        let name = if person.name.trim().is_empty() {
            person.email.clone()
        } else {
            person.name.clone()
        };
        let headers = apply_headers(&rows);
        let name_i = headers
            .idx(crate::headers::col::APPLY_NAME)
            .ok_or_else(|| AppError::BadRequest("month tab has no Candidate Name header".into()))?;
        let mail_i = headers
            .idx(crate::headers::col::APPLY_MAIL)
            .ok_or_else(|| AppError::BadRequest("month tab has no Mail header".into()))?;
        let (min, max, values) = crate::headers::sparse_row(&[
            (name_i, json!(name)),
            (mail_i, json!(person.email)),
        ]);
        state
            .google
            .sheets_values_update(
                &state.cfg.hiring_spreadsheet_id,
                &headers.range_row(&month_tab, sheet_row as i32, min, max),
                vec![values],
                "USER_ENTERED",
            )
            .await
            .map_err(AppError::from)?;
    }

    state.history.invalidate_apply_tabs().await;

    Ok(Json(json!({
        "ok": true,
        "count": body.count,
        "date": today.to_string(),
        "monthTab": month_tab,
        "row": sheet_row,
    })))
}
