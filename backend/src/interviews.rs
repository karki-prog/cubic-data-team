//! Cubic Interviews: the "Client List - Interview recieved" tab on the Data
//! application tracking sheet, mirrored into Mongo (or `data/` without Mongo).
//!
//! Same lifecycle as hiring postings: the UI only reads the stored copy, and the
//! copy is replaced by the daily cron, the manual sync route, the webhook, or the
//! sheet watcher. An empty pull never wipes a non-empty copy.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{header, HeaderMap};
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::Mutex;

use crate::auth::require_session;
use crate::error::AppError;
use crate::google::sheets::url_from_grid_cell;
use crate::headers::SheetHeaders;
use crate::jobs::assert_job_auth;
use crate::AppState;

const STORE_KEY: &str = "cubic-interviews";
const STORE_NAME: &str = "cubic-interviews.json";
/// Used when no tab title starts with "client list" (spelling on the sheet is "recieved").
const DEFAULT_TAB: &str = "Client List - Interview recieved";
const MAX_ROWS: i32 = 2000;

mod col {
    pub const COMPANY: &[&str] = &["company"];
    pub const TOTAL: &[&str] = &["total"];
    pub const DATA: &[&str] = &["data"];
    pub const JAVA: &[&str] = &["java"];
    pub const DATES: &[&str] = &["interview dates", "interview date"];
    pub const LINK: &[&str] = &["job portal link", "job portal", "portal link", "url"];
    pub const COMMENT: &[&str] = &["comment", "comments", "note"];
}

