use std::collections::HashMap;

use crate::dates::{parse_date_key, retention_cutoff_date_key};
use crate::google::drive::folder_view_url;
use crate::booking::PHONE_CALLS_DATA_START_ROW;
use crate::google::sheets::{hyperlink_formula, links_to_restore, url_from_grid_cell, RichTextLinkCell};
use crate::google::token::GoogleClient;
use crate::headers::{a1_col, col, SheetHeaders};
use crate::AppState;

const FOLDER_MIME: &str = "application/vnd.google-apps.folder";

fn is_booking_row(row: &[String], headers: &SheetHeaders) -> bool {
    !headers.get(row, col::CANDIDATE).is_empty()
        || !headers.get(row, col::CLIENT).is_empty()
        || !headers.get(row, col::DATE).is_empty()
}

/// Visible JD / Resume / Folder columns, then the three hidden URL backups.
fn phone_link_cols(headers: &SheetHeaders) -> (Option<usize>, Option<usize>, Option<usize>, [usize; 3]) {
    let jd = headers.idx(col::JD);
    let resume = headers.idx(col::RESUME);
    let folder = headers.idx(col::FOLDER);
    let run = headers.hidden_run(3);
    let hidden = if run.len() >= 3 {
        [run[0], run[1], run[2]]
    } else {
        let start = headers.last_named_idx() + 1;
        [start, start + 1, start + 2]
    };
    (jd, resume, folder, hidden)
}

impl PhoneCallRowLinks {
    fn as_array(&self) -> [String; 3] {
        [self.jd.clone(), self.resume.clone(), self.folder.clone()]
    }
}

#[derive(Clone, Default)]
pub struct PhoneCallRowLinks {
    pub jd: String,
    pub resume: String,
    pub folder: String,
}

fn norm(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect()
}

fn tokens(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| t.len() > 2)
        .map(|t| t.to_string())
        .collect()
}

fn classify(name: &str) -> &'static str {
    let n = name.to_lowercase();
    if regex::Regex::new(r"job_description|job description|_jd|jobdescription")
        .unwrap()
        .is_match(&n)
        || n.starts_with("jd")
    {
        "jd"
    } else if n.contains("resume") {
        "resume"
    } else {
        ""
    }
}

#[derive(Clone)]
struct DriveItem {
    id: String,
    name: String,
    mime_type: String,
    web_view_link: String,
    modified_time: String,
}

impl From<crate::google::drive::DriveFile> for DriveItem {
    fn from(f: crate::google::drive::DriveFile) -> Self {
        Self {
            id: f.id.unwrap_or_default(),
            name: f.name.unwrap_or_default(),
            mime_type: f.mime_type.unwrap_or_default(),
            web_view_link: f.web_view_link.unwrap_or_default(),
            modified_time: f.modified_time.unwrap_or_default(),
        }
    }
}

async fn list_children(g: &GoogleClient, parent_id: &str) -> anyhow::Result<Vec<DriveItem>> {
    let q = format!("'{parent_id}' in parents and trashed=false");
    let files = g
        .drive_list_all(&q, "nextPageToken,files(id,name,mimeType,webViewLink,modifiedTime)")
        .await?;
    Ok(files
        .into_iter()
        .filter(|f| f.id.is_some() && f.name.is_some())
        .map(DriveItem::from)
        .collect())
}

async fn collect_files(g: &GoogleClient, folder_id: &str, depth: u32) -> anyhow::Result<Vec<DriveItem>> {
    let kids = list_children(g, folder_id).await?;
    let mut extra = Vec::new();
    if depth < 2 {
        for item in &kids {
            if item.mime_type == FOLDER_MIME {
                extra.extend(Box::pin(collect_files(g, &item.id, depth + 1)).await?);
            }
        }
    }
    let mut out = kids;
    out.extend(extra);
    Ok(out)
}

fn pick<'a>(files: &'a [DriveItem], kind: &str, client: &str) -> Option<&'a DriveItem> {
    let mut matched: Vec<&DriveItem> = files.iter().filter(|f| classify(&f.name) == kind).collect();
    if matched.is_empty() {
        return None;
    }
    let toks = tokens(client);
    matched.sort_by(|a, b| {
        let sa = toks.iter().filter(|t| a.name.to_lowercase().contains(t.as_str())).count();
        let sb = toks.iter().filter(|t| b.name.to_lowercase().contains(t.as_str())).count();
        sb.cmp(&sa)
            .then_with(|| b.modified_time.cmp(&a.modified_time))
    });
    matched.first().copied()
}

