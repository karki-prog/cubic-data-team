pub mod readers;
pub mod retention;
pub mod state;
pub mod sync;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde_json::json;

use crate::auth::{cookie_value, verify_token, ACCESS_COOKIE, REFRESH_COOKIE};
use crate::booking::drain_booking_queue;
use crate::error::AppError;
use crate::AppState;

pub async fn assert_job_auth(state: &AppState, headers: &HeaderMap) -> Result<(), AppError> {
    let secret = &state.cfg.cron_secret;
    let header = headers
        .get("authorization")
        .or_else(|| headers.get("x-cron-secret"))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let token = header
        .strip_prefix("Bearer ")
        .or_else(|| header.strip_prefix("bearer "))
        .unwrap_or(header)
        .trim();
    if !secret.is_empty() && token == secret {
        return Ok(());
    }
    let access = cookie_value(headers, ACCESS_COOKIE);
    let refresh = cookie_value(headers, REFRESH_COOKIE);
    if !access.is_empty() && verify_token(&state.cfg, &access, "access").is_ok() {
        return Ok(());
    }
    if !refresh.is_empty() && verify_token(&state.cfg, &refresh, "refresh").is_ok() {
        return Ok(());
    }
    if secret.is_empty() {
        if std::env::var("VERCEL").is_ok() {
            return Err(AppError::Internal("CRON_SECRET is required in production.".into()));
        }
        return Ok(());
    }
    Err(AppError::Forbidden("Forbidden".into()))
}

pub async fn daily(State(state): State<AppState>, headers: HeaderMap) -> Result<impl IntoResponse, AppError> {
    assert_job_auth(&state, &headers).await?;
    let bookings = drain_booking_queue(&state).await.map_err(AppError::from)?;
    let prune = retention::prune_phone_calls_older_than_retention(&state)
        .await
        .map_err(AppError::from)?;
    let phone_call_links = retention::repair_phone_calls_hyperlinks(&state)
        .await
        .map_err(AppError::from)?;
    let status = sync::sync_sheet_status_to_google(&state)
        .await
        .map_err(AppError::from)?;
    // Refresh the login allow-list from Current_Market so new candidates can sign
    // in without a manual sync. Non-fatal.
    let allowed_users = match crate::access::pull_and_store(&state, "daily").await {
        Ok(set) => json!({ "count": set.len() }),
        Err(err) => {
            tracing::warn!("[daily] allowed-users refresh failed: {err}");
            json!({ "error": err.to_string() })
        }
    };
    let hiring = match crate::hiring::pull_and_store(&state, "daily").await {
        Ok(body) => json!({ "jobCount": body.get("jobCount") }),
        Err(err) => {
            tracing::warn!("[daily] hiring refresh failed: {err}");
            json!({ "error": err.to_string() })
        }
    };
    let interviews = match crate::interviews::pull_and_store(&state, "daily").await {
        Ok(body) => json!({ "count": body.get("count") }),
        Err(err) => {
            tracing::warn!("[daily] cubic interviews refresh failed: {err}");
            json!({ "error": err.to_string() })
        }
    };
    let dna = match crate::dna::pull_and_store(&state, "daily").await {
        Ok(body) => json!({ "count": body.get("count") }),
        Err(err) => {
            tracing::warn!("[daily] do-not-apply refresh failed: {err}");
            json!({ "error": err.to_string() })
        }
    };
    Ok(Json(json!({
        "ok": true,
        "result": {
            "bookings": bookings,
            "prune": prune,
            "phoneCallLinks": phone_call_links,
            "status": status,
            "allowedUsers": allowed_users,
            "hiring": hiring,
            "cubicInterviews": interviews,
            "doNotApply": dna,
            "ranAt": chrono::Utc::now().to_rfc3339()
        }
    })))
}

pub async fn status_sync(State(state): State<AppState>, headers: HeaderMap) -> Result<impl IntoResponse, AppError> {
    assert_job_auth(&state, &headers).await?;
    let result = sync::sync_sheet_status_to_google(&state)
        .await
        .map_err(AppError::from)?;
    Ok(Json(json!({ "ok": true, "result": result })))
}

pub async fn booking_drain(State(state): State<AppState>, headers: HeaderMap) -> Result<impl IntoResponse, AppError> {
    assert_job_auth(&state, &headers).await?;
    let result = drain_booking_queue(&state).await.map_err(AppError::from)?;
    Ok(Json(json!({ "ok": true, "result": result })))
}

