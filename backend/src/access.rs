use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::Mutex;

use crate::error::AppError;
use crate::jobs::assert_job_auth;
use crate::AppState;

const STORE_NAME: &str = "allowed-users.json";

#[derive(Clone, Debug, Serialize, Deserialize)]
struct AccessStore {
    ok: bool,
    #[serde(default)]
    source: String,
    #[serde(rename = "generatedAt")]
    generated_at: String,
    count: usize,
    emails: Vec<String>,
}

#[derive(Clone)]
struct AccessSnapshot {
    file_mtime: Option<SystemTime>,
    emails: HashSet<String>,
}

#[derive(Clone)]
pub struct AccessCache {
    snapshot: Arc<Mutex<Option<AccessSnapshot>>>,
}

impl AccessCache {
    pub fn new() -> Self {
        Self {
            snapshot: Arc::new(Mutex::new(None)),
        }
    }
}

#[derive(Deserialize, Default)]
pub struct AccessPushBody {
    emails: Option<Vec<String>>,
    #[serde(default)]
    source: String,
}

fn store_path(state: &AppState) -> PathBuf {
    state.cfg.data_dir.join(STORE_NAME)
}

fn normalize_emails(rows: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut set = HashSet::<String>::new();
    let mut out = Vec::new();
    for raw in rows {
        let email = raw.trim().to_lowercase();
        if email.is_empty() || !email.contains('@') {
            continue;
        }
        if set.insert(email.clone()) {
            out.push(email);
        }
    }
    out.sort();
    out
}

fn store_from_emails(emails: Vec<String>, source: &str) -> AccessStore {
    let emails = normalize_emails(emails);
    AccessStore {
        ok: true,
        source: source.to_string(),
        generated_at: chrono::Utc::now().to_rfc3339(),
        count: emails.len(),
        emails,
    }
}

fn store_to_json(store: &AccessStore) -> Value {
    json!({
        "ok": store.ok,
        "source": store.source,
        "generatedAt": store.generated_at,
        "count": store.count,
        "emails": store.emails,
    })
}

fn emails_set(store: &AccessStore) -> HashSet<String> {
    store.emails.iter().cloned().collect()
}

fn file_mtime(path: &PathBuf) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

async fn write_store(state: &AppState, store: &AccessStore) -> anyhow::Result<HashSet<String>> {
    let body = store_to_json(store);
    let emails = emails_set(store);
    if state.db.is_connected() {
        state.db.put_json("allowed-users", &body).await?;
    } else {
        let path = store_path(state);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&path, serde_json::to_vec_pretty(&body)?).await?;
    }
    *state.access.snapshot.lock().await = Some(AccessSnapshot {
        file_mtime: None,
        emails: emails.clone(),
    });
    Ok(emails)
}

fn read_store_file(path: &PathBuf) -> Option<(SystemTime, AccessStore)> {
    let raw = std::fs::read(path).ok()?;
    let mut store: AccessStore = serde_json::from_slice(&raw).ok()?;
    store.emails = normalize_emails(store.emails);
    store.count = store.emails.len();
    Some((file_mtime(path).unwrap_or(SystemTime::UNIX_EPOCH), store))
}

async fn cached_emails(state: &AppState) -> Option<HashSet<String>> {
    if let Some(body) = state.db.get_json("allowed-users").await {
        if let Ok(mut store) = serde_json::from_value::<AccessStore>(body) {
            store.emails = normalize_emails(store.emails);
            store.count = store.emails.len();
            let emails = emails_set(&store);
            *state.access.snapshot.lock().await = Some(AccessSnapshot {
                file_mtime: None,
                emails: emails.clone(),
            });
            return Some(emails);
        }
    }
    {
        let snap = state.access.snapshot.lock().await;
        if let Some(current) = snap.as_ref() {
            return Some(current.emails.clone());
        }
    }
    let path = store_path(state);
    if let Some((_mtime, store)) = read_store_file(&path) {
        let emails = emails_set(&store);
        *state.access.snapshot.lock().await = Some(AccessSnapshot {
            file_mtime: None,
            emails: emails.clone(),
        });
        return Some(emails);
    }
    None
}

/// "Email (Personal)" on Current_Market, found by header — the column has moved
/// as staff inserted columns (L → M → N).
async fn pull_from_sheet(state: &AppState) -> anyhow::Result<Vec<String>> {
    let market = crate::current_market::load(state, None).await?;
    if market.email.is_none() {
        anyhow::bail!("Current_Market header has no Email column");
    }
    Ok(normalize_emails(
        market.rows.iter().map(|row| market.cell(row, market.email).to_string()),
    ))
}

