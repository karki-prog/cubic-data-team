use std::path::PathBuf;
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

use crate::auth::require_session;
use crate::error::AppError;
use crate::jobs::assert_job_auth;
use crate::AppState;

const STORE_NAME: &str = "do-not-apply.json";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DnaCompany {
    pub name: String,
    #[serde(default)]
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct DnaStore {
    ok: bool,
    #[serde(default)]
    source: String,
    #[serde(rename = "generatedAt")]
    generated_at: String,
    count: usize,
    companies: Vec<DnaCompany>,
}

#[derive(Clone)]
struct DnaSnapshot {
    file_mtime: Option<SystemTime>,
    body: Value,
}

#[derive(Clone)]
pub struct DnaCache {
    snapshot: Arc<Mutex<Option<DnaSnapshot>>>,
}

impl DnaCache {
    pub fn new() -> Self {
        Self {
            snapshot: Arc::new(Mutex::new(None)),
        }
    }
}

#[derive(Deserialize, Default)]
pub struct DnaPushBody {
    companies: Option<Vec<DnaCompanyIn>>,
    #[serde(default)]
    source: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum DnaCompanyIn {
    Obj {
        name: String,
        #[serde(default)]
        reason: String,
    },
    Pair(Vec<String>),
}

impl DnaCompanyIn {
    fn into_company(self) -> Option<DnaCompany> {
        match self {
            DnaCompanyIn::Obj { name, reason } => Some(DnaCompany { name, reason }),
            DnaCompanyIn::Pair(parts) => {
                let name = parts.first().cloned().unwrap_or_default();
                let reason = parts.get(1).cloned().unwrap_or_default();
                Some(DnaCompany { name, reason })
            }
        }
    }
}

fn store_path(state: &AppState) -> PathBuf {
    state.cfg.data_dir.join(STORE_NAME)
}

fn normalize_companies(rows: impl IntoIterator<Item = DnaCompany>) -> Vec<DnaCompany> {
    let mut by_name = std::collections::BTreeMap::<String, DnaCompany>::new();
    for mut row in rows {
        row.name = row.name.trim().to_string();
        row.reason = row.reason.trim().to_string();
        if row.name.is_empty() || row.name.eq_ignore_ascii_case("company") {
            continue;
        }
        let key = row.name.to_lowercase();
        match by_name.get_mut(&key) {
            Some(existing) => {
                if existing.reason.is_empty() && !row.reason.is_empty() {
                    existing.reason = row.reason;
                }
            }
            None => {
                by_name.insert(key, row);
            }
        }
    }
    let mut out: Vec<DnaCompany> = by_name.into_values().collect();
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

fn store_from_companies(companies: Vec<DnaCompany>, source: &str) -> DnaStore {
    let companies = normalize_companies(companies);
    DnaStore {
        ok: true,
        source: source.to_string(),
        generated_at: chrono::Utc::now().to_rfc3339(),
        count: companies.len(),
        companies,
    }
}

fn store_to_json(store: &DnaStore) -> Value {
    json!({
        "ok": store.ok,
        "source": store.source,
        "generatedAt": store.generated_at,
        "count": store.count,
        "companies": store.companies,
    })
}

fn file_mtime(path: &PathBuf) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

async fn write_store(state: &AppState, store: &DnaStore) -> anyhow::Result<Value> {
    let body = store_to_json(store);
    if state.db.is_connected() {
        state.db.put_json("do-not-apply", &body).await?;
    } else {
        let path = store_path(state);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&path, serde_json::to_vec_pretty(&body)?).await?;
    }
    *state.dna.snapshot.lock().await = Some(DnaSnapshot {
        file_mtime: None,
        body: body.clone(),
    });
    Ok(body)
}

fn read_store_file(path: &PathBuf) -> Option<(SystemTime, Value)> {
    let raw = std::fs::read(path).ok()?;
    let mut store: DnaStore = serde_json::from_slice(&raw).ok()?;
    if store.companies.is_empty() && store.count == 0 && !store.ok {
        return None;
    }
    store.companies = normalize_companies(store.companies);
    store.count = store.companies.len();
    Some((file_mtime(path).unwrap_or(SystemTime::UNIX_EPOCH), store_to_json(&store)))
}

fn companies_from_sheet_rows(headers: &crate::headers::SheetHeaders, rows: Vec<Vec<String>>) -> Vec<DnaCompany> {
    let header_idx = (headers.row_1 as usize).saturating_sub(1);
    normalize_companies(rows.into_iter().enumerate().filter_map(|(i, row)| {
        if i == header_idx {
            return None;
        }
        let name = headers.get_owned(&row, crate::headers::col::COMPANY);
        if name.is_empty() {
            return None;
        }
        let reason = headers.get_owned(&row, crate::headers::col::REASON);
        Some(DnaCompany { name, reason })
    }))
}

async fn pull_from_sheet(state: &AppState) -> anyhow::Result<Vec<DnaCompany>> {
    let src_tab = &state.cfg.do_not_apply_source_tab;
    let src_range = format!("'{src_tab}'!A1:Z2000");
    let src_rows = state
        .google
        .sheets_values_get(
            &state.cfg.appointment_spreadsheet_id,
            &src_range,
            Some("FORMATTED_VALUE"),
        )
        .await
        .unwrap_or_default();
    if !src_rows.is_empty() {
        let headers = crate::headers::SheetHeaders::detect(&src_rows);
        let companies = companies_from_sheet_rows(&headers, src_rows);
        if !companies.is_empty() {
            return Ok(companies);
        }
    }
    let display_tab = &state.cfg.do_not_apply_tab;
    let display_range = format!("'{display_tab}'!A1:Z2000");
    let display_rows = state
        .google
        .sheets_values_get(
            &state.cfg.hiring_spreadsheet_id,
            &display_range,
            Some("FORMATTED_VALUE"),
        )
        .await?;
    let headers = crate::headers::SheetHeaders::detect(&display_rows);
    Ok(companies_from_sheet_rows(&headers, display_rows))
}

pub(crate) async fn pull_and_store(state: &AppState, source: &str) -> anyhow::Result<Value> {
    let companies = pull_from_sheet(state).await?;
    if companies.is_empty() {
        if let Some(existing) = cached_body(state).await {
            let current = existing
                .get("count")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            if current > 0 {
                tracing::warn!(
                    source,
                    "refusing empty Do Not Apply pull; keeping {current} cached company(ies)"
                );
                return Ok(existing);
            }
        }
    }
    let store = store_from_companies(companies, source);
    write_store(state, &store).await
}

async fn cached_body(state: &AppState) -> Option<Value> {
    if let Some(body) = state.db.get_json("do-not-apply").await {
        *state.dna.snapshot.lock().await = Some(DnaSnapshot {
            file_mtime: None,
            body: body.clone(),
        });
        return Some(body);
    }
    {
        let snap = state.dna.snapshot.lock().await;
        if let Some(current) = snap.as_ref() {
            return Some(current.body.clone());
        }
    }
    let path = store_path(state);
    if let Some((_mtime, body)) = read_store_file(&path) {
        *state.dna.snapshot.lock().await = Some(DnaSnapshot {
            file_mtime: None,
            body: body.clone(),
        });
        return Some(body);
    }
    None
}

async fn response_json(body: Value) -> impl IntoResponse {
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
pub async fn do_not_apply(
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
    Ok(response_json(body).await)
}

/// Sheet changed → ping here (empty body) or POST companies. Empty body re-reads the sheet.
pub async fn webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, AppError> {
    assert_job_auth(&state, &headers).await?;
    let parsed: DnaPushBody = if body.is_empty() {
        DnaPushBody::default()
    } else {
        serde_json::from_slice(&body).unwrap_or_default()
    };
    let source = if parsed.source.trim().is_empty() {
        "webhook"
    } else {
        parsed.source.trim()
    };
    if let Some(incoming) = parsed.companies.filter(|c| !c.is_empty()) {
        let companies: Vec<DnaCompany> = incoming.into_iter().filter_map(|c| c.into_company()).collect();
        let store = store_from_companies(companies, source);
        let _body = write_store(&state, &store).await.map_err(AppError::from)?;
        return Ok(Json(json!({
            "ok": true,
            "updated": true,
            "count": store.count,
            "generatedAt": store.generated_at,
            "source": store.source,
        })));
    }
    let body = pull_and_store(&state, source).await.map_err(AppError::from)?;
    Ok(Json(json!({
        "ok": true,
        "updated": true,
        "count": body.get("count"),
        "generatedAt": body.get("generatedAt"),
        "source": body.get("source"),
    })))
}

/// Manual / cron: pull Cubic Do_NOT_Apply once and replace the local copy.
pub async fn sync(State(state): State<AppState>, headers: HeaderMap) -> Result<impl IntoResponse, AppError> {
    assert_job_auth(&state, &headers).await?;
    let body = pull_and_store(&state, "sheet-sync").await.map_err(AppError::from)?;
    Ok(Json(json!({
        "ok": true,
        "updated": true,
        "count": body.get("count"),
        "generatedAt": body.get("generatedAt"),
        "source": body.get("source"),
        "copy": body,
    })))
}
