use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use axum::extract::{Multipart, State};
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::auth::require_session;
use crate::dates::{
    chicago_timestamp, format_mdy, format_meeting_time, format_time_display, is_phone_call_stage,
    normalize_meeting_time_cst, parse_date_key, parse_time_minutes,
};
use crate::availability::{meeting_capacity, sheet_and_queue_intervals};
use crate::error::{booking_error, AppError};
use crate::google::drive::folder_view_url;
use crate::google::mail::SendMailInput;
use crate::google::sheets::hyperlink_formula;
use crate::headers::{self, col, sparse_row};
use crate::staff::{normalize_email, resolve_poc_sheet_name};
use crate::types::{record_fingerprint, SheetKind};
use crate::AppState;

const MAX_RESUME_BYTES: usize = 5 * 1024 * 1024;
/// Phone calls tab: header row 1, two blank spacer rows, data from row 4
/// (Apps Script PHONE_CALLS_DATA_START_ROW keeps the same gap).
pub const PHONE_CALLS_DATA_START_ROW: i32 = 4;
const STAGES: &[&str] = &[
    "Initial interview",
    "Video call",
    "Recruiter screening",
    "2nd interview",
    "3rd interview",
    "4th interview",
    "Final",
];

#[derive(Clone)]
pub struct BookingQueue {
    draining: Arc<AtomicBool>,
    pending_kick: Arc<AtomicBool>,
}

