//! Mirror spreadsheet snapshots into Mongo as soon as Google says the file changed.
//!
//! Sheets remain the source of truth. Google Drive POSTs `/api/webhooks/drive-changes`
//! when a watched workbook is edited. A ping to the older Apps Script webhook URLs
//! (with no row payload) also re-reads the sheet. A slow poll is only the safety net
//! if a Drive notification is dropped.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::AppState;

const DEBOUNCE_MS: u64 = 1500;
const WATCH_TTL: Duration = Duration::from_secs(12 * 60 * 60);
const WATCH_EXPIRY: Duration = Duration::from_secs(20 * 60 * 60);

struct Channel {
    file_id: String,
    resource_id: String,
}

struct Hub {
    token: String,
    channels: Mutex<HashMap<String, Channel>>,
    gen: AtomicU64,
}

fn hub() -> &'static Hub {
    static HUB: OnceLock<Hub> = OnceLock::new();
    HUB.get_or_init(|| Hub {
        token: Uuid::new_v4().to_string(),
        channels: Mutex::new(HashMap::new()),
        gen: AtomicU64::new(0),
    })
}

fn watch_url(state: &AppState) -> Option<String> {
    let base = state.cfg.site_url.trim().trim_end_matches('/');
    if !base.starts_with("https://") {
        return None;
    }
    let host = base.trim_start_matches("https://");
    if host.starts_with("localhost") || host.starts_with("127.0.0.1") {
        return None;
    }
    Some(format!("{base}/api/webhooks/drive-changes"))
}

pub fn spawn(state: AppState) {
    spawn_drive_watch(state.clone());
    let secs = state.cfg.sheet_watch_secs;
    if secs == 0 {
        tracing::info!("sheet → Mongo backup poll is off (SHEET_WATCH_SECS=0)");
        return;
    }
    tracing::info!("sheet → Mongo backup poll every {secs}s");
    tokio::spawn(async move {
        run_once(&state, "sheet-watch").await;
        loop {
            tokio::time::sleep(Duration::from_secs(secs)).await;
            run_once(&state, "sheet-watch").await;
        }
    });
}

fn spawn_drive_watch(state: AppState) {
    let Some(address) = watch_url(&state) else {
        tracing::info!("Drive change watch skipped (AUTH_SITE_URL is not public HTTPS)");
        return;
    };
    tracing::info!("watching Drive file changes → {address}");
    tokio::spawn(async move {
        loop {
            register_all(&state, &address).await;
            tokio::time::sleep(WATCH_TTL).await;
        }
    });
}

async fn register_all(state: &AppState, address: &str) {
    stop_all(state).await;
    let files = [
        state.cfg.hiring_spreadsheet_id.as_str(),
        state.cfg.data_candidate_spreadsheet_id.as_str(),
    ];
    for file_id in files {
        if file_id.is_empty() {
            continue;
        }
        match register_one(state, file_id, address).await {
            Ok(()) => tracing::info!(file_id, "Drive watch registered"),
            Err(err) => tracing::warn!(file_id, error = %err, "Drive watch failed — backup poll still runs"),
        }
    }
}

async fn register_one(state: &AppState, file_id: &str, address: &str) -> anyhow::Result<()> {
    let channel_id = Uuid::new_v4().to_string();
    let expiration = chrono::Utc::now() + chrono::Duration::from_std(WATCH_EXPIRY).unwrap_or(chrono::Duration::hours(20));
    let watched = state
        .google
        .drive_watch_file(
            file_id,
            &channel_id,
            address,
            &hub().token,
            expiration.timestamp_millis(),
        )
        .await?;
    let resource_id = watched
        .resource_id
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Drive watch returned no resourceId"))?;
    hub().channels.lock().await.insert(
        channel_id,
        Channel {
            file_id: file_id.to_string(),
            resource_id,
        },
    );
    Ok(())
}

async fn stop_all(state: &AppState) {
    let previous = {
        let mut map = hub().channels.lock().await;
        std::mem::take(&mut *map)
    };
    for (channel_id, ch) in previous {
        if let Err(err) = state
            .google
            .drive_stop_channel(&channel_id, &ch.resource_id)
            .await
        {
            tracing::debug!(channel_id, error = %err, "Drive channel stop skipped");
        }
    }
}

/// Google Drive POSTs here (empty body, X-Goog-* headers) when a watched sheet file changes.
pub async fn drive_changes(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let token = header_str(&headers, "x-goog-channel-token");
    if token.is_empty() || token != hub().token {
        return StatusCode::NO_CONTENT;
    }
    let resource_state = header_str(&headers, "x-goog-resource-state");
    if resource_state.eq_ignore_ascii_case("sync") {
        return StatusCode::OK;
    }
    let channel_id = header_str(&headers, "x-goog-channel-id");
    let file_id = {
        let map = hub().channels.lock().await;
        map.get(&channel_id).map(|ch| ch.file_id.clone())
    };
    let Some(file_id) = file_id else {
        return StatusCode::OK;
    };
    kick_refresh(state, file_id);
    StatusCode::OK
}

fn header_str(headers: &HeaderMap, name: &'static str) -> String {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .trim()
        .to_string()
}

fn kick_refresh(state: AppState, file_id: String) {
    let gen = hub().gen.fetch_add(1, Ordering::SeqCst) + 1;
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(DEBOUNCE_MS)).await;
        if hub().gen.load(Ordering::SeqCst) != gen {
            return;
        }
        refresh_file(&state, &file_id, "drive-watch").await;
    });
}

pub async fn refresh_file(state: &AppState, file_id: &str, source: &str) {
    if file_id == state.cfg.hiring_spreadsheet_id {
        pull_hiring(state, source).await;
        pull_interviews(state, source).await;
        pull_dna(state, source).await;
        return;
    }
    if file_id == state.cfg.data_candidate_spreadsheet_id {
        pull_access(state, source).await;
        return;
    }
    run_once(state, source).await;
}

async fn run_once(state: &AppState, source: &str) {
    pull_hiring(state, source).await;
    pull_interviews(state, source).await;
    pull_dna(state, source).await;
    pull_access(state, source).await;
}

async fn pull_hiring(state: &AppState, source: &str) {
    match crate::hiring::pull_and_store(state, source).await {
        Ok(_) => {}
        Err(err) => tracing::warn!(source, error = %err, "sheet-watch hiring failed"),
    }
}

async fn pull_interviews(state: &AppState, source: &str) {
    match crate::interviews::pull_and_store(state, source).await {
        Ok(_) => {}
        Err(err) => tracing::warn!(source, error = %err, "sheet-watch cubic interviews failed"),
    }
}

async fn pull_dna(state: &AppState, source: &str) {
    match crate::dna::pull_and_store(state, source).await {
        Ok(_) => {}
        Err(err) => tracing::warn!(source, error = %err, "sheet-watch do-not-apply failed"),
    }
}

async fn pull_access(state: &AppState, source: &str) {
    match crate::access::pull_and_store(state, source).await {
        Ok(_) => {}
        Err(err) => tracing::warn!(source, error = %err, "sheet-watch allowed-users failed"),
    }
}