pub(crate) async fn pull_and_store(state: &AppState, source: &str) -> anyhow::Result<HashSet<String>> {
    let emails = pull_from_sheet(state).await?;
    // An empty read means a broken sheet or column, not "revoke everyone" — keep
    // the current list, same rule as the webhook handler.
    if emails.is_empty() {
        if let Some(existing) = cached_emails(state).await {
            if !existing.is_empty() {
                tracing::warn!(
                    "allowed-users: Current_Market returned no emails; keeping {} cached",
                    existing.len()
                );
                return Ok(existing);
            }
        }
    }
    let store = store_from_emails(emails, source);
    write_store(state, &store).await
}

/// Gmail ignores dots and a trailing "+tag" in the local part — two spellings
/// of the same address (e.g. "asha.shah228@gmail.com" vs "ashashah228@gmail.com")
/// land in the same inbox. Candidates self-report their email once on a form
/// and sign in with whatever their Google account canonically returns, so an
/// exact-string allow-list check silently locks out anyone whose stored
/// spelling doesn't match. Canonicalize both sides before comparing.
pub(crate) fn gmail_canonical(email: &str) -> String {
    let email = email.trim().to_lowercase();
    let Some((local, domain)) = email.split_once('@') else {
        return email;
    };
    if domain == "gmail.com" || domain == "googlemail.com" {
        let local = local.split('+').next().unwrap_or(local).replace('.', "");
        format!("{local}@gmail.com")
    } else {
        email
    }
}

/// Login gate: @cubicit.net + AUTH_EXTRA_EMAILS + AUTH_CANDIDATE_EMAILS always; others must be on Current_Market (local copy).
/// AUTH_ALLOW_ANY_GOOGLE=true bypasses the sheet list (emergency only).
pub async fn is_login_allowed(state: &AppState, email: &str) -> bool {
    let value = email.trim().to_lowercase();
    if value.is_empty() || !value.contains('@') {
        return false;
    }
    if state.cfg.allow_any_google {
        return true;
    }
    let domain = state.cfg.allowed_domain.trim().to_lowercase();
    if !domain.is_empty() && value.ends_with(&format!("@{domain}")) {
        return true;
    }
    if state.cfg.extra_emails.contains(&value) || state.cfg.candidate_emails.contains(&value) {
        return true;
    }
    let Some(set) = ensure_emails(state).await else {
        return false;
    };
    if set.contains(&value) {
        return true;
    }
    let canon = gmail_canonical(&value);
    set.iter().any(|allowed| gmail_canonical(allowed) == canon)
}

async fn ensure_emails(state: &AppState) -> Option<HashSet<String>> {
    if let Some(set) = cached_emails(state).await {
        return Some(set);
    }
    match pull_and_store(state, "sheet-bootstrap").await {
        Ok(set) => Some(set),
        Err(err) => {
            tracing::warn!(error = %err, "allowed-users bootstrap from Current_Market failed");
            None
        }
    }
}

/// Sheet changed → ping here (empty body) or POST emails. Empty body re-reads Current_Market.
pub async fn webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, AppError> {
    assert_job_auth(&state, &headers).await?;
    let parsed: AccessPushBody = if body.is_empty() {
        AccessPushBody::default()
    } else {
        serde_json::from_slice(&body).unwrap_or_default()
    };
    let source = if parsed.source.trim().is_empty() {
        "webhook"
    } else {
        parsed.source.trim()
    };
    if let Some(incoming) = parsed.emails.filter(|e| !e.is_empty()) {
        let emails = normalize_emails(incoming);
        let store = store_from_emails(emails, source);
        let set = write_store(&state, &store).await.map_err(AppError::from)?;
        return Ok(Json(json!({
            "ok": true,
            "updated": true,
            "count": set.len(),
            "generatedAt": store.generated_at,
            "source": store.source,
        })));
    }
    let set = pull_and_store(&state, source)
        .await
        .map_err(AppError::from)?;
    Ok(Json(json!({
        "ok": true,
        "updated": true,
        "count": set.len(),
        "source": source,
    })))
}

/// Manual / cron: pull Current_Market emails once and replace the local copy.
pub async fn sync(State(state): State<AppState>, headers: HeaderMap) -> Result<impl IntoResponse, AppError> {
    assert_job_auth(&state, &headers).await?;
    let set = pull_and_store(&state, "sheet-sync")
        .await
        .map_err(AppError::from)?;
    Ok(Json(json!({
        "ok": true,
        "updated": true,
        "count": set.len(),
        "source": "sheet-sync",
    })))
}