impl BookingQueue {
    pub fn new() -> Self {
        Self {
            draining: Arc::new(AtomicBool::new(false)),
            pending_kick: Arc::new(AtomicBool::new(false)),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueuedPayload {
    pub candidate_name: String,
    pub interview_stage: String,
    pub location: String,
    pub interview_platform: String,
    pub meeting_date: String,
    pub meeting_time: String,
    pub meeting_duration: String,
    pub panel: String,
    pub client: String,
    pub vendor: String,
    pub job_description: String,
    pub special_note: String,
    pub resume_filename: String,
    pub resume_content_type: String,
    pub skip_thank_you_email: bool,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct JobMeta {
    id: String,
    status: String,
    attempts: u32,
    #[serde(rename = "createdAt")]
    created_at: String,
    #[serde(rename = "updatedAt")]
    updated_at: String,
    error: Option<String>,
    payload: QueuedPayload,
}

#[derive(Clone, Default)]
pub struct CandidateProfile {
    pub email: String,
    pub poc: String,
    pub tech: String,
    pub visa: String,
    pub location: String,
}

fn normalize_name(name: &str) -> String {
    name.trim()
        .to_lowercase()
        .replace('_', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub async fn lookup_candidate_profile(state: &AppState, candidate_name: &str) -> anyhow::Result<CandidateProfile> {
    let target = normalize_name(candidate_name);
    if target.is_empty() {
        return Ok(CandidateProfile {
            tech: "Data".into(),
            ..Default::default()
        });
    }
    let market =
        crate::current_market::load(state, Some(std::time::Duration::from_secs(90))).await?;
    for row in &market.rows {
        if normalize_name(market.cell(row, market.name)) != target {
            continue;
        }
        return Ok(CandidateProfile {
            poc: market.cell(row, market.poc).to_string(),
            email: normalize_email(market.cell(row, market.email)),
            tech: "Data".into(),
            visa: market.cell(row, market.visa).to_string(),
            location: market.cell(row, market.location).to_string(),
        });
    }
    Ok(CandidateProfile {
        tech: "Data".into(),
        ..Default::default()
    })
}

fn normalize_location(form_location: &str, profile: &CandidateProfile) -> String {
    let loc = form_location.trim();
    if !loc.is_empty() {
        return loc.to_string();
    }
    let marketing = profile.location.trim();
    if marketing.eq_ignore_ascii_case("remote") {
        "Remote".into()
    } else if marketing.eq_ignore_ascii_case("onsite") {
        "Onsite".into()
    } else if marketing.eq_ignore_ascii_case("hybrid") {
        "Hybrid".into()
    } else {
        loc.to_string()
    }
}

async fn ensure_stage_folder_tree(
    state: &AppState,
    candidate: &str,
    client: &str,
    stage: &str,
) -> anyhow::Result<(String, String)> {
    let root = &state.cfg.resumes_data_folder_id;
    let candidate_id = state.google.find_or_create_folder(root, candidate).await?;
    let company_id = state.google.find_or_create_folder(&candidate_id, client).await?;
    let stage_id = state.google.find_or_create_folder(&company_id, stage).await?;
    state.google.make_anyone_reader(&candidate_id).await;
    state.google.make_anyone_reader(&company_id).await;
    state.google.make_anyone_reader(&stage_id).await;
    let path = format!("Resumes_Data/{candidate}/{client}/{stage}");
    Ok((stage_id, path))
}

async fn remember_pending(state: &AppState, key: &str, kind: SheetKind, status: &str) -> anyhow::Result<()> {
    crate::jobs::state::remember_pending_booking(state, key, kind, status).await
}


/// True when the destination tab already holds a row for this exact booking
/// (same candidate + client + stage + date + time). A lost HTTP response, a
/// service restart mid-drain, or a double queue-drain would otherwise write the
/// same booking twice — this makes the append idempotent.
async fn connector_booking_exists(
    state: &AppState,
    payload: &QueuedPayload,
    profile: &CandidateProfile,
) -> bool {
    let phone = is_phone_call_stage(&payload.interview_stage);
    let (spreadsheet_id, sheet_name, start_row) = if phone {
        (
            state.cfg.data_interview_spreadsheet_id.as_str(),
            state.cfg.phone_calls_sheet.clone(),
            2,
        )
    } else {
        (
            state.cfg.connector_spreadsheet_id.as_str(),
            resolve_poc_sheet_name(&profile.poc),
            state.cfg.connector_data_start_row,
        )
    };
    let headers = match state.google.load_headers(spreadsheet_id, &sheet_name).await {
        Ok(h) => h,
        Err(err) => {
            tracing::warn!("[booking] duplicate check skipped ({sheet_name}): {err}");
            return false;
        }
    };
    let want = record_fingerprint(
        &sheet_name,
        &payload.candidate_name,
        &payload.client,
        &parse_date_key(&payload.meeting_date),
        &payload.meeting_time,
        if phone { "Phone call" } else { &payload.interview_stage },
    );
    let last = headers
        .idx(col::CLIENT)
        .into_iter()
        .chain(headers.idx(col::TIME))
        .chain(headers.idx(col::CANDIDATE))
        .max()
        .unwrap_or_else(|| headers.last_named_idx());
    let range = format!("'{sheet_name}'!A{start_row}:{}", headers::a1_col(last));
    let rows = match state
        .google
        .sheets_values_get(spreadsheet_id, &range, Some("FORMATTED_VALUE"))
        .await
    {
        Ok(rows) => rows,
        Err(err) => {
            tracing::warn!("[booking] duplicate check skipped ({sheet_name}): {err}");
            return false;
        }
    };
    for row in &rows {
        let cand = headers.get(row, col::CANDIDATE);
        if cand.is_empty() {
            continue;
        }
        let stage = if phone {
            "Phone call".to_string()
        } else {
            headers.get_owned(row, col::STAGE)
        };
        let got = record_fingerprint(
            &sheet_name,
            cand,
            headers.get(row, col::CLIENT),
            &parse_date_key(&headers.get_owned(row, col::DATE)),
            headers.get(row, col::TIME),
            &stage,
        );
        if got == want {
            return true;
        }
    }
    false
}

pub async fn process_native_booking(state: &AppState, payload: &QueuedPayload, resume: &[u8]) -> anyhow::Result<serde_json::Value> {
    if payload.candidate_name.trim().is_empty() {
        anyhow::bail!("Candidate Name is required.");
    }
    if payload.client.trim().is_empty() {
        anyhow::bail!("Client is required.");
    }
    if payload.interview_stage.trim().is_empty() {
        anyhow::bail!("Interview Stage is required.");
    }
    if resume.is_empty() {
        anyhow::bail!("Resume upload is required.");
    }
    if payload.job_description.trim().is_empty() {
        anyhow::bail!("Job Description is required.");
    }
    let profile = lookup_candidate_profile(state, &payload.candidate_name).await?;
    if profile.poc.is_empty() {
        anyhow::bail!(
            "Nepal POC is missing for \"{}\" on Current_Market. Add POC and retry.",
            payload.candidate_name
        );
    }

    // Idempotency: if this booking is already on the sheet (client retry, a
    // restart mid-drain, a double drain), do not write it a second time.
    if connector_booking_exists(state, payload, &profile).await {
        tracing::info!(
            "[booking] duplicate suppressed: {} / {} / {} {}",
            payload.candidate_name,
            payload.client,
            payload.meeting_date,
            payload.meeting_time
        );
        return Ok(json!({
            "success": true,
            "duplicate": true,
            "candidateName": payload.candidate_name,
            "companyName": payload.client,
            "submitterEmail": profile.email,
        }));
    }

    let location = normalize_location(&payload.location, &profile);
    let stage_name = if payload.interview_stage.is_empty() {
        "Unspecified"
    } else {
        payload.interview_stage.as_str()
    };
    let (stage_folder_id, folder_path) =
        ensure_stage_folder_tree(state, &payload.candidate_name, &payload.client, stage_name).await?;
    let folder_url = folder_view_url(&stage_folder_id);

    let mut resume_name = payload.resume_filename.clone();
    if !resume_name.to_lowercase().ends_with(".pdf") {
        if let Some((stem, _)) = resume_name.rsplit_once('.') {
            resume_name = format!("{stem}.pdf");
        } else {
            resume_name.push_str(".pdf");
        }
    }
    if resume_name.is_empty() {
        resume_name = "Resume.pdf".into();
    }
    let mime = if payload.resume_content_type.is_empty() {
        "application/pdf"
    } else {
        payload.resume_content_type.as_str()
    };
    let (_, _, resume_url) = state
        .google
        .upload_bytes(&stage_folder_id, &resume_name, mime, resume)
        .await?;
    let (_, _, jd_url) = state
        .google
        .upload_bytes(
            &stage_folder_id,
            "Job_Description.txt",
            "text/plain",
            payload.job_description.trim().as_bytes(),
        )
        .await?;

    if is_phone_call_stage(&payload.interview_stage) {
        append_phone_call_row(state, payload, &profile, &location, &resume_url, &jd_url, &folder_url).await?;
    } else {
        append_interview_row(state, payload, &profile, &location, &resume_url, &jd_url, &folder_url).await?;
    }

    let sheet_name = if is_phone_call_stage(&payload.interview_stage) {
        "Phone calls".to_string()
    } else {
        resolve_poc_sheet_name(&profile.poc)
    };
    let key = record_fingerprint(
        &sheet_name,
        &payload.candidate_name,
        &payload.client,
        &parse_date_key(&payload.meeting_date),
        &payload.meeting_time,
        &payload.interview_stage,
    );
    if let Err(err) = remember_pending(
        state,
        &key,
        if is_phone_call_stage(&payload.interview_stage) {
            SheetKind::Phone
        } else {
            SheetKind::Interview
        },
        "Pending",
    )
    .await
    {
        tracing::warn!("[booking] backend state seed skipped: {err}");
    }

    Ok(json!({
        "success": true,
        "candidateName": payload.candidate_name,
        "companyName": payload.client,
        "folderPath": folder_path,
        "submitterEmail": profile.email,
    }))
}

async fn append_interview_row(
    state: &AppState,
    payload: &QueuedPayload,
    profile: &CandidateProfile,
    location: &str,
    resume_url: &str,
    jd_url: &str,
    folder_url: &str,
) -> anyhow::Result<()> {
    let sheet_name = resolve_poc_sheet_name(&profile.poc);
    let spreadsheet_id = &state.cfg.connector_spreadsheet_id;
    let headers = state.google.load_headers(spreadsheet_id, &sheet_name).await?;
    let last_col = headers::a1_col(
        headers
            .idx(col::EMAIL)
            .or(headers.idx(col::CLIENT))
            .unwrap_or_else(|| headers.last_named_idx()),
    );
    let row = state
        .google
        .connector_next_append_row(
            spreadsheet_id,
            &sheet_name,
            state.cfg.connector_data_start_row,
            &last_col,
        )
        .await?;
    if let Some(ts_idx) = headers.idx(col::TIMESTAMP) {
        state
            .google
            .sheets_values_update(
                spreadsheet_id,
                &format!("'{sheet_name}'!{}{row}", headers::a1_col(ts_idx)),
                vec![vec![json!(chicago_timestamp())]],
                "RAW",
            )
            .await?;
    }
    let mut pairs = vec![
        (headers.must(col::CANDIDATE)?, json!(payload.candidate_name)),
        (headers.must(col::STAGE)?, json!(payload.interview_stage)),
        (headers.must(col::LOCATION)?, json!(location)),
        (headers.must(col::POC)?, json!(profile.poc)),
        (headers.must(col::DATE)?, json!(payload.meeting_date)),
        (headers.must(col::TIME)?, json!(format_meeting_time(&payload.meeting_time))),
        (headers.must(col::DURATION)?, json!(payload.meeting_duration)),
        (headers.must(col::CLIENT)?, json!(payload.client)),
    ];
    pairs.extend(
        [
            headers.pair(col::PLATFORM, json!(payload.interview_platform)),
            headers.pair(col::VENDOR, json!(payload.vendor)),
            headers.pair(col::PANEL, json!(payload.panel)),
            headers.pair(col::STATUS, json!(state.cfg.status_default)),
            headers.pair(col::NOTE, json!(payload.special_note)),
            headers.pair(col::EMAIL, json!(profile.email)),
        ]
        .into_iter()
        .flatten(),
    );
    let (min, max, values) = sparse_row(&pairs);
    state
        .google
        .sheets_values_update(
            spreadsheet_id,
            &headers.range_row(&sheet_name, row, min, max),
            vec![values],
            "USER_ENTERED",
        )
        .await?;
    let hidden = headers.hidden_run(3);
    if let Some(&start) = hidden.first() {
        state
            .google
            .write_hidden_urls(
                spreadsheet_id,
                &sheet_name,
                row,
                start,
                [resume_url, jd_url, folder_url],
            )
            .await?;
    }
    match state
        .google
        .restore_connector_hyperlinks(
            spreadsheet_id,
            &sheet_name,
            state.cfg.connector_data_start_row,
        )
        .await
    {
        Ok(n) if n > 0 => {
            tracing::info!("Restored {n} connector hyperlink(s) on {sheet_name}")
        }
        Ok(_) => {}
        Err(err) => tracing::warn!(error = %err, "connector hyperlink restore failed (row still written)"),
    }
    Ok(())
}

async fn append_phone_call_row(
    state: &AppState,
    payload: &QueuedPayload,
    profile: &CandidateProfile,
    location: &str,
    resume_url: &str,
    jd_url: &str,
    folder_url: &str,
) -> anyhow::Result<()> {
    let spreadsheet_id = &state.cfg.data_interview_spreadsheet_id;
    let sheet_name = &state.cfg.phone_calls_sheet;
    let headers = state.google.load_headers(spreadsheet_id, sheet_name).await?;
    let row = state
        .google
        .phone_calls_next_append_row(spreadsheet_id, sheet_name, PHONE_CALLS_DATA_START_ROW)
        .await?;
    let date = match format_mdy(&parse_date_key(&payload.meeting_date)) {
        md if md.is_empty() => payload.meeting_date.clone(),
        md => md,
    };
    let link = |url: &str, label: &str| -> String {
        if url.trim().is_empty() {
            String::new()
        } else {
            hyperlink_formula(url, label)
        }
    };
    let tech = if profile.tech.is_empty() {
        "Data".to_string()
    } else {
        profile.tech.clone()
    };
    let mut pairs = vec![
        (headers.must(col::POC)?, json!(profile.poc)),
        (headers.must(col::CLIENT)?, json!(payload.client)),
        (headers.must(col::CANDIDATE)?, json!(payload.candidate_name)),
        (headers.must(col::DATE)?, json!(date)),
        (headers.must(col::TIME)?, json!(format_meeting_time(&payload.meeting_time))),
        (headers.must(col::DURATION)?, json!(payload.meeting_duration)),
    ];
    pairs.extend(
        [
            headers.pair(col::TECH, json!(tech)),
            headers.pair(col::LOCATION, json!(location)),
            headers.pair(col::VISA, json!(profile.visa)),
            headers.pair(col::PANEL, json!(payload.panel)),
            headers.pair(col::STATUS, json!(state.cfg.status_default)),
            headers.pair(col::JD, json!(link(jd_url, "Job Description Link"))),
            headers.pair(col::RESUME, json!(link(resume_url, "Resume Link"))),
            headers.pair(col::FOLDER, json!(link(folder_url, "Drive Folder Link"))),
            headers.pair(col::NOTE, json!(payload.special_note)),
            headers.pair(col::EMAIL, json!(profile.email)),
        ]
        .into_iter()
        .flatten(),
    );
    let (min, max, values) = sparse_row(&pairs);
    state
        .google
        .sheets_values_update(
            spreadsheet_id,
            &headers.range_row(sheet_name, row, min, max),
            vec![values],
            "USER_ENTERED",
        )
        .await?;
    let hidden = headers.hidden_run(3);
    if let Some(&start) = hidden.first() {
        state
            .google
            .write_hidden_urls(
                spreadsheet_id,
                sheet_name,
                row,
                start,
                [jd_url, resume_url, folder_url],
            )
            .await?;
    }
    Ok(())
}

fn esc(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn first_name(full: &str) -> String {
    full.trim().split_whitespace().next().unwrap_or("").to_string()
}

async fn send_thank_you(state: &AppState, payload: &QueuedPayload, poc: &str, to_email: &str) -> anyhow::Result<(bool, String)> {
    let to = normalize_email(to_email);
    if to.is_empty() {
        return Ok((false, String::new()));
    }
    let phone = is_phone_call_stage(&payload.interview_stage);
    let kind = if phone {
        "phone call request"
    } else {
        "interview request"
    };
    let client = payload.client.trim();
    let greeting = if !first_name(&payload.candidate_name).is_empty() {
        format!("Hello {}", first_name(&payload.candidate_name))
    } else if !payload.candidate_name.is_empty() {
        format!("Hello {}", payload.candidate_name)
    } else {
        "Hello".into()
    };
    let subject = if client.is_empty() {
        format!("We received your {kind}")
    } else {
        format!("We received your {kind} — {client}")
    };
    let p1 = format!(
        "Thank you for your {kind}{}. We got it and our team is reviewing it now.",
        if client.is_empty() {
            String::new()
        } else {
            format!(" for {client}")
        }
    );
    let p2 = "You will get another email when it is accepted or declined. You do not need to do anything else right now.";
    let p3 = if poc.is_empty() {
        "If something needs to change, reply to this email or contact your POC.".into()
    } else {
        format!("If something needs to change, reply to this email or contact your POC ({poc}).")
    };
    let rows = [
        ("Candidate", payload.candidate_name.as_str()),
        ("Client", payload.client.as_str()),
        ("Interview stage", payload.interview_stage.as_str()),
        ("Meeting date", payload.meeting_date.as_str()),
        ("Meeting time (CST)", payload.meeting_time.as_str()),
        ("Duration", payload.meeting_duration.as_str()),
        ("POC", poc),
        ("Platform", payload.interview_platform.as_str()),
        ("Panel", payload.panel.as_str()),
        ("Vendor", payload.vendor.as_str()),
    ];
    let details: Vec<_> = rows.into_iter().filter(|(_, v)| !v.trim().is_empty()).collect();
    let details_plain = details
        .iter()
        .map(|(k, v)| format!("{k}: {v}"))
        .collect::<Vec<_>>()
        .join("\n");
    let plain = format!(
        "{greeting},\n\n{p1}\n\n{p2}\n\n{p3}\n\nRequest details\n{details_plain}\n\n— Cubic Interview Team"
    );
    let detail_rows_html = details
        .iter()
        .map(|(label, value)| {
            format!(
                "<tr><td style=\"padding:10px 16px 10px 0;width:36%;font-family:Arial,Helvetica,sans-serif;font-size:14px;color:#000;border-bottom:1px solid #e5e5e5;\">{}</td><td style=\"padding:10px 0;font-family:Arial,Helvetica,sans-serif;font-size:14px;font-weight:700;color:#000;border-bottom:1px solid #e5e5e5;\">{}</td></tr>",
                esc(label),
                esc(value)
            )
        })
        .collect::<String>();
    let html = format!(
        r#"<!DOCTYPE html><html><body style="margin:0;padding:0;background:#f2f2f2;">
<table role="presentation" width="100%" style="background:#f2f2f2;"><tr><td align="center" style="padding:28px 14px;">
<table role="presentation" width="600" style="width:100%;max-width:600px;background:#fff;border:1px solid #ddd;border-radius:12px;overflow:hidden;">
<tr><td style="padding:20px 28px;background:#c5221f;"><div style="font-family:Arial,Helvetica,sans-serif;font-size:18px;font-weight:700;color:#fff;">Cubic Technologies</div><div style="margin-top:4px;font-family:Arial,Helvetica,sans-serif;font-size:13px;color:#fff;">Interview Request</div></td></tr>
<tr><td style="padding:26px 28px 8px;"><h1 style="margin:0;font-family:Arial,Helvetica,sans-serif;font-size:22px;font-weight:700;color:#000;">We received your {}</h1></td></tr>
<tr><td style="padding:18px 28px 8px;">
<p style="margin:0 0 14px;font-family:Arial,Helvetica,sans-serif;font-size:15px;line-height:1.55;color:#000;">{},</p>
<p style="margin:0 0 14px;font-family:Arial,Helvetica,sans-serif;font-size:15px;line-height:1.55;color:#000;">Thank you for your {}{}. We got it and our team is reviewing it now.</p>
<p style="margin:0 0 14px;font-family:Arial,Helvetica,sans-serif;font-size:15px;line-height:1.55;color:#000;">{}</p>
<p style="margin:0 0 14px;font-family:Arial,Helvetica,sans-serif;font-size:15px;line-height:1.55;color:#000;">{}</p>
<div style="margin:22px 0 6px;"><div style="margin:0 0 10px;font-family:Arial,Helvetica,sans-serif;font-size:14px;font-weight:700;color:#000;">Request details</div>
<table role="presentation" width="100%" style="border-collapse:collapse;width:100%;">{}</table></div>
</td></tr>
<tr><td style="padding:20px 28px 28px;">
<p style="margin:0 0 4px;font-family:Arial,Helvetica,sans-serif;font-size:14px;color:#000;">Questions? Reply to this email or contact your POC.</p>
<p style="margin:16px 0 0;font-family:Arial,Helvetica,sans-serif;font-size:14px;color:#000;">— Cubic Interview Team</p>
</td></tr>
</table></td></tr></table>
</body></html>"#,
        esc(kind),
        esc(&greeting),
        esc(kind),
        if client.is_empty() {
            String::new()
        } else {
            format!(" for <strong>{}</strong>", esc(client))
        },
        esc(p2),
        if poc.is_empty() {
            esc(&p3)
        } else {
            format!(
                "If something needs to change, reply to this email or contact your POC (<strong>{}</strong>).",
                esc(poc)
            )
        },
        detail_rows_html
    );

    // CC the POCs + Sushant so the team gets a copy back.
    let mut cc: Vec<String> = Vec::new();
    for email in state
        .cfg
        .poc_emails
        .values()
        .cloned()
        .chain(["sushantmaharjan@cubicit.net".into()])
    {
        let n = normalize_email(&email);
        if n.is_empty() || n == to || cc.contains(&n) {
            continue;
        }
        cc.push(n);
    }

    // Test mode: route the whole email to one address.
    let (final_to, final_cc, final_subject) = match &state.cfg.test_email {
        Some(test) => (
            test.clone(),
            Vec::new(),
            format!("[TEST → would go to {to}] {subject}"),
        ),
        None => (to.clone(), cc, subject),
    };

    state
        .google
        .send_mail(SendMailInput {
            to: final_to,
            cc: final_cc,
            subject: final_subject,
            plain,
            html,
            // From: reminder@cubicit.net; replies come back to karki.
            reply_to: "karki@cubicit.net".into(),
        })
        .await?;
    Ok((true, to))
}

async fn submit_apps_script(state: &AppState, payload: &QueuedPayload, resume: &[u8]) -> anyhow::Result<serde_json::Value> {
    let urls = [
        "https://script.google.com/macros/s/AKfycbxVwxl0t4V_ADtqQGLOtHLbY0yh5yZxaOxMEBZmdl1adwEhp9iju7GpZ9AcabzyRMskWg/exec",
        "https://script.google.com/macros/s/AKfycbwf10_WwjJ51JtoNKRKAnmpHx9BcYaAqSrYJwbwLKnP0B1-YHStb9c-pwnPLAIJ9Rl-mQ/exec",
    ];
    let body = json!({
        "action": "submitInterview",
        "payload": {
            "candidateName": payload.candidate_name,
            "interviewStage": payload.interview_stage,
            "location": payload.location,
            "interviewPlatform": payload.interview_platform,
            "meetingDate": payload.meeting_date,
            "meetingTime": payload.meeting_time,
            "meetingDuration": payload.meeting_duration,
            "panel": payload.panel,
            "client": payload.client,
            "vendor": payload.vendor,
            "jobDescription": payload.job_description,
            "specialNote": payload.special_note,
            "resumeBase64": base64::Engine::encode(&base64::engine::general_purpose::STANDARD, resume),
            "resumeFilename": payload.resume_filename,
            "resumeContentType": payload.resume_content_type,
            "skipThankYouEmail": true
        }
    });
    let mut last = anyhow::anyhow!("Submission failed");
    for url in urls {
        match state
            .google
            .http()
            .post(url)
            .header("content-type", "text/plain;charset=utf-8")
            .body(body.to_string())
            .send()
            .await
        {
            Ok(res) => {
                let text = res.text().await.unwrap_or_default();
                if let Ok(data) = serde_json::from_str::<serde_json::Value>(&text) {
                    if data.get("ok").and_then(|v| v.as_bool()) == Some(true) {
                        return Ok(data.get("result").cloned().unwrap_or(json!({ "success": true })));
                    }
                    last = anyhow::anyhow!("{}", data.get("error").and_then(|v| v.as_str()).unwrap_or("Submission failed"));
                } else {
                    last = anyhow::anyhow!("Booking server returned an unexpected response. Try again.");
                }
            }
            Err(err) => last = err.into(),
        }
    }
    Err(last)
}

pub async fn book_from_website(state: &AppState, payload: &QueuedPayload, resume: &[u8]) -> anyhow::Result<serde_json::Value> {
    let profile = lookup_candidate_profile(state, &payload.candidate_name).await?;
    let mut result = if state.cfg.booking_use_apps_script {
        submit_apps_script(state, payload, resume).await?
    } else {
        process_native_booking(state, payload, resume).await?
    };
    let mut email_sent = false;
    let mut submitter = profile.email.clone();
    if !profile.email.is_empty() {
        match send_thank_you(state, payload, &profile.poc, &profile.email).await {
            Ok((sent, to)) => {
                email_sent = sent;
                if !to.is_empty() {
                    submitter = to;
                }
            }
            Err(err) => tracing::error!("[booking] site thank-you email failed: {err}"),
        }
    } else {
        tracing::warn!(
            "[booking] no Current_Market email for candidate \"{}\" — thank-you not sent",
            payload.candidate_name
        );
    }
    if let Some(obj) = result.as_object_mut() {
        obj.insert("emailSent".into(), json!(email_sent));
        obj.insert("submitterEmail".into(), json!(submitter));
    }
    Ok(result)
}

fn queue_dir(state: &AppState) -> PathBuf {
    state.cfg.data_dir.join("booking-queue")
}

pub(crate) async fn queued_slot_intervals(state: &AppState, kind: SheetKind) -> Vec<(String, i32, i32)> {
    let metas = if state.db.is_connected() {
        match state.db.list_booking_jobs().await {
            Ok(jobs) => jobs
                .into_iter()
                .filter_map(|(id, v, _)| {
                    let mut meta: JobMeta = serde_json::from_value(v).ok()?;
                    if meta.id.is_empty() {
                        meta.id = id;
                    }
                    Some(meta)
                })
                .collect(),
            Err(_) => Vec::new(),
        }
    } else {
        let root = queue_dir(state);
        let mut rd = match tokio::fs::read_dir(&root).await {
            Ok(rd) => rd,
            Err(_) => return Vec::new(),
        };
        let mut metas = Vec::new();
        while let Ok(Some(entry)) = rd.next_entry().await {
            if !entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let raw = match tokio::fs::read(entry.path().join("meta.json")).await {
                Ok(b) => b,
                Err(_) => continue,
            };
            if let Ok(meta) = serde_json::from_slice::<JobMeta>(&raw) {
                metas.push(meta);
            }
        }
        metas
    };
    let mut out = Vec::new();
    for meta in metas {
        if meta.status != "queued" && meta.status != "processing" {
            continue;
        }
        let is_phone = is_phone_call_stage(&meta.payload.interview_stage);
        let job_kind = if is_phone {
            SheetKind::Phone
        } else {
            SheetKind::Interview
        };
        if job_kind != kind {
            continue;
        }
        let date_key = format_mdy(&parse_date_key(&meta.payload.meeting_date));
        if date_key.is_empty() {
            continue;
        }
        let Some(start_min) = parse_time_minutes(&meta.payload.meeting_time) else {
            continue;
        };
        out.push((
            date_key,
            start_min,
            crate::availability::regular_slot_end(kind, start_min),
        ));
    }
    out
}

async fn enqueue_job(state: &AppState, payload: QueuedPayload, resume: &[u8]) -> anyhow::Result<String> {
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let meta = JobMeta {
        id: id.clone(),
        status: "queued".into(),
        attempts: 0,
        created_at: now.clone(),
        updated_at: now,
        error: None,
        payload,
    };
    if state.db.is_connected() {
        let val = serde_json::to_value(&meta)?;
        state.db.enqueue_booking(&id, &val, resume).await?;
        return Ok(id);
    }
    let dir = queue_dir(state).join(&id);
    tokio::fs::create_dir_all(&dir).await?;
    tokio::fs::write(dir.join("resume.bin"), resume).await?;
    tokio::fs::write(dir.join("meta.json"), serde_json::to_vec_pretty(&meta)?).await?;
    Ok(id)
}

pub async fn drain_booking_queue(state: &AppState) -> anyhow::Result<serde_json::Value> {
    if state
        .queue
        .draining
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        state.queue.pending_kick.store(true, Ordering::SeqCst);
        return Ok(json!({ "done": 0, "failed": 0, "skipped": 0 }));
    }
    let result = async {
        let mut last = json!({ "done": 0, "failed": 0, "skipped": 0 });
        loop {
            last = drain_once(state).await?;
            if !state.queue.pending_kick.swap(false, Ordering::SeqCst) {
                break;
            }
        }
        Ok::<_, anyhow::Error>(last)
    }
    .await;
    state.queue.draining.store(false, Ordering::SeqCst);
    result
}

async fn drain_once(state: &AppState) -> anyhow::Result<serde_json::Value> {
    let mut done = 0;
    let mut failed = 0;
    let mut skipped = 0;
    if state.db.is_connected() {
        let jobs = state.db.list_booking_jobs().await?;
        for (id, val, resume) in jobs {
            match process_one_mongo(state, &id, val, resume).await? {
                "done" => done += 1,
                "failed" => failed += 1,
                _ => skipped += 1,
            }
        }
        return Ok(json!({ "done": done, "failed": failed, "skipped": skipped }));
    }
    let root = queue_dir(state);
    let mut ids = match tokio::fs::read_dir(&root).await {
        Ok(rd) => {
            let mut out = Vec::new();
            let mut rd = rd;
            while let Some(e) = rd.next_entry().await? {
                if e.file_name().to_string_lossy().starts_with('.') {
                    continue;
                }
                if e.file_type().await?.is_dir() {
                    out.push(e.file_name().to_string_lossy().into_owned());
                }
            }
            out
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(json!({ "done": 0, "failed": 0, "skipped": 0 }));
        }
        Err(err) => return Err(err.into()),
    };
    ids.sort();
    for id in ids {
        match process_one(state, &id).await? {
            "done" => done += 1,
            "failed" => failed += 1,
            _ => skipped += 1,
        }
    }
    Ok(json!({ "done": done, "failed": failed, "skipped": skipped }))
}

async fn process_one_mongo(
    state: &AppState,
    id: &str,
    val: serde_json::Value,
    resume: Vec<u8>,
) -> anyhow::Result<&'static str> {
    let mut meta: JobMeta = serde_json::from_value(val)?;
    if meta.id.is_empty() {
        meta.id = id.to_string();
    }
    let stale_ms = 10 * 60 * 1000;
    let age = chrono::Utc::now()
        .signed_duration_since(
            chrono::DateTime::parse_from_rfc3339(&meta.updated_at).unwrap_or(chrono::Utc::now().into()),
        )
        .num_milliseconds();
    let claimable = meta.status == "queued"
        || (meta.status == "failed" && meta.attempts < 5)
        || (meta.status == "processing" && age > stale_ms);
    if !claimable {
        return Ok("skipped");
    }
    meta.status = "processing".into();
    meta.attempts += 1;
    meta.updated_at = chrono::Utc::now().to_rfc3339();
    let val = serde_json::to_value(&meta)?;
    state.db.save_booking_job(id, &val, None).await?;
    let result = async {
        if resume.is_empty() {
            anyhow::bail!("Queued resume file is empty.");
        }
        book_from_website(state, &meta.payload, &resume).await?;
        Ok::<_, anyhow::Error>(())
    }
    .await;
    match result {
        Ok(()) => {
            state.db.delete_booking_job(id).await?;
            tracing::info!("[booking-queue] done {id}");
            Ok("done")
        }
        Err(err) => {
            meta.error = Some(err.to_string());
            meta.status = "failed".into();
            meta.updated_at = chrono::Utc::now().to_rfc3339();
            let val = serde_json::to_value(&meta)?;
            state.db.save_booking_job(id, &val, None).await?;
            tracing::error!("[booking-queue] failed {id} (attempt {}): {err}", meta.attempts);
            Ok("failed")
        }
    }
}

async fn process_one(state: &AppState, id: &str) -> anyhow::Result<&'static str> {
    let dir = queue_dir(state).join(id);
    let meta_path = dir.join("meta.json");
    let raw = match tokio::fs::read(&meta_path).await {
        Ok(b) => b,
        Err(_) => return Ok("skipped"),
    };
    let mut meta: JobMeta = serde_json::from_slice(&raw)?;
    let stale_ms = 10 * 60 * 1000;
    let age = chrono::Utc::now()
        .signed_duration_since(chrono::DateTime::parse_from_rfc3339(&meta.updated_at).unwrap_or(chrono::Utc::now().into()))
        .num_milliseconds();
    let claimable = meta.status == "queued"
        || (meta.status == "failed" && meta.attempts < 5)
        || (meta.status == "processing" && age > stale_ms);
    if !claimable {
        return Ok("skipped");
    }
    let lock_path = dir.join(".lock");
    if tokio::fs::write(&lock_path, b"1").await.is_err() {
        if let Ok(st) = tokio::fs::metadata(&lock_path).await {
            let modified = st.modified().ok();
            let old = modified
                .and_then(|t| t.elapsed().ok())
                .map(|d| d.as_millis() > stale_ms as u128)
                .unwrap_or(false);
            if old {
                let _ = tokio::fs::remove_file(&lock_path).await;
                if tokio::fs::write(&lock_path, b"1").await.is_err() {
                    return Ok("skipped");
                }
            } else {
                return Ok("skipped");
            }
        } else {
            return Ok("skipped");
        }
    }
    meta.status = "processing".into();
    meta.attempts += 1;
    meta.updated_at = chrono::Utc::now().to_rfc3339();
    tokio::fs::write(&meta_path, serde_json::to_vec_pretty(&meta)?).await?;
    let result = async {
        let resume = tokio::fs::read(dir.join("resume.bin")).await?;
        if resume.is_empty() {
            anyhow::bail!("Queued resume file is empty.");
        }
        book_from_website(state, &meta.payload, &resume).await?;
        Ok::<_, anyhow::Error>(())
    }
    .await;
    let _ = tokio::fs::remove_file(&lock_path).await;
    match result {
        Ok(()) => {
            let _ = tokio::fs::remove_dir_all(&dir).await;
            tracing::info!("[booking-queue] done {id}");
            Ok("done")
        }
        Err(err) => {
            meta.error = Some(err.to_string());
            meta.status = "failed".into();
            meta.updated_at = chrono::Utc::now().to_rfc3339();
            tokio::fs::write(&meta_path, serde_json::to_vec_pretty(&meta)?).await?;
            tracing::error!("[booking-queue] failed {id} (attempt {}): {err}", meta.attempts);
            Ok("failed")
        }
    }
}

pub fn kick_booking_worker(state: &AppState) {
    let state = state.clone();
    tokio::task::spawn_blocking(move || {
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            if let Err(err) = handle.block_on(drain_booking_queue(&state)) {
                tracing::error!("[booking-queue] drain error: {err}");
            }
        }
    });
}

pub async fn enqueue_website_booking(
    state: &AppState,
    payload: QueuedPayload,
    resume: Vec<u8>,
) -> anyhow::Result<serde_json::Value> {
    if payload.candidate_name.trim().is_empty() {
        anyhow::bail!("Candidate Name is required.");
    }
    if payload.client.trim().is_empty() {
        anyhow::bail!("Client is required.");
    }
    if payload.interview_stage.trim().is_empty() {
        anyhow::bail!("Interview Stage is required.");
    }
    if resume.is_empty() {
        anyhow::bail!("Resume upload is required.");
    }
    if payload.job_description.trim().is_empty() {
        anyhow::bail!("Job Description is required.");
    }
    let profile = lookup_candidate_profile(state, &payload.candidate_name).await?;
    if profile.poc.is_empty() {
        anyhow::bail!(
            "Nepal POC is missing for \"{}\" on Current_Market. Add POC and retry.",
            payload.candidate_name
        );
    }
    let job_id = enqueue_job(state, payload.clone(), &resume).await?;
    tracing::info!(
        "[booking-queue] queued {job_id} ({} / {})",
        payload.candidate_name,
        payload.client
    );
    kick_booking_worker(state);
    Ok(json!({
        "success": true,
        "queued": true,
        "jobId": job_id,
        "candidateName": payload.candidate_name,
        "companyName": payload.client,
        "folderPath": format!("Resumes_Data/{}/{}/{}", payload.candidate_name, payload.client, payload.interview_stage),
        "submitterEmail": profile.email
    }))
}

#[derive(Default)]
struct BookingFormExtras {
    hold_id: String,
    emergency: bool,
    time_zone: String,
}

fn parse_boolish(value: &str) -> bool {
    matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")
}

async fn parse_booking_form(mut form: Multipart) -> anyhow::Result<(QueuedPayload, Vec<u8>, BookingFormExtras)> {
    let mut payload = QueuedPayload {
        candidate_name: String::new(),
        interview_stage: String::new(),
        location: String::new(),
        interview_platform: String::new(),
        meeting_date: String::new(),
        meeting_time: String::new(),
        meeting_duration: String::new(),
        panel: String::new(),
        client: String::new(),
        vendor: String::new(),
        job_description: String::new(),
        special_note: String::new(),
        resume_filename: "resume.pdf".into(),
        resume_content_type: "application/octet-stream".into(),
        skip_thank_you_email: true,
    };
    let mut resume = Vec::new();
    let mut extras = BookingFormExtras::default();
    while let Some(field) = form.next_field().await? {
        let name = field.name().unwrap_or("").to_string();
        if name == "resume" {
            let filename = field.file_name().unwrap_or("resume.pdf").to_string();
            let ctype = field.content_type().unwrap_or("application/octet-stream").to_string();
            let bytes = field.bytes().await?;
            if bytes.is_empty() {
                anyhow::bail!("Resume upload is required.");
            }
            if bytes.len() > MAX_RESUME_BYTES {
                anyhow::bail!("Resume is too large. Use a PDF under 5 MB.");
            }
            payload.resume_filename = filename;
            payload.resume_content_type = ctype;
            resume = bytes.to_vec();
        } else {
            let text = field.text().await.unwrap_or_default().trim().to_string();
            match name.as_str() {
                "candidateName" => payload.candidate_name = text,
                "interviewStage" => payload.interview_stage = text,
                "location" => payload.location = text,
                "interviewPlatform" => payload.interview_platform = text,
                "meetingDate" => payload.meeting_date = text,
                "meetingTime" => payload.meeting_time = text,
                "meetingDuration" => payload.meeting_duration = text,
                "panel" => payload.panel = text,
                "client" => payload.client = text,
                "vendor" => payload.vendor = text,
                "jobDescription" => payload.job_description = text,
                "specialNote" => payload.special_note = text,
                "holdId" => extras.hold_id = text,
                "emergency" => extras.emergency = parse_boolish(&text),
                "timeZone" => extras.time_zone = text,
                _ => {}
            }
        }
    }
    if resume.is_empty() {
        anyhow::bail!("Resume upload is required.");
    }
    Ok((payload, resume, extras))
}

fn required(v: &str, label: &str) -> anyhow::Result<String> {
    if v.trim().is_empty() {
        anyhow::bail!("{label} is required.");
    }
    Ok(v.trim().to_string())
}

async fn handle_book(
    state: AppState,
    headers: HeaderMap,
    form: Multipart,
    force_stage: Option<&str>,
) -> Result<impl IntoResponse, AppError> {
    let user = require_session(&state.cfg, &headers)?;
    let (mut payload, resume, extras) = parse_booking_form(form).await.map_err(booking_error)?;
    if let Some(stage) = force_stage {
        payload.interview_stage = stage.into();
    } else {
        payload.interview_stage = required(&payload.interview_stage, "Interview Stage").map_err(booking_error)?;
        if !STAGES.contains(&payload.interview_stage.as_str()) {
            return Err(AppError::BadRequest("Select a valid interview stage.".into()));
        }
    }
    payload.candidate_name = required(&payload.candidate_name, "Candidate Name").map_err(booking_error)?;
    payload.location = required(&payload.location, "Location").map_err(booking_error)?;
    payload.interview_platform = required(&payload.interview_platform, "Mode of Interview").map_err(booking_error)?;
    payload.meeting_date = required(&payload.meeting_date, "Meeting Date").map_err(booking_error)?;
    payload.meeting_time = required(&payload.meeting_time, "Meeting Time").map_err(booking_error)?;
    payload.meeting_time = normalize_meeting_time_cst(&payload.meeting_time, &extras.time_zone);
    payload.meeting_duration = required(&payload.meeting_duration, "Meeting Duration").map_err(booking_error)?;
    payload.panel = required(&payload.panel, "Panel").map_err(booking_error)?;
    payload.client = required(&payload.client, "Client").map_err(booking_error)?;
    payload.job_description = required(&payload.job_description, "Job Description").map_err(booking_error)?;
    payload.resume_filename = required(&payload.resume_filename, "Resume filename").map_err(booking_error)?;

    let kind = if force_stage.is_some() {
        SheetKind::Phone
    } else {
        SheetKind::Interview
    };
    if extras.emergency {
        // Emergency = skip slot holds / capacity. Team Accepts or Declines on the
        // sheet. Do NOT reject when the chosen time also happens to land on a
        // regular grid hour — that blocked real urgent bookings (users pick
        // 10:00 AM in the emergency form and got "book it from the grid").
        let result = enqueue_website_booking(&state, payload, resume)
            .await
            .map_err(booking_error)?;
        return Ok(Json(json!({ "ok": true, "result": result })));
    } else {
        // Preferred path: a live in-memory hold from the grid.
        let live_hold = if extras.hold_id.trim().is_empty() {
            None
        } else {
            state
                .holds
                .peek(extras.hold_id.trim(), &user.email, kind)
                .await
                .ok()
        };

        if let Some(peeked) = live_hold {
            let external = sheet_and_queue_intervals(&state, kind, &peeked.date_key)
                .await
                .map_err(AppError::from)?;
            let hold = state
                .holds
                .verify_for_commit(
                    extras.hold_id.trim(),
                    &user.email,
                    kind,
                    &external,
                    meeting_capacity(kind),
                )
                .await?;
            let requested_start_min = parse_time_minutes(&payload.meeting_time);
            if requested_start_min == Some(hold.start_min) {
                payload.meeting_date = hold.date_iso.clone();
                payload.meeting_time = format_time_display(hold.start_min);
                // Keep the meeting's real duration from the form — the hold only
                // covers one grid cell, not the whole meeting.
                let result = enqueue_website_booking(&state, payload, resume)
                    .await
                    .map_err(booking_error)?;
                state.holds.consume(&hold.id, &user.email).await;
                return Ok(Json(json!({ "ok": true, "result": result })));
            } else {
                // Hold was for a different time than requested — consume the stale hold
                // and proceed to direct capacity check for the requested time below.
                state.holds.consume(&hold.id, &user.email).await;
            }
        }

        // Hold lost (server restart, race, flaky client) — don't bounce a real
        // booking. Re-check capacity straight from the sheet + queue for the
        // submitted slot and proceed if there's still room.
        let date = chrono::NaiveDate::parse_from_str(payload.meeting_date.trim(), "%Y-%m-%d")
            .map_err(|_| AppError::BadRequest("Meeting date is not valid.".into()))?;
        let date_key = crate::dates::format_date_key_mdy(date);
        let start_min = parse_time_minutes(&payload.meeting_time)
            .ok_or_else(|| AppError::BadRequest("Meeting time is not valid.".into()))?;
        let external = sheet_and_queue_intervals(&state, kind, &date_key)
            .await
            .map_err(AppError::from)?;
        // Capacity check covers one grid cell only, like a hold would.
        let peak = crate::availability::peak_occupancy(
            &external,
            &date_key,
            start_min,
            crate::availability::regular_slot_end(kind, start_min),
        );
        if peak >= meeting_capacity(kind) {
            return Err(AppError::Conflict(
                "This time is fully booked. Pick another slot.".into(),
            ));
        }
        payload.meeting_time = format_time_display(start_min);
        let result = enqueue_website_booking(&state, payload, resume)
            .await
            .map_err(booking_error)?;
        return Ok(Json(json!({ "ok": true, "result": result })));
    }
}

pub async fn appointment_book(
    State(state): State<AppState>,
    headers: HeaderMap,
    form: Multipart,
) -> Result<impl IntoResponse, AppError> {
    handle_book(state, headers, form, None).await
}

pub async fn phone_call_book(
    State(state): State<AppState>,
    headers: HeaderMap,
    form: Multipart,
) -> Result<impl IntoResponse, AppError> {
    handle_book(state, headers, form, Some("Phone call")).await
}