fn find_candidate<'a>(folders: &'a [DriveItem], name: &str) -> Option<&'a DriveItem> {
    let key = norm(name);
    if let Some(exact) = folders.iter().find(|f| norm(&f.name) == key) {
        return Some(exact);
    }
    let want: std::collections::HashSet<_> = tokens(name).into_iter().collect();
    let mut best = None;
    let mut best_score = 0;
    for folder in folders {
        let score = tokens(&folder.name).iter().filter(|t| want.contains(*t)).count();
        if score > best_score {
            best = Some(folder);
            best_score = score;
        }
    }
    if best_score > 0 {
        best
    } else {
        None
    }
}

fn file_url(item: Option<&DriveItem>) -> String {
    let Some(item) = item else {
        return String::new();
    };
    if !item.web_view_link.trim().is_empty() {
        item.web_view_link.trim().to_string()
    } else if !item.id.is_empty() {
        format!("https://drive.google.com/file/d/{}/view", item.id)
    } else {
        String::new()
    }
}

pub(crate) async fn recover_phone_call_links_from_drive(
    state: &AppState,
    candidate: &str,
    client: &str,
    candidate_folders: Option<&[DriveItem]>,
) -> anyhow::Result<PhoneCallRowLinks> {
    let owned;
    let folders = if let Some(f) = candidate_folders {
        f
    } else {
        owned = list_children(&state.google, &state.cfg.resumes_data_folder_id)
            .await?
            .into_iter()
            .filter(|f| f.mime_type == FOLDER_MIME)
            .collect::<Vec<_>>();
        &owned
    };
    let Some(folder) = find_candidate(folders, candidate) else {
        return Ok(PhoneCallRowLinks::default());
    };
    let kids = list_children(&state.google, &folder.id).await?;
    let mut company = None;
    let mut best = 0;
    for kid in &kids {
        if kid.mime_type != FOLDER_MIME {
            continue;
        }
        let score = tokens(client)
            .iter()
            .filter(|t| kid.name.to_lowercase().contains(t.as_str()))
            .count();
        if score > best {
            company = Some(kid);
            best = score;
        }
    }
    let target = company.unwrap_or(folder);
    let files = collect_files(&state.google, &target.id, 0).await?;
    let stage = files
        .iter()
        .find(|f| f.mime_type == FOLDER_MIME && f.name.to_lowercase().contains("phone"));
    let drive_id = stage.unwrap_or(target).id.clone();
    Ok(PhoneCallRowLinks {
        jd: file_url(pick(&files, "jd", client)),
        resume: file_url(pick(&files, "resume", client)),
        folder: folder_view_url(&drive_id),
    })
}

pub async fn read_phone_call_row_links(
    state: &AppState,
) -> anyhow::Result<HashMap<i32, PhoneCallRowLinks>> {
    let spreadsheet_id = &state.cfg.data_interview_spreadsheet_id;
    let sheet_name = &state.cfg.phone_calls_sheet;
    let values = state
        .google
        .sheets_values_get(spreadsheet_id, &format!("'{sheet_name}'!A1:AZ"), None)
        .await?;
    if values.is_empty() {
        return Ok(HashMap::new());
    }
    let headers = SheetHeaders::parse(&values[0]);
    let (jd, resume, folder, _) = phone_link_cols(&headers);
    let present: Vec<usize> = [jd, resume, folder].into_iter().flatten().collect();
    if present.is_empty() {
        return Ok(HashMap::new());
    }
    let min = *present.iter().min().unwrap();
    let max = *present.iter().max().unwrap();
    let row_count = values.len().max(1);
    let grid = state
        .google
        .grid_data(
            spreadsheet_id,
            &format!("'{sheet_name}'!{}1:{}{row_count}", a1_col(min), a1_col(max)),
            "sheets(data(rowData(values(formattedValue,hyperlink,userEnteredValue,textFormatRuns))))",
        )
        .await?;
    let mut out = HashMap::new();
    for i in 1..row_count {
        let cells = grid.get(i).and_then(|r| r.values.as_ref());
        let at = |idx: Option<usize>| {
            idx.map(|i| url_from_grid_cell(cells.and_then(|c| c.get(i.saturating_sub(min)))))
                .unwrap_or_default()
        };
        out.insert(
            i as i32 + 1,
            PhoneCallRowLinks {
                jd: at(jd),
                resume: at(resume),
                folder: at(folder),
            },
        );
    }
    Ok(out)
}

