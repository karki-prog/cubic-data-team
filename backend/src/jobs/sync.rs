use crate::booking::lookup_candidate_profile;
use crate::dates::{combine_date_time_iso, duration_minutes, format_mdy, format_time_display, parse_date_key, parse_time_minutes};
use crate::google::calendar::CalendarEventInput;
use crate::staff::resolve_support_email;
use crate::types::{record_fingerprint, BackendState, BookingRecord, SheetKind};
use crate::AppState;

fn esc(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn line(label: &str, value: &str) -> String {
    let text = value.trim();
    if text.is_empty() || text == "—" {
        String::new()
    } else {
        format!("<div><b>{}:</b> {}</div>", esc(label), esc(text))
    }
}

fn description(record: &BookingRecord) -> String {
    let mut html = format!(
        "<div><b>Phone call details</b></div><div><b>Candidate:</b> {}</div><div><b>Client:</b> {}</div>",
        esc(&record.candidate_name),
        esc(&record.client)
    );
    html.push_str(&line("Date", &record.meeting_date));
    html.push_str(&line("Time (CST)", &record.meeting_time));
    html.push_str(&line("Duration", &record.meeting_duration));
    html.push_str(&line("Location", &record.location));
    html.push_str(&line("POC", &record.poc));
    html.push_str(&line("Support", &record.support));
    html.push_str(&line("Panel", &record.panel));
    html.push_str(&line("Tech", &record.tech));
    html.push_str(&line("Visa", &record.visa));
    let mut links = Vec::new();
    if !record.resume_link.is_empty() {
        links.push(format!("<a href=\"{}\">Resume</a>", esc(&record.resume_link)));
    }
    if !record.jd_link.is_empty() {
        links.push(format!("<a href=\"{}\">Job Description</a>", esc(&record.jd_link)));
    }
    if !record.folder_link.is_empty() {
        links.push(format!("<a href=\"{}\">Folder</a>", esc(&record.folder_link)));
    }
    if !links.is_empty() {
        html.push_str(&format!(
            "<div><b>Drive</b></div><div>{}</div>",
            links.join(" &nbsp;|&nbsp; ")
        ));
    }
    html
}

async fn sync_phone_call_calendar(
    state: &AppState,
    record: &BookingRecord,
    existing_event_id: &str,
) -> anyhow::Result<(String, bool)> {
    if record.status != "Accepted" {
        if !existing_event_id.is_empty() {
            state.google.delete_calendar_event(existing_event_id).await;
            return Ok((String::new(), true));
        }
        return Ok((String::new(), false));
    }
    let date_key = if record.date_key.is_empty() {
        parse_date_key(&record.meeting_date)
    } else {
        record.date_key.clone()
    };
    let start_iso = combine_date_time_iso(&date_key, &record.meeting_time);
    if start_iso.is_empty() {
        anyhow::bail!("Phone-call date/time could not be parsed.");
    }
    let end_mins = duration_minutes(&record.meeting_duration);
    let (date_part, time_part) = start_iso.split_once('T').unwrap_or(("", ""));
    let hm: Vec<i32> = time_part.split(':').filter_map(|n| n.parse().ok()).collect();
    let h = hm.first().copied().unwrap_or(0);
    let m = hm.get(1).copied().unwrap_or(0);
    let end_total = h * 60 + m + end_mins;
    let end_iso = format!(
        "{date_part}T{:02}:{:02}:00",
        (end_total / 60) % 24,
        end_total % 60
    );
    let mut attendees = state.cfg.calendar_always_guests.clone();
    let support = resolve_support_email(&state.cfg, &record.support);
    if !support.is_empty() {
        attendees.push(support);
    }
    attendees.retain(|e| e != &state.cfg.calendar_owner_email);
    let mut attachments = Vec::new();
    if !record.resume_link.is_empty() {
        attachments.push((record.resume_link.clone(), "Resume".into()));
    }
    if !record.jd_link.is_empty() {
        attachments.push((record.jd_link.clone(), "Job Description".into()));
    }
    let event_id = state
        .google
        .upsert_calendar_event(CalendarEventInput {
            title: format!(
                "Phone call: {} — {}",
                if record.candidate_name.is_empty() {
                    "Candidate"
                } else {
                    &record.candidate_name
                },
                if record.client.is_empty() {
                    "Client"
                } else {
                    &record.client
                }
            ),
            description_html: description(record),
            start_iso,
            end_iso,
            attendees,
            attachments,
            existing_event_id: existing_event_id.to_string(),
        })
        .await?;
    Ok((event_id, false))
}

fn row_matches(headers: &crate::headers::SheetHeaders, existing: &[String], record: &BookingRecord) -> bool {
    let candidate = headers.get(existing, crate::headers::col::CANDIDATE).to_lowercase();
    let client = headers.get(existing, crate::headers::col::CLIENT).to_lowercase();
    let date_key = parse_date_key(headers.get(existing, crate::headers::col::DATE));
    let time = headers.get(existing, crate::headers::col::TIME).to_lowercase();
    candidate == record.candidate_name.trim().to_lowercase()
        && client == record.client.trim().to_lowercase()
        && date_key == record.date_key
        && time == record.meeting_time.trim().to_lowercase()
}

async fn sync_accepted_interview_to_cubic(state: &AppState, record: &BookingRecord) -> anyhow::Result<(bool, Option<i32>)> {
    if record.kind == SheetKind::Phone {
        return Ok((false, None));
    }
    let spreadsheet_id = &state.cfg.connector_spreadsheet_id;
    let sheet_name = &state.cfg.cubic_tab;
    let start = state.cfg.connector_data_start_row;
    let headers = state.google.load_headers(spreadsheet_id, sheet_name).await?;
    let last = crate::headers::a1_col(
        headers
            .idx(crate::headers::col::NOTE)
            .or(headers.idx(crate::headers::col::PANEL))
            .unwrap_or_else(|| headers.last_named_idx()),
    );
    let rows = state
        .google
        .sheets_values_get(spreadsheet_id, &format!("'{sheet_name}'!A{start}:{last}"), None)
        .await?;
    let match_index = rows.iter().position(|row| row_matches(&headers, row, record));
    let mins = parse_time_minutes(&record.meeting_time);
    let time = mins
        .map(format_time_display)
        .unwrap_or_else(|| record.meeting_time.clone());
    let date = {
        let md = format_mdy(&record.date_key);
        if md.is_empty() {
            record.meeting_date.clone()
        } else {
            md
        }
    };
    let tech = if record.tech.is_empty() { "Data" } else { record.tech.as_str() };
    let pairs: Vec<(usize, serde_json::Value)> = [
        headers.pair(crate::headers::col::POC, serde_json::json!(record.poc)),
        headers.pair(crate::headers::col::CLIENT, serde_json::json!(record.client)),
        headers.pair(crate::headers::col::TECH, serde_json::json!(tech)),
        headers.pair(crate::headers::col::LOCATION, serde_json::json!(record.location)),
        headers.pair(crate::headers::col::VISA, serde_json::json!(record.visa)),
        headers.pair(crate::headers::col::CANDIDATE, serde_json::json!(record.candidate_name)),
        headers.pair(crate::headers::col::DATE, serde_json::json!(date)),
        headers.pair(crate::headers::col::TIME, serde_json::json!(time)),
        headers.pair(crate::headers::col::DURATION, serde_json::json!(record.meeting_duration)),
        headers.pair(crate::headers::col::PANEL, serde_json::json!(record.panel)),
        headers.pair(crate::headers::col::NOTE, serde_json::json!(record.special_note)),
    ]
    .into_iter()
    .flatten()
    .collect();
    if pairs.is_empty() {
        anyhow::bail!("COPY_TO_CUBIC_SHEET has no matching header names to write");
    }
    let (min, max, values) = crate::headers::sparse_row(&pairs);
    if let Some(idx) = match_index {
        let row = start + idx as i32;
        state
            .google
            .sheets_values_update(
                spreadsheet_id,
                &headers.range_row(sheet_name, row, min, max),
                vec![values.clone()],
                "USER_ENTERED",
            )
            .await?;
        return Ok((true, Some(row)));
    }
    state
        .google
        .insert_row_at(spreadsheet_id, sheet_name, start - 1)
        .await?;
    state
        .google
        .sheets_values_update(
            spreadsheet_id,
            &headers.range_row(sheet_name, start, min, max),
            vec![values],
            "USER_ENTERED",
        )
        .await?;
    Ok((true, Some(start)))
}

async fn enrich_visa(state: &AppState, mut record: BookingRecord) -> BookingRecord {
    if !record.visa.is_empty() && !record.submitter_email.is_empty() {
        return record;
    }
    if let Ok(profile) = lookup_candidate_profile(state, &record.candidate_name).await {
        if record.visa.is_empty() {
            record.visa = profile.visa;
        }
        if record.submitter_email.is_empty() {
            record.submitter_email = profile.email;
        }
        if record.poc.is_empty() {
            record.poc = profile.poc;
        }
        if record.tech.is_empty() {
            record.tech = if profile.tech.is_empty() {
                "Data".into()
            } else {
                profile.tech
            };
        }
    }
    record
}

pub async fn sync_sheet_status_to_google(state: &AppState) -> anyhow::Result<serde_json::Value> {
    let phone_rows = super::readers::read_phone_call_records(state).await?;
    let interview_rows = super::readers::read_interview_records(state).await?;
    let mut map = super::state::load_backend_state(state).await?;
    let mut scanned = 0;
    let mut calendars = 0;
    let mut cubic = 0;
    let mut errors = Vec::new();

    for raw in phone_rows.into_iter().chain(interview_rows) {
        scanned += 1;
        let record = enrich_visa(state, raw).await;
        let key = record_fingerprint(
            &record.sheet_name,
            &record.candidate_name,
            &record.client,
            &record.date_key,
            &record.meeting_time,
            &record.interview_stage,
        );
        if !map.contains_key(&key) {
            let email_status = if record.status == "Accepted" || record.status == "Declined" {
                record.status.clone()
            } else {
                String::new()
            };
            map.insert(
                key.clone(),
                BackendState {
                    key,
                    kind: record.kind,
                    status: record.status.clone(),
                    calendar_event_id: String::new(),
                    email_status,
                    cubic_synced: record.kind == SheetKind::Interview && record.status == "Accepted",
                },
            );
            continue;
        }
        let mut prev = map.get(&key).cloned().unwrap();
        if (record.status == "Accepted" || record.status == "Declined")
            && prev.email_status != record.status
            && prev.status != record.status
        {
            prev.email_status = record.status.clone();
        }
        // Phone-call calendar events are owned by the Apps Script
        // `phoneCallCalendar.gs` bound to the Data Interview sheet. Running this
        // writer too puts a duplicate event on the calendar for every booking.
        // Opt back in with RUST_PHONE_CALENDAR=1 only if that script is retired.
        if record.kind == SheetKind::Phone && state.cfg.rust_phone_calendar {
            match sync_phone_call_calendar(state, &record, &prev.calendar_event_id).await {
                Ok((event_id, removed)) => {
                    if (!event_id.is_empty() && event_id != prev.calendar_event_id) || removed {
                        calendars += 1;
                    }
                    prev.calendar_event_id = event_id;
                }
                Err(err) => errors.push(format!("calendar {}: {err}", record.candidate_name)),
            }
        }
        if record.kind == SheetKind::Interview && record.status == "Accepted" && !prev.cubic_synced {
            match sync_accepted_interview_to_cubic(state, &record).await {
                Ok((synced, _)) => {
                    if synced {
                        prev.cubic_synced = true;
                        cubic += 1;
                    }
                }
                Err(err) => errors.push(format!("cubic {}: {err}", record.candidate_name)),
            }
        }
        prev.status = record.status;
        prev.kind = record.kind;
        map.insert(key, prev);
    }
    super::state::save_backend_state(state, &map).await?;
    Ok(serde_json::json!({
        "scanned": scanned,
        "emails": 0,
        "calendars": calendars,
        "cubic": cubic,
        "errors": errors
    }))
}
