use crate::dates::{normalize_status, parse_date_key};
use chrono::Datelike;

use crate::types::{BookingRecord, HistoryRecord, SheetKind};
use crate::AppState;

fn normalize_doc_url(raw: &str) -> String {
    let text = raw.trim().trim_matches('"').trim_matches('\'').trim();
    if text.is_empty() || text == "—" {
        return String::new();
    }
    if let Some(caps) = regex::Regex::new(r#"(?i)HYPERLINK\s*\(\s*"([^"]+)""#)
        .ok()
        .and_then(|re| re.captures(text))
        .or_else(|| {
            regex::Regex::new(r"(?i)HYPERLINK\s*\(\s*'([^']+)'")
                .ok()
                .and_then(|re| re.captures(text))
        })
    {
        return normalize_doc_url(&caps[1]);
    }
    let lower = text.to_lowercase();
    if lower == "resume"
        || lower == "jd"
        || lower.starts_with("resume link")
        || lower.starts_with("job description")
        || lower.starts_with("drive folder")
    {
        return String::new();
    }
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return text.to_string();
    }
    if lower.starts_with("www.")
        || lower.starts_with("drive.google.com")
        || lower.starts_with("docs.google.com")
    {
        return format!("https://{text}");
    }
    String::new()
}

fn first_url(parts: &[&str]) -> String {
    for part in parts {
        let url = normalize_doc_url(part);
        if !url.is_empty() {
            return url;
        }
    }
    String::new()
}

fn formula_cell(formulas: &[Vec<String>], row_idx: usize, col: usize) -> String {
    formulas
        .get(row_idx)
        .and_then(|r| r.get(col))
        .cloned()
        .unwrap_or_default()
}

fn formula_at(formulas: &[Vec<String>], row_idx: usize, col: Option<usize>, off: usize) -> String {
    col.map(|c| formula_cell(formulas, row_idx, c.saturating_sub(off)))
        .unwrap_or_default()
}

pub async fn read_phone_call_records(state: &AppState) -> anyhow::Result<Vec<BookingRecord>> {
    let rows = state
        .google
        .sheets_values_get(
            &state.cfg.data_interview_spreadsheet_id,
            &format!("'{}'!A1:AZ", state.cfg.phone_calls_sheet),
            None,
        )
        .await?;
    if rows.len() < 2 {
        return Ok(Vec::new());
    }
    let headers = crate::headers::SheetHeaders::parse(&rows[0]);
    let jd_i = headers.idx(crate::headers::col::JD);
    let resume_i = headers.idx(crate::headers::col::RESUME);
    let folder_i = headers.idx(crate::headers::col::FOLDER);
    let formula_off = [jd_i, resume_i, folder_i]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(0);
    let formula_end = [jd_i, resume_i, folder_i]
        .into_iter()
        .flatten()
        .max()
        .unwrap_or(formula_off);
    let formulas = state
        .google
        .sheets_values_get(
            &state.cfg.data_interview_spreadsheet_id,
            &format!(
                "'{}'!{}1:{}",
                state.cfg.phone_calls_sheet,
                crate::headers::a1_col(formula_off),
                crate::headers::a1_col(formula_end)
            ),
            Some("FORMULA"),
        )
        .await
        .unwrap_or_default();
    let hidden = headers.hidden_run(3);
    let hidden_at = |row: &[String], n: usize| {
        hidden
            .get(n)
            .and_then(|c| row.get(*c))
            .cloned()
            .unwrap_or_default()
    };

    let mut records = Vec::new();
    for (i, row) in rows.iter().enumerate().skip(1) {
        let candidate = headers.get_owned(row, crate::headers::col::CANDIDATE);
        let client = headers.get_owned(row, crate::headers::col::CLIENT);
        if candidate.is_empty() && client.is_empty() {
            continue;
        }
        let meeting_date = headers.get_owned(row, crate::headers::col::DATE);
        let notes = headers.get_owned(row, crate::headers::col::NOTE);
        let feedback = headers.get_owned(row, crate::headers::col::FEEDBACK);
        records.push(BookingRecord {
            kind: SheetKind::Phone,
            sheet_name: state.cfg.phone_calls_sheet.clone(),
            row: i as i32 + 1,
            candidate_name: candidate,
            client,
            interview_stage: "Phone call".into(),
            location: headers.get_owned(row, crate::headers::col::LOCATION),
            interview_platform: String::new(),
            poc: headers.get_owned(row, crate::headers::col::POC),
            meeting_date: meeting_date.clone(),
            meeting_time: headers.get_owned(row, crate::headers::col::TIME),
            meeting_duration: headers.get_owned(row, crate::headers::col::DURATION),
            panel: headers.get_owned(row, crate::headers::col::PANEL),
            vendor: String::new(),
            support: headers.get_owned(row, crate::headers::col::SUPPORT),
            tech: {
                let t = headers.get_owned(row, crate::headers::col::TECH);
                if t.is_empty() {
                    "Data".into()
                } else {
                    t
                }
            },
            visa: headers.get_owned(row, crate::headers::col::VISA),
            status: normalize_status(&headers.get_owned(row, crate::headers::col::STATUS)),
            special_note: if notes.is_empty() { feedback } else { notes },
            // Visible link first. The hidden URL copy can be left behind when a row
            // moves, and preferring it sent candidates each other's files.
            resume_link: first_url(&[
                &formula_at(&formulas, i, resume_i, formula_off),
                &headers.get_owned(row, crate::headers::col::RESUME),
                &hidden_at(row, 1),
            ]),
            jd_link: first_url(&[
                &formula_at(&formulas, i, jd_i, formula_off),
                &headers.get_owned(row, crate::headers::col::JD),
                &hidden_at(row, 0),
            ]),
            folder_link: first_url(&[
                &formula_at(&formulas, i, folder_i, formula_off),
                &headers.get_owned(row, crate::headers::col::FOLDER),
                &hidden_at(row, 2),
            ]),
            submitter_email: headers.get_owned(row, crate::headers::col::EMAIL),
            date_key: parse_date_key(&meeting_date),
        });
    }
    Ok(records)
}