pub async fn repair_phone_calls_hyperlinks(state: &AppState) -> anyhow::Result<serde_json::Value> {
    let spreadsheet_id = &state.cfg.data_interview_spreadsheet_id;
    let sheet_name = &state.cfg.phone_calls_sheet;
    let sheet_id = state.google.sheet_id(spreadsheet_id, sheet_name).await?;
    let rows = state
        .google
        .sheets_values_get(spreadsheet_id, &format!("'{sheet_name}'!A1:AZ"), None)
        .await?;
    if rows.len() < 2 {
        return Ok(serde_json::json!({
            "scanned": 0, "repaired": 0, "alreadyOk": 0, "missing": 0, "cellsWritten": 0, "backupsSynced": 0
        }));
    }
    let headers = SheetHeaders::parse(&rows[0]);
    let (jd_i, resume_i, folder_i, _) = phone_link_cols(&headers);
    let existing = read_phone_call_row_links(state).await?;
    let candidate_folders: Vec<DriveItem> = list_children(&state.google, &state.cfg.resumes_data_folder_id)
        .await?
        .into_iter()
        .filter(|f| f.mime_type == FOLDER_MIME)
        .collect();

    let mut batch = Vec::new();
    let mut repaired = 0;
    let mut already_ok = 0;
    let mut missing = 0;
    // What each row's links will be once this run is done, for the backup sync.
    let mut final_links: HashMap<i32, PhoneCallRowLinks> = HashMap::new();

    for i in 1..rows.len() {
        let row = &rows[i];
        let candidate = headers.get(row, col::CANDIDATE).to_string();
        let client = headers.get(row, col::CLIENT).to_string();
        if candidate.is_empty() {
            continue;
        }
        let sheet_row = i as i32 + 1;
        let cur = existing.get(&sheet_row).cloned().unwrap_or_default();
        let need_jd = cur.jd.is_empty();
        let need_resume = cur.resume.is_empty();
        let need_folder = cur.folder.is_empty();
        if !need_jd && !need_resume && !need_folder {
            already_ok += 1;
            final_links.insert(sheet_row, cur);
            continue;
        }
        let recovered =
            recover_phone_call_links_from_drive(state, &candidate, &client, Some(&candidate_folders)).await?;
        let next = PhoneCallRowLinks {
            jd: if cur.jd.is_empty() { recovered.jd } else { cur.jd },
            resume: if cur.resume.is_empty() {
                recovered.resume
            } else {
                cur.resume
            },
            folder: if cur.folder.is_empty() {
                recovered.folder
            } else {
                cur.folder
            },
        };
        final_links.insert(sheet_row, next.clone());
        let mut cells = Vec::new();
        if need_jd && !next.jd.is_empty() {
            if let Some(jd_i) = jd_i {
                cells.push(RichTextLinkCell {
                    column1: jd_i as i32 + 1,
                    url: next.jd,
                    label: "Job Description Link".into(),
                });
            }
        }
        if need_resume && !next.resume.is_empty() {
            if let Some(resume_i) = resume_i {
                cells.push(RichTextLinkCell {
                    column1: resume_i as i32 + 1,
                    url: next.resume,
                    label: "Resume Link".into(),
                });
            }
        }
        if need_folder && !next.folder.is_empty() {
            if let Some(folder_i) = folder_i {
                cells.push(RichTextLinkCell {
                    column1: folder_i as i32 + 1,
                    url: next.folder,
                    label: "Drive Folder Link".into(),
                });
            }
        }
        if cells.is_empty() {
            missing += 1;
            continue;
        }
        batch.push((sheet_row, cells));
        repaired += 1;
    }
    let cells_written = state
        .google
        .write_rich_text_hyperlinks_batch(spreadsheet_id, sheet_id, &batch)
        .await?;
    let backups_synced =
        sync_phone_calls_link_backups(state, sheet_id, &headers, &rows, &final_links).await?;
    Ok(serde_json::json!({
        "scanned": rows.len() - 1,
        "repaired": repaired,
        "alreadyOk": already_ok,
        "missing": missing,
        "cellsWritten": cells_written,
        "backupsSynced": backups_synced
    }))
}