/// Visa columns in sheet order. Matched exactly so `OPT` never grabs `OPT EAD`.
const VISA_HEADERS: &[&str] = &[
    "cpt", "opt ead", "opt", "stem", "h1b", "tps / asylum", "gc ead", "gc", "usc",
];

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct VisaCount {
    pub label: String,
    pub count: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InterviewClient {
    pub company: String,
    pub total: u32,
    pub data: u32,
    pub java: u32,
    #[serde(default)]
    pub visas: Vec<VisaCount>,
    #[serde(default)]
    pub interview_dates: Vec<String>,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub link_label: String,
    #[serde(default)]
    pub comment: String,
}

#[derive(Clone)]
pub struct InterviewsCache {
    snapshot: Arc<Mutex<Option<Value>>>,
}

impl InterviewsCache {
    pub fn new() -> Self {
        Self {
            snapshot: Arc::new(Mutex::new(None)),
        }
    }
}

#[derive(Deserialize, Default)]
pub struct InterviewsPushBody {
    #[serde(default)]
    source: String,
}

fn store_path(state: &AppState) -> PathBuf {
    state.cfg.data_dir.join(STORE_NAME)
}

fn count(text: &str) -> u32 {
    text.trim().replace(',', "").parse::<f64>().map(|n| n.max(0.0) as u32).unwrap_or(0)
}

fn exact_idx(headers: &SheetHeaders, name: &str) -> Option<usize> {
    headers.names.iter().position(|h| h == name)
}

/// "Company (last 15 days before and 5 days ahead interview)" → the bracketed note.
fn window_note(header: &str) -> String {
    let Some(open) = header.find('(') else {
        return String::new();
    };
    let inner = header[open + 1..].trim_end().trim_end_matches(')').trim();
    let mut chars = inner.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

async fn resolve_tab(state: &AppState) -> String {
    let titles = state
        .google
        .sheet_titles(&state.cfg.hiring_spreadsheet_id)
        .await
        .unwrap_or_default();
    titles
        .into_iter()
        .find(|t| t.trim().to_lowercase().starts_with("client list"))
        .unwrap_or_else(|| DEFAULT_TAB.to_string())
}

struct Pulled {
    tab: String,
    window_note: String,
    clients: Vec<InterviewClient>,
}

async fn pull_from_sheet(state: &AppState) -> anyhow::Result<Pulled> {
    let tab = resolve_tab(state).await;
    let grid = state
        .google
        .grid_data(
            &state.cfg.hiring_spreadsheet_id,
            &format!("'{tab}'!A1:Z{MAX_ROWS}"),
            "sheets(data(rowData(values(formattedValue,hyperlink,userEnteredValue,textFormatRuns))))",
        )
        .await?;
    let cells: Vec<Vec<String>> = grid
        .iter()
        .map(|row| {
            row.values
                .as_ref()
                .map(|vals| {
                    vals.iter()
                        .map(|c| c.formatted_value.clone().unwrap_or_default())
                        .collect()
                })
                .unwrap_or_default()
        })
        .collect();

    let headers = SheetHeaders::detect(&cells);
    let company_idx = headers.must(col::COMPANY)?;
    let total_idx = headers.idx(col::TOTAL);
    let data_idx = headers.idx(col::DATA);
    let java_idx = headers.idx(col::JAVA);
    let dates_idx = headers.idx(col::DATES);
    let link_idx = headers.idx(col::LINK);
    let comment_idx = headers.idx(col::COMMENT);
    let header_row = (headers.row_1 as usize).saturating_sub(1);
    let raw_header = cells.get(header_row).cloned().unwrap_or_default();
    let visa_cols: Vec<(usize, String)> = VISA_HEADERS
        .iter()
        .filter_map(|name| {
            let i = exact_idx(&headers, name)?;
            let label = raw_header.get(i).map(|s| s.trim().to_string()).unwrap_or_default();
            Some((i, label))
        })
        .collect();
    let note = window_note(raw_header.get(company_idx).map(String::as_str).unwrap_or(""));

    let get = |row: &[String], idx: Option<usize>| -> String {
        idx.and_then(|i| row.get(i)).map(|s| s.trim().to_string()).unwrap_or_default()
    };

    let mut clients = Vec::new();
    for (i, row) in cells.iter().enumerate().skip(header_row + 1) {
        let company = get(row, Some(company_idx));
        if company.is_empty() {
            continue;
        }
        let link_cell = link_idx.and_then(|li| {
            grid.get(i)
                .and_then(|r| r.values.as_ref())
                .and_then(|v| v.get(li))
        });
        let url = url_from_grid_cell(link_cell);
        let mut link_label = get(row, link_idx);
        if link_label.starts_with("http://") || link_label.starts_with("https://") {
            link_label.clear();
        }
        let mut url = url;
        if url.is_empty() {
            let raw = get(row, link_idx);
            if raw.starts_with("http://") || raw.starts_with("https://") {
                url = raw;
            }
        }
        let visas = visa_cols
            .iter()
            .map(|(vi, label)| VisaCount {
                label: label.clone(),
                count: count(&get(row, Some(*vi))),
            })
            .filter(|v| v.count > 0)
            .collect();
        let interview_dates = get(row, dates_idx)
            .split(',')
            .map(|d| d.trim().to_string())
            .filter(|d| !d.is_empty())
            .collect();
        clients.push(InterviewClient {
            company,
            total: count(&get(row, total_idx)),
            data: count(&get(row, data_idx)),
            java: count(&get(row, java_idx)),
            visas,
            interview_dates,
            url,
            link_label,
            comment: get(row, comment_idx),
        });
    }
    clients.sort_by(|a, b| {
        b.total
            .cmp(&a.total)
            .then_with(|| a.company.to_lowercase().cmp(&b.company.to_lowercase()))
    });
    Ok(Pulled {
        tab,
        window_note: note,
        clients,
    })
}

fn signature(clients: &[InterviewClient]) -> String {
    let mut h = DefaultHasher::new();
    serde_json::to_string(clients).unwrap_or_default().hash(&mut h);
    format!("{:016x}", h.finish())
}

fn store_body(pulled: Pulled, source: &str) -> Value {
    let total: u32 = pulled.clients.iter().map(|c| c.total).sum();
    let data: u32 = pulled.clients.iter().map(|c| c.data).sum();
    let java: u32 = pulled.clients.iter().map(|c| c.java).sum();
    json!({
        "ok": true,
        "source": source,
        "generatedAt": chrono::Utc::now().to_rfc3339(),
        "tab": pulled.tab,
        "windowNote": pulled.window_note,
        "count": pulled.clients.len(),
        "totals": { "interviews": total, "data": data, "java": java },
        "signature": signature(&pulled.clients),
        "clients": pulled.clients,
    })
}

async fn write_store(state: &AppState, body: Value) -> anyhow::Result<Value> {
    if state.db.is_connected() {
        state.db.put_json(STORE_KEY, &body).await?;
    } else {
        let path = store_path(state);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&path, serde_json::to_vec_pretty(&body)?).await?;
    }
    *state.interviews.snapshot.lock().await = Some(body.clone());
    Ok(body)
}

fn valid(body: &Value) -> bool {
    body.get("ok").and_then(|v| v.as_bool()) == Some(true)
        && body.get("clients").map(|c| c.is_array()).unwrap_or(false)
}

async fn cached_body(state: &AppState) -> Option<Value> {
    if let Some(body) = state.db.get_json(STORE_KEY).await.filter(valid) {
        *state.interviews.snapshot.lock().await = Some(body.clone());
        return Some(body);
    }
    if let Some(body) = state.interviews.snapshot.lock().await.clone() {
        return Some(body);
    }
    let raw = std::fs::read(store_path(state)).ok()?;
    let body: Value = serde_json::from_slice(&raw).ok().filter(valid)?;
    *state.interviews.snapshot.lock().await = Some(body.clone());
    Some(body)
}

pub(crate) async fn pull_and_store(state: &AppState, source: &str) -> anyhow::Result<Value> {
    let pulled = pull_from_sheet(state).await?;
    if pulled.clients.is_empty() {
        if let Some(existing) = cached_body(state).await {
            let current = existing.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
            if current > 0 {
                tracing::warn!(
                    source,
                    "refusing empty Client List pull; keeping {current} cached client(s)"
                );
                return Ok(existing);
            }
        }
    }
    write_store(state, store_body(pulled, source)).await
}

fn summary(body: &Value) -> Value {
    json!({
        "ok": true,
        "updated": true,
        "count": body.get("count"),
        "generatedAt": body.get("generatedAt"),
        "source": body.get("source"),
    })
}

/// UI reads the stored copy; only a sync/cron/webhook/watcher replaces it.
pub async fn cubic_interviews(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, AppError> {
    require_session(&state.cfg, &headers)?;
    let body = match cached_body(&state).await {
        Some(body) => body,
        None => pull_and_store(&state, "sheet-bootstrap")
            .await
            .map_err(AppError::from)?,
    };
    let mut out = HeaderMap::new();
    out.insert(
        header::CACHE_CONTROL,
        "private, max-age=0, must-revalidate".parse().unwrap(),
    );
    Ok((out, Json(body)))
}

/// Sheet changed → ping here. Always re-reads the tab.
pub async fn webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, AppError> {
    assert_job_auth(&state, &headers).await?;
    let parsed: InterviewsPushBody = serde_json::from_slice(&body).unwrap_or_default();
    let source = match parsed.source.trim() {
        "" => "webhook",
        s => s,
    };
    let stored = pull_and_store(&state, source).await.map_err(AppError::from)?;
    Ok(Json(summary(&stored)))
}

/// Manual / cron: pull the Client List tab once and replace the stored copy.
pub async fn sync(State(state): State<AppState>, headers: HeaderMap) -> Result<impl IntoResponse, AppError> {
    assert_job_auth(&state, &headers).await?;
    let stored = pull_and_store(&state, "sheet-sync").await.map_err(AppError::from)?;
    Ok(Json(summary(&stored)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_note_from_company_header() {
        assert_eq!(
            window_note("Company (last 15 days before and 5 days ahead interview) "),
            "Last 15 days before and 5 days ahead interview"
        );
        assert_eq!(window_note("Company"), "");
    }

    #[test]
    fn visa_headers_match_exactly() {
        let headers = SheetHeaders::parse(&[
            "Company".into(),
            "OPT EAD ".into(),
            "OPT ".into(),
            "GC EAD".into(),
            "GC".into(),
        ]);
        assert_eq!(exact_idx(&headers, "opt"), Some(2));
        assert_eq!(exact_idx(&headers, "opt ead"), Some(1));
        assert_eq!(exact_idx(&headers, "gc"), Some(4));
    }

    #[test]
    fn counts_tolerate_blanks() {
        assert_eq!(count(" 5 "), 5);
        assert_eq!(count(""), 0);
        assert_eq!(count("n/a"), 0);
    }
}