pub async fn read_interview_records(state: &AppState) -> anyhow::Result<Vec<BookingRecord>> {
    let start = state.cfg.connector_data_start_row;
    let mut records = Vec::new();
    for sheet_name in &state.cfg.poc_sheets {
        let headers = state
            .google
            .load_headers(&state.cfg.connector_spreadsheet_id, sheet_name)
            .await?;
        let last = crate::headers::a1_col(
            headers
                .idx(crate::headers::col::EMAIL)
                .or(headers.idx(crate::headers::col::FOLDER))
                .unwrap_or_else(|| headers.last_named_idx()),
        );
        let formatted = state
            .google
            .sheets_values_get(
                &state.cfg.connector_spreadsheet_id,
                &format!("'{sheet_name}'!A{start}:{last}"),
                None,
            )
            .await?;
        let resume_i = headers.idx(crate::headers::col::RESUME);
        let jd_i = headers.idx(crate::headers::col::JD);
        let folder_i = headers.idx(crate::headers::col::FOLDER);
        let formula_off = [resume_i, jd_i, folder_i]
            .into_iter()
            .flatten()
            .min()
            .unwrap_or(0);
        let formula_end = [resume_i, jd_i, folder_i]
            .into_iter()
            .flatten()
            .max()
            .unwrap_or(formula_off);
        let formula_start = crate::headers::a1_col(formula_off);
        let formula_end_l = crate::headers::a1_col(formula_end);
        let formulas = state
            .google
            .sheets_values_get(
                &state.cfg.connector_spreadsheet_id,
                &format!("'{sheet_name}'!{formula_start}{start}:{formula_end_l}"),
                Some("FORMULA"),
            )
            .await
            .unwrap_or_default();
        let hidden = headers.hidden_run(3);
        for (i, row) in formatted.iter().enumerate() {
            let candidate = headers.get_owned(row, crate::headers::col::CANDIDATE);
            let client = headers.get_owned(row, crate::headers::col::CLIENT);
            if candidate.is_empty() && client.is_empty() {
                continue;
            }
            let meeting_date = headers.get_owned(row, crate::headers::col::DATE);
            records.push(BookingRecord {
                kind: SheetKind::Interview,
                sheet_name: sheet_name.clone(),
                row: start + i as i32,
                candidate_name: candidate,
                client,
                interview_stage: headers.get_owned(row, crate::headers::col::STAGE),
                location: headers.get_owned(row, crate::headers::col::LOCATION),
                interview_platform: headers.get_owned(row, crate::headers::col::PLATFORM),
                poc: headers.get_owned(row, crate::headers::col::POC),
                meeting_date: meeting_date.clone(),
                meeting_time: headers.get_owned(row, crate::headers::col::TIME),
                meeting_duration: headers.get_owned(row, crate::headers::col::DURATION),
                panel: headers.get_owned(row, crate::headers::col::PANEL),
                vendor: headers.get_owned(row, crate::headers::col::VENDOR),
                support: String::new(),
                tech: "Data".into(),
                visa: String::new(),
                status: normalize_status(&headers.get_owned(row, crate::headers::col::STATUS)),
                special_note: headers.get_owned(row, crate::headers::col::NOTE),
                resume_link: first_url(&[
                    &formula_at(&formulas, i, resume_i, formula_off),
                    &headers.get_owned(row, crate::headers::col::RESUME),
                    &hidden.get(0).and_then(|c| row.get(*c)).cloned().unwrap_or_default(),
                ]),
                jd_link: first_url(&[
                    &formula_at(&formulas, i, jd_i, formula_off),
                    &headers.get_owned(row, crate::headers::col::JD),
                    &hidden.get(1).and_then(|c| row.get(*c)).cloned().unwrap_or_default(),
                ]),
                folder_link: first_url(&[
                    &formula_at(&formulas, i, folder_i, formula_off),
                    &headers.get_owned(row, crate::headers::col::FOLDER),
                    &hidden.get(2).and_then(|c| row.get(*c)).cloned().unwrap_or_default(),
                ]),
                submitter_email: headers.get_owned(row, crate::headers::col::EMAIL),
                date_key: parse_date_key(&meeting_date),
            });
        }
    }
    Ok(records)
}