/// Point every hidden URL backup at its own row's visible links, and empty the
/// backups on rows that hold no booking.
///
/// The backups exist to refill a link that got flattened, which only works if
/// they still describe the row they sit on. Rows moved without them left a
/// trail of other bookings' URLs down the tab; this makes that self-healing.
/// A booking row with no visible link keeps its backup — that is exactly the
/// case the backup is for.
async fn sync_phone_calls_link_backups(
    state: &AppState,
    sheet_id: i64,
    headers: &SheetHeaders,
    rows: &[Vec<String>],
    final_links: &HashMap<i32, PhoneCallRowLinks>,
) -> anyhow::Result<usize> {
    let (_, _, _, hidden) = phone_link_cols(headers);
    let mut requests = Vec::new();
    for (i, row) in rows.iter().enumerate().skip(PHONE_CALLS_DATA_START_ROW as usize - 1) {
        let sheet_row = i as i32 + 1;
        let backup: [String; 3] = std::array::from_fn(|c| {
            row.get(hidden[c])
                .map(|v| v.trim().to_string())
                .unwrap_or_default()
        });
        let target = backup_target(is_booking_row(row, headers), final_links.get(&sheet_row), &backup);
        if target != backup {
            requests.push(serde_json::json!({
                "updateCells": {
                    "start": { "sheetId": sheet_id, "rowIndex": sheet_row - 1, "columnIndex": hidden[0] as i32 },
                    "rows": [{ "values": target.iter().map(|u| serde_json::json!({
                        "userEnteredValue": { "stringValue": u }
                    })).collect::<Vec<_>>() }],
                    "fields": "userEnteredValue"
                }
            }));
        }
    }
    let n = requests.len();
    for chunk in requests.chunks(80) {
        state
            .google
            .batch_update(&state.cfg.data_interview_spreadsheet_id, chunk.to_vec())
            .await?;
    }
    Ok(n)
}

/// What a row's hidden backup should hold: its visible links where it has
/// them, its existing backup where it doesn't, nothing on a non-booking row.
fn backup_target(
    is_booking: bool,
    visible: Option<&PhoneCallRowLinks>,
    backup: &[String; 3],
) -> [String; 3] {
    if !is_booking {
        return Default::default();
    }
    let visible = visible.map(PhoneCallRowLinks::as_array).unwrap_or_default();
    std::array::from_fn(|c| {
        if visible[c].is_empty() {
            backup[c].clone()
        } else {
            visible[c].clone()
        }
    })
}