pub async fn phone_calls_prune(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, AppError> {
    assert_job_auth(&state, &headers).await?;
    let result = retention::prune_phone_calls_older_than_retention(&state)
        .await
        .map_err(AppError::from)?;
    Ok(Json(json!({ "ok": true, "result": result })))
}

pub async fn phone_calls_repair_links(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, AppError> {
    assert_job_auth(&state, &headers).await?;
    let result = retention::repair_phone_calls_hyperlinks(&state)
        .await
        .map_err(AppError::from)?;
    Ok(Json(json!({ "ok": true, "result": result })))
}

pub async fn google_health(State(state): State<AppState>, headers: HeaderMap) -> Result<impl IntoResponse, AppError> {
    assert_job_auth(&state, &headers).await?;
    let result = check_google_backend(&state).await;
    let ok = result.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
    let status = if ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    Ok((status, Json(json!({ "ok": ok, "result": result }))))
}

async fn check_google_backend(state: &AppState) -> serde_json::Value {
    let sa = "sheets-automation@cubic-interview-automation.iam.gserviceaccount.com";
    let sheets = [
        ("sheet.connector", state.cfg.connector_spreadsheet_id.as_str(), true),
        (
            "sheet.dataInterview",
            state.cfg.data_interview_spreadsheet_id.as_str(),
            true,
        ),
        (
            "sheet.currentMarket",
            state.cfg.data_candidate_spreadsheet_id.as_str(),
            false,
        ),
        (
            "sheet.cubicSchedule",
            state.cfg.appointment_spreadsheet_id.as_str(),
            false,
        ),
        ("sheet.hiring", state.cfg.hiring_spreadsheet_id.as_str(), false),
    ];
    let mut checks = Vec::new();
    for (id, spreadsheet_id, need_write) in sheets {
        checks.push(check_sheet(state, id, spreadsheet_id, need_write, sa).await);
    }
    checks.push(check_drive_folder(state).await);
    checks.push(check_drive_upload(state).await);
    checks.push(check_gmail(state).await);
    checks.push(check_calendar(state).await);
    let ok = checks.iter().all(|c| c["ok"].as_bool() == Some(true));
    json!({
        "ok": ok,
        "nativeBooking": !state.cfg.booking_use_apps_script,
        "checks": checks,
        "checkedAt": chrono::Utc::now().to_rfc3339()
    })
}

async fn check_sheet(
    state: &AppState,
    id: &str,
    spreadsheet_id: &str,
    need_write: bool,
    sa: &str,
) -> serde_json::Value {
    match state.google.spreadsheet_title(spreadsheet_id).await {
        Ok(title) => match state
            .google
            .drive_get(spreadsheet_id, "capabilities(canEdit)", false)
            .await
        {
            Ok(file) => {
                let can_edit = file.capabilities.and_then(|c| c.can_edit).unwrap_or(false);
                if need_write && !can_edit {
                    json!({
                        "id": id,
                        "ok": false,
                        "detail": format!("\"{title}\" is readable but not editable. Share Editor with {sa}.")
                    })
                } else {
                    json!({
                        "id": id,
                        "ok": true,
                        "detail": format!("\"{title}\" {}", if can_edit { "writer" } else { "reader" })
                    })
                }
            }
            Err(err) => json!({"id": id, "ok": false, "detail": err.to_string()}),
        },
        Err(err) => json!({"id": id, "ok": false, "detail": err.to_string()}),
    }
}

async fn check_drive_folder(state: &AppState) -> serde_json::Value {
    let folder_id = &state.cfg.resumes_data_folder_id;
    match state
        .google
        .drive_get(folder_id, "name,capabilities(canEdit,canAddChildren)", false)
        .await
    {
        Ok(file) => {
            let q = format!(
                "'{folder_id}' in parents and mimeType='application/vnd.google-apps.folder' and trashed=false"
            );
            let listed = state.google.drive_list(&q, "files(id)", 1, None, None).await;
            let n = listed.map(|(f, _)| f.len()).unwrap_or(0);
            json!({
                "id": "drive.resumesData.list",
                "ok": true,
                "detail": format!("\"{}\" list ok ({})", file.name.unwrap_or_default(), if n > 0 { "has folders" } else { "empty/first page empty" })
            })
        }
        Err(err) => json!({
            "id": "drive.resumesData.list",
            "ok": false,
            "detail": err.to_string()
        }),
    }
}

async fn check_drive_upload(state: &AppState) -> serde_json::Value {
    let folder_id = &state.cfg.resumes_data_folder_id;
    let name = format!("_cubic_health_{}.txt", chrono::Utc::now().timestamp_millis());
    match state
        .google
        .upload_bytes(folder_id, &name, "text/plain", b"ok")
        .await
    {
        Ok((id, _, _)) => {
            let _ = state.google.drive_delete(&id, true).await;
            json!({ "id": "drive.resumesData.upload", "ok": true, "detail": "user OAuth upload ok" })
        }
        Err(err) => json!({
            "id": "drive.resumesData.upload",
            "ok": false,
            "detail": err.to_string()
        }),
    }
}

async fn check_gmail(state: &AppState) -> serde_json::Value {
    match state.google.gmail_probe().await {
        Ok(()) => json!({
            "id": "gmail.send",
            "ok": true,
            "detail": format!("{} token ok", state.cfg.mail_from_email)
        }),
        Err(err) => json!({ "id": "gmail.send", "ok": false, "detail": err.to_string() }),
    }
}

async fn check_calendar(state: &AppState) -> serde_json::Value {
    match state.google.calendar_probe().await {
        Ok(name) => json!({ "id": "calendar", "ok": true, "detail": format!("ok ({name})") }),
        Err(err) => json!({ "id": "calendar", "ok": false, "detail": err.to_string() }),
    }
}