/// Month tabs on the Data Interview Sheet are named either `August_2026` or, for
/// the 2025 tabs, just `December`. A 12-month window contains each month name at
/// most once, so the bare form is unambiguous inside it.
fn recent_month_tab_names(months: u32) -> Vec<(String, String)> {
    let today = chrono::Utc::now().with_timezone(&chrono_tz::America::Chicago).date_naive();
    let mut out = Vec::new();
    let mut year = today.year();
    let mut month = today.month();
    for _ in 0..months.min(12) {
        let name = chrono::NaiveDate::from_ymd_opt(year, month, 1)
            .map(|d| d.format("%B").to_string())
            .unwrap_or_default();
        if !name.is_empty() {
            out.push((format!("{name}_{year}"), name));
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

/// Past interviews logged on the Data Interview Sheet month tabs.
///
/// Header driven: the 2025 tabs put Vendor in column B while the 2026 tabs put
/// POC there, so columns are resolved by header name rather than by position.
/// Row 1 is the header, row 2 is a spacer, data starts at row 3.
pub async fn read_data_interview_history(
    state: &AppState,
    months: u32,
) -> anyhow::Result<Vec<HistoryRecord>> {
    let titles = state
        .google
        .sheet_titles_cached(&state.cfg.data_interview_spreadsheet_id)
        .await?;
    let mut records = Vec::new();

    for (dated, bare) in recent_month_tab_names(months) {
        let Some(tab) = titles
            .iter()
            .find(|t| t.trim() == dated)
            .or_else(|| titles.iter().find(|t| t.trim() == bare))
            .cloned()
        else {
            continue;
        };

        let rows = match state
            .google
            .sheets_values_get(
                &state.cfg.data_interview_spreadsheet_id,
                &format!("'{tab}'!A1:S"),
                Some("FORMATTED_VALUE"),
            )
            .await
        {
            Ok(rows) => rows,
            Err(err) => {
                tracing::warn!(tab = %tab, error = %err, "data interview month tab read failed");
                continue;
            }
        };
        if rows.len() < 3 {
            continue;
        }

        let headers = crate::headers::SheetHeaders::parse(&rows[0]);
        if headers.idx(crate::headers::col::CANDIDATE).is_none()
            || headers.idx(crate::headers::col::CLIENT).is_none()
        {
            continue;
        }

        for row in rows.iter().skip(2) {
            let candidate = headers.get_owned(row, crate::headers::col::CANDIDATE);
            let client = headers.get_owned(row, crate::headers::col::CLIENT);
            if candidate.is_empty() || client.is_empty() {
                continue;
            }
            let meeting_date = headers.get_owned(row, crate::headers::col::DATE);
            records.push(HistoryRecord {
                sheet_name: tab.clone(),
                candidate_name: candidate,
                client,
                vendor: headers.get_owned(row, crate::headers::col::VENDOR),
                tech: headers.get_owned(row, crate::headers::col::TECH),
                location: headers.get_owned(row, crate::headers::col::LOCATION),
                poc: headers.get_owned(row, crate::headers::col::POC),
                support: headers.get_owned(row, crate::headers::col::SUPPORT),
                panel: headers.get_owned(row, crate::headers::col::PANEL),
                meeting_date: meeting_date.clone(),
                meeting_time: headers.get_owned(row, crate::headers::col::TIME),
                meeting_duration: headers.get_owned(row, crate::headers::col::DURATION),
                note: headers.get_owned(row, crate::headers::col::NOTE),
                date_key: parse_date_key(&meeting_date),
            });
        }
    }

    Ok(records)
}