/// Drops Phone calls rows older than the retention window and closes the gaps.
///
/// Off unless `RUST_PHONE_CALLS_PRUNE` is set — see `Config::rust_phone_calls_prune`.
/// When it does run, each kept row moves as a whole, hidden link backups
/// included, and data stays below the two-row header gap.
pub async fn prune_phone_calls_older_than_retention(state: &AppState) -> anyhow::Result<serde_json::Value> {
    if !state.cfg.rust_phone_calls_prune {
        return Ok(serde_json::json!({
            "skipped": "Apps Script prunePhoneCallsRetentionDaily owns Phone calls retention; set RUST_PHONE_CALLS_PRUNE=1 to run it here instead"
        }));
    }
    let spreadsheet_id = &state.cfg.data_interview_spreadsheet_id;
    let sheet_name = &state.cfg.phone_calls_sheet;
    let start = PHONE_CALLS_DATA_START_ROW;
    let cutoff = retention_cutoff_date_key(state.cfg.phone_calls_retention_days);
    let rows = state
        .google
        .sheets_values_get(spreadsheet_id, &format!("'{sheet_name}'!A1:AZ"), None)
        .await?;
    if rows.len() < start as usize {
        let repair = repair_phone_calls_hyperlinks(state).await?;
        return Ok(serde_json::json!({
            "removed": 0, "kept": 0, "cutoff": cutoff, "repair": repair
        }));
    }
    let headers = SheetHeaders::parse(&rows[0]);
    let (jd_i, resume_i, folder_i, hidden) = phone_link_cols(&headers);
    let existing = read_phone_call_row_links(state).await?;

    let body = &rows[start as usize - 1..];
    let visible: Vec<[String; 3]> = (0..body.len())
        .map(|i| existing.get(&(start + i as i32)).cloned().unwrap_or_default().as_array())
        .collect();
    let backup: Vec<[String; 3]> = body
        .iter()
        .map(|r| std::array::from_fn(|c| r.get(hidden[c]).map(|v| v.trim().to_string()).unwrap_or_default()))
        .collect();
    let is_booking: Vec<bool> = body.iter().map(|r| is_booking_row(r, &headers)).collect();
    let mut links = visible.clone();
    for (i, c, url) in links_to_restore(&visible, &backup, &is_booking) {
        links[i][c] = url;
    }

    let width = hidden.iter().copied().max().unwrap_or(21) + 1;
    let last_letter = a1_col(width - 1);
    let mut out = Vec::new();
    let mut removed = 0;
    for (i, row) in body.iter().enumerate() {
        if !is_booking[i] {
            continue;
        }
        let date_key = parse_date_key(headers.get(row, col::DATE));
        if !date_key.is_empty() && date_key < cutoff {
            removed += 1;
            continue;
        }
        let mut row_links = links[i].clone();
        if row_links.iter().any(|u| u.is_empty()) {
            let recovered = recover_phone_call_links_from_drive(
                state,
                headers.get(row, col::CANDIDATE),
                headers.get(row, col::CLIENT),
                None,
            )
            .await?;
            for (c, url) in recovered.as_array().into_iter().enumerate() {
                if row_links[c].is_empty() {
                    row_links[c] = url;
                }
            }
        }
        let mut next: Vec<serde_json::Value> = (0..width)
            .map(|k| serde_json::json!(row.get(k).cloned().unwrap_or_default()))
            .collect();
        let set_link = |next: &mut [serde_json::Value], idx: Option<usize>, url: &str, label: &str| {
            let Some(idx) = idx else {
                return;
            };
            next[idx] = serde_json::json!(if url.is_empty() {
                String::new()
            } else {
                hyperlink_formula(url, label)
            });
        };
        set_link(&mut next, jd_i, &row_links[0], "Job Description Link");
        set_link(&mut next, resume_i, &row_links[1], "Resume Link");
        set_link(&mut next, folder_i, &row_links[2], "Drive Folder Link");
        for c in 0..3 {
            next[hidden[c]] = serde_json::json!(row_links[c].clone());
        }
        out.push(next);
    }

    let last_row = rows.len() as i32;
    state
        .google
        .sheets_values_clear(spreadsheet_id, &format!("'{sheet_name}'!A{start}:{last_letter}{last_row}"))
        .await?;
    if !out.is_empty() {
        let end = start + out.len() as i32 - 1;
        state
            .google
            .sheets_values_update(
                spreadsheet_id,
                &format!("'{sheet_name}'!A{start}:{last_letter}{end}"),
                out.clone(),
                "USER_ENTERED",
            )
            .await?;
    }
    let repair = repair_phone_calls_hyperlinks(state).await?;
    Ok(serde_json::json!({
        "removed": removed,
        "kept": out.len(),
        "cutoff": cutoff,
        "repair": repair
    }))
}

#[cfg(test)]
mod backup_tests {
    use super::{backup_target, is_booking_row, PhoneCallRowLinks, SheetHeaders};

    fn arr(a: &str, b: &str, c: &str) -> [String; 3] {
        [a.into(), b.into(), c.into()]
    }

    #[test]
    fn a_backup_follows_its_rows_visible_links() {
        let visible = PhoneCallRowLinks { jd: "https://jd/monika".into(), resume: String::new(), folder: "https://f/monika".into() };
        let stale = arr("https://jd/sanket", "https://r/sanket", "https://f/sanket");
        // The resume link is missing, so its backup is kept for a later refill.
        assert_eq!(
            backup_target(true, Some(&visible), &stale),
            arr("https://jd/monika", "https://r/sanket", "https://f/monika")
        );
    }

    #[test]
    fn a_booking_with_no_visible_links_keeps_its_backup() {
        let backup = arr("https://jd/x", "https://r/x", "https://f/x");
        assert_eq!(backup_target(true, None, &backup), backup);
    }

    /// Rows 14-23 on 2026-09-14: no booking, but other candidates' URLs.
    #[test]
    fn orphaned_backups_are_emptied() {
        let orphan = arr("https://jd/shweta", "https://r/shweta", "https://f/shweta");
        assert_eq!(backup_target(false, None, &orphan), arr("", "", ""));
    }

    #[test]
    fn a_dragged_support_value_is_not_a_booking() {
        let headers = SheetHeaders::parse(&[
            "POC".into(),
            "".into(),
            "Client".into(),
            "Tech".into(),
            "Location".into(),
            "Visa".into(),
            "Candidate".into(),
            "Date".into(),
            "Time".into(),
            "Duration".into(),
            "Panel".into(),
            "Support".into(),
        ]);
        let mut row = vec![String::new(); 22];
        row[11] = "Sushant".into();
        assert!(!is_booking_row(&row, &headers));
        row[6] = "Monika Regmi".into();
        assert!(is_booking_row(&row, &headers));
    }
}
