use std::collections::HashMap;

use serde::Deserialize;
use serde_json::{json, Value};

use super::token::GoogleClient;

const SHEETS_RETRY_ATTEMPTS: u32 = 5;
const SHEETS_RETRY_BASE_MS: u64 = 700;

const QUOTA_USER_MESSAGE: &str =
    "Google Sheets is busy right now. Wait about a minute and try again.";

fn body_is_sheets_rate_limit(body: &str) -> bool {
    body.contains("RATE_LIMIT_EXCEEDED")
        || body.contains("RESOURCE_EXHAUSTED")
        || body.contains("Quota exceeded")
}

fn is_sheets_rate_limit(status: reqwest::StatusCode, body: &str) -> bool {
    status.as_u16() == 429 || body_is_sheets_rate_limit(body)
}

pub fn friendly_sheets_error(action: &str, status: reqwest::StatusCode, body: &str) -> String {
    if is_sheets_rate_limit(status, body) {
        return QUOTA_USER_MESSAGE.into();
    }
    if let Ok(v) = serde_json::from_str::<Value>(body) {
        if let Some(msg) = v
            .pointer("/error/message")
            .and_then(|m| m.as_str())
            .map(str::trim)
            .filter(|m| !m.is_empty())
        {
            if msg.len() > 180 {
                return format!("{action} failed. Try again in a moment.");
            }
            return format!("{action} failed: {msg}");
        }
    }
    let trimmed = body.trim();
    if trimmed.is_empty() {
        format!("{action} failed (HTTP {}).", status.as_u16())
    } else if trimmed.len() > 180 || trimmed.starts_with('{') {
        format!("{action} failed. Try again in a moment.")
    } else {
        format!("{action} failed: {trimmed}")
    }
}

pub fn sanitize_google_user_error(message: &str) -> String {
    let text = message.trim();
    if text.is_empty() {
        return "Something went wrong. Try again.".into();
    }
    // Body patterns only. There is no HTTP status at this point, and passing a
    // hardcoded 429 made is_sheets_rate_limit true for *every* error, so real
    // failures (missing POC, bad input) all surfaced as "Sheets is busy".
    if body_is_sheets_rate_limit(text)
        || text.contains("Read requests per minute")
        || text.contains("sheets.googleapis.com")
    {
        return QUOTA_USER_MESSAGE.into();
    }
    if text.contains("{\"error\"") || text.contains("\"RESOURCE_EXHAUSTED\"") {
        return "Google is temporarily unavailable. Try again in a moment.".into();
    }
    text.to_string()
}

async fn sheets_send_with_retry<F, Fut>(
    action: &str,
    mut send: F,
) -> anyhow::Result<(reqwest::StatusCode, String)>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<reqwest::Response>>,
{
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        let res = send().await?;
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        if status.is_success() {
            return Ok((status, body));
        }
        if is_sheets_rate_limit(status, &body) && attempt < SHEETS_RETRY_ATTEMPTS {
            let wait_ms = SHEETS_RETRY_BASE_MS * (1u64 << (attempt - 1));
            tracing::warn!(
                "{action} hit Sheets quota (attempt {attempt}/{SHEETS_RETRY_ATTEMPTS}); retrying in {wait_ms}ms"
            );
            tokio::time::sleep(std::time::Duration::from_millis(wait_ms)).await;
            continue;
        }
        anyhow::bail!("{}", friendly_sheets_error(action, status, &body));
    }
}

#[derive(Deserialize)]
struct ValuesResponse {
    values: Option<Vec<Vec<Value>>>,
}

#[derive(Deserialize)]
struct BatchGetResponse {
    #[serde(rename = "valueRanges")]
    value_ranges: Option<Vec<ValueRangeItem>>,
}

#[derive(Deserialize)]
struct ValueRangeItem {
    values: Option<Vec<Vec<Value>>>,
}

#[derive(Deserialize)]
struct Spreadsheet {
    sheets: Option<Vec<Sheet>>,
    properties: Option<SheetProps>,
}

#[derive(Deserialize)]
struct Sheet {
    properties: Option<SheetProps>,
    data: Option<Vec<GridData>>,
}

#[derive(Deserialize)]
struct SheetProps {
    #[serde(rename = "sheetId")]
    sheet_id: Option<i64>,
    title: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct GridData {
    #[serde(rename = "rowData")]
    pub row_data: Option<Vec<RowData>>,
}

#[derive(Deserialize, Default)]
pub struct RowData {
    pub values: Option<Vec<GridCell>>,
}

#[derive(Deserialize, Default, Clone)]
pub struct GridCell {
    #[serde(rename = "formattedValue")]
    pub formatted_value: Option<String>,
    pub hyperlink: Option<String>,
    #[serde(rename = "userEnteredValue")]
    pub user_entered_value: Option<UserEntered>,
    #[serde(rename = "textFormatRuns")]
    pub text_format_runs: Option<Vec<TextRun>>,
    #[serde(rename = "effectiveFormat")]
    pub effective_format: Option<CellFormat>,
}

#[derive(Deserialize, Default, Clone)]
pub struct CellFormat {
    #[serde(rename = "backgroundColor")]
    pub background_color: Option<CellColor>,
}

#[derive(Deserialize, Default, Clone)]
pub struct CellColor {
    #[serde(default)]
    pub red: f32,
    #[serde(default)]
    pub green: f32,
    #[serde(default)]
    pub blue: f32,
}

impl CellColor {
    /// A red fill, tolerant of the softer reds people also use by hand.
    pub fn is_red(&self) -> bool {
        self.red > 0.7 && self.green < 0.55 && self.blue < 0.55
    }
}

/// True when the row carries a red fill in any of the sampled cells.
pub fn row_is_red(row: &RowData) -> bool {
    row.values
        .as_ref()
        .map(|cells| {
            cells.iter().any(|c| {
                c.effective_format
                    .as_ref()
                    .and_then(|f| f.background_color.as_ref())
                    .map(|bg| bg.is_red())
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

#[derive(Deserialize, Default, Clone)]
pub struct UserEntered {
    #[serde(rename = "formulaValue")]
    pub formula_value: Option<String>,
    #[serde(rename = "stringValue")]
    pub string_value: Option<String>,
}

#[derive(Deserialize, Default, Clone)]
pub struct TextRun {
    pub format: Option<TextFormat>,
}

#[derive(Deserialize, Default, Clone)]
pub struct TextFormat {
    pub link: Option<LinkUri>,
}

#[derive(Deserialize, Default, Clone)]
pub struct LinkUri {
    pub uri: Option<String>,
}

pub fn cell_str(row: &[String], idx: i32) -> String {
    if idx < 0 {
        return String::new();
    }
    row.get(idx as usize).cloned().unwrap_or_default().trim().to_string()
}

pub fn values_to_strings(values: Option<Vec<Vec<Value>>>) -> Vec<Vec<String>> {
    values
        .unwrap_or_default()
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(|v| match v {
                    Value::String(s) => s,
                    Value::Null => String::new(),
                    other => other.to_string().trim_matches('"').to_string(),
                })
                .collect()
        })
        .collect()
}

pub fn url_from_grid_cell(cell: Option<&GridCell>) -> String {
    let Some(cell) = cell else {
        return String::new();
    };
    let direct = cell.hyperlink.clone().unwrap_or_default();
    if direct.starts_with("http://") || direct.starts_with("https://") {
        return direct;
    }
    if let Some(runs) = &cell.text_format_runs {
        for run in runs {
            if let Some(uri) = run
                .format
                .as_ref()
                .and_then(|f| f.link.as_ref())
                .and_then(|l| l.uri.clone())
            {
                let uri = uri.trim().to_string();
                if uri.starts_with("http://") || uri.starts_with("https://") {
                    return uri;
                }
            }
        }
    }
    let formula = cell
        .user_entered_value
        .as_ref()
        .and_then(|u| u.formula_value.clone())
        .unwrap_or_default();
    if let Some(caps) = regex::Regex::new(r#"(?i)HYPERLINK\s*\(\s*"([^"]+)""#)
        .unwrap()
        .captures(&formula)
        .or_else(|| {
            regex::Regex::new(r"(?i)HYPERLINK\s*\(\s*'([^']+)'")
                .unwrap()
                .captures(&formula)
        })
    {
        return caps[1].trim().to_string();
    }
    let text = cell
        .user_entered_value
        .as_ref()
        .and_then(|u| u.string_value.clone())
        .or_else(|| cell.formatted_value.clone())
        .unwrap_or_default();
    let text = text.trim();
    if text.starts_with("http://") || text.starts_with("https://") {
        text.to_string()
    } else {
        String::new()
    }
}

impl GoogleClient {
    pub async fn sheets_values_get(
        &self,
        spreadsheet_id: &str,
        range: &str,
        render: Option<&str>,
    ) -> anyhow::Result<Vec<Vec<String>>> {
        let encoded = urlencoding::encode(range);
        let mut url = format!(
            "https://sheets.googleapis.com/v4/spreadsheets/{spreadsheet_id}/values/{encoded}"
        );
        if let Some(render) = render {
            url.push_str(&format!("?valueRenderOption={render}"));
        }
        let url = url;
        let (_, body) = sheets_send_with_retry("Sheet read", || {
            let url = url.clone();
            async move {
                let token = self.sa_token().await?;
                Ok(self.http().get(url).bearer_auth(token).send().await?)
            }
        })
        .await?;
        let parsed: ValuesResponse = serde_json::from_str(&body)?;
        Ok(values_to_strings(parsed.values))
    }

    /// One round trip for many ranges. Results are in the same order as `ranges`.
    pub async fn sheets_values_batch_get(
        &self,
        spreadsheet_id: &str,
        ranges: &[String],
        render: Option<&str>,
    ) -> anyhow::Result<Vec<Vec<Vec<String>>>> {
        if ranges.is_empty() {
            return Ok(Vec::new());
        }
        let mut out = Vec::with_capacity(ranges.len());
        for chunk in ranges.chunks(40) {
            let mut url = format!(
                "https://sheets.googleapis.com/v4/spreadsheets/{spreadsheet_id}/values:batchGet?"
            );
            for (i, range) in chunk.iter().enumerate() {
                if i > 0 {
                    url.push('&');
                }
                url.push_str("ranges=");
                url.push_str(&urlencoding::encode(range));
            }
            if let Some(render) = render {
                url.push_str("&valueRenderOption=");
                url.push_str(render);
            }
            let url = url;
            let (_, body) = sheets_send_with_retry("Sheet batch read", || {
                let url = url.clone();
                async move {
                    let token = self.sa_token().await?;
                    Ok(self.http().get(url).bearer_auth(token).send().await?)
                }
            })
            .await?;
            let parsed: BatchGetResponse = serde_json::from_str(&body)?;
            let value_ranges = parsed.value_ranges.unwrap_or_default();
            for item in value_ranges {
                out.push(values_to_strings(item.values));
            }
        }
        Ok(out)
    }

    pub async fn sheets_values_update(
        &self,
        spreadsheet_id: &str,
        range: &str,
        values: Vec<Vec<serde_json::Value>>,
        input: &str,
    ) -> anyhow::Result<()> {
        let encoded = urlencoding::encode(range);
        let url = format!(
            "https://sheets.googleapis.com/v4/spreadsheets/{spreadsheet_id}/values/{encoded}?valueInputOption={input}"
        );
        let payload = json!({ "values": values });
        sheets_send_with_retry("Sheet update", || {
            let url = url.clone();
            let payload = payload.clone();
            async move {
                let token = self.sa_token().await?;
                Ok(self
                    .http()
                    .put(url)
                    .bearer_auth(token)
                    .json(&payload)
                    .send()
                    .await?)
            }
        })
        .await?;
        Ok(())
    }

    pub async fn sheets_values_clear(&self, spreadsheet_id: &str, range: &str) -> anyhow::Result<()> {
        let encoded = urlencoding::encode(range);
        let url = format!(
            "https://sheets.googleapis.com/v4/spreadsheets/{spreadsheet_id}/values/{encoded}:clear"
        );
        sheets_send_with_retry("Sheet clear", || {
            let url = url.clone();
            async move {
                let token = self.sa_token().await?;
                Ok(self
                    .http()
                    .post(url)
                    .bearer_auth(token)
                    .json(&json!({}))
                    .send()
                    .await?)
            }
        })
        .await?;
        Ok(())
    }

    pub async fn sheets_meta(&self, spreadsheet_id: &str, fields: &str) -> anyhow::Result<Value> {
        let url = format!(
            "https://sheets.googleapis.com/v4/spreadsheets/{spreadsheet_id}?fields={}",
            urlencoding::encode(fields)
        );
        let (_, body) = sheets_send_with_retry("Sheet metadata", || {
            let url = url.clone();
            async move {
                let token = self.sa_token().await?;
                Ok(self.http().get(url).bearer_auth(token).send().await?)
            }
        })
        .await?;
        Ok(serde_json::from_str(&body)?)
    }

    pub async fn sheet_id(&self, spreadsheet_id: &str, sheet_name: &str) -> anyhow::Result<i64> {
        let meta = self
            .sheets_meta(spreadsheet_id, "sheets.properties(sheetId,title)")
            .await?;
        let parsed: Spreadsheet = serde_json::from_value(meta)?;
        for sheet in parsed.sheets.unwrap_or_default() {
            if sheet.properties.as_ref().and_then(|p| p.title.as_deref()) == Some(sheet_name) {
                if let Some(id) = sheet.properties.and_then(|p| p.sheet_id) {
                    return Ok(id);
                }
            }
        }
        anyhow::bail!("Sheet \"{sheet_name}\" not found.");
    }

    pub async fn sheet_titles(&self, spreadsheet_id: &str) -> anyhow::Result<Vec<String>> {
        let meta = self.sheets_meta(spreadsheet_id, "sheets.properties.title").await?;
        let parsed: Spreadsheet = serde_json::from_value(meta)?;
        Ok(parsed
            .sheets
            .unwrap_or_default()
            .into_iter()
            .filter_map(|s| s.properties.and_then(|p| p.title))
            .collect())
    }

    pub async fn spreadsheet_title(&self, spreadsheet_id: &str) -> anyhow::Result<String> {
        let meta = self.sheets_meta(spreadsheet_id, "properties.title").await?;
        let parsed: Spreadsheet = serde_json::from_value(meta)?;
        Ok(parsed.properties.and_then(|p| p.title).unwrap_or_else(|| spreadsheet_id.to_string()))
    }

    pub async fn batch_update(&self, spreadsheet_id: &str, requests: Vec<Value>) -> anyhow::Result<()> {
        if requests.is_empty() {
            return Ok(());
        }
        let url = format!("https://sheets.googleapis.com/v4/spreadsheets/{spreadsheet_id}:batchUpdate");
        let payload = json!({ "requests": requests });
        sheets_send_with_retry("Sheet batch update", || {
            let url = url.clone();
            let payload = payload.clone();
            async move {
                let token = self.sa_token().await?;
                Ok(self
                    .http()
                    .post(url)
                    .bearer_auth(token)
                    .json(&payload)
                    .send()
                    .await?)
            }
        })
        .await?;
        Ok(())
    }

    pub async fn load_headers(
        &self,
        spreadsheet_id: &str,
        sheet_name: &str,
    ) -> anyhow::Result<crate::headers::SheetHeaders> {
        let rows = self
            .sheets_values_get(
                spreadsheet_id,
                &format!("'{sheet_name}'!A1:AZ8"),
                Some("FORMATTED_VALUE"),
            )
            .await?;
        Ok(crate::headers::SheetHeaders::detect(&rows))
    }

    /// First empty row at the BOTTOM of a connector data block (1-based), mirroring
    /// the Apps Script `connectorNextAppendRow_`. New rows must append below existing
    /// data — never insert at the top, which shifts every row and flattens the
    /// O:Q / N:P rich-text hyperlinks on the survivors.
    pub async fn connector_next_append_row(
        &self,
        spreadsheet_id: &str,
        sheet_name: &str,
        data_start_row: i32,
        last_col: &str,
    ) -> anyhow::Result<i32> {
        let rows = self
            .sheets_values_get(
                spreadsheet_id,
                &format!("'{sheet_name}'!A{data_start_row}:{last_col}"),
                None,
            )
            .await?;
        let mut last_non_empty = 0i32;
        for (i, row) in rows.iter().enumerate() {
            if row.iter().any(|c| !c.trim().is_empty()) {
                last_non_empty = i as i32 + 1;
            }
        }
        Ok(data_start_row + last_non_empty)
    }

    pub async fn insert_row_at(
        &self,
        spreadsheet_id: &str,
        sheet_name: &str,
        row_index0: i32,
    ) -> anyhow::Result<()> {
        let sheet_id = self.sheet_id(spreadsheet_id, sheet_name).await?;
        self.batch_update(
            spreadsheet_id,
            vec![json!({
                "insertDimension": {
                    "range": {
                        "sheetId": sheet_id,
                        "dimension": "ROWS",
                        "startIndex": row_index0,
                        "endIndex": row_index0 + 1
                    },
                    "inheritFromBefore": false
                }
            })],
        )
        .await
    }

    pub async fn add_hidden_sheet(&self, spreadsheet_id: &str, title: &str) -> anyhow::Result<()> {
        self.batch_update(
            spreadsheet_id,
            vec![json!({
                "addSheet": {
                    "properties": { "title": title, "hidden": true }
                }
            })],
        )
        .await
    }

    pub async fn grid_data(
        &self,
        spreadsheet_id: &str,
        range: &str,
        fields: &str,
    ) -> anyhow::Result<Vec<RowData>> {
        let url = format!(
            "https://sheets.googleapis.com/v4/spreadsheets/{spreadsheet_id}?ranges={}&includeGridData=true&fields={}",
            urlencoding::encode(range),
            urlencoding::encode(fields)
        );
        let (_, body) = sheets_send_with_retry("Sheet grid read", || {
            let url = url.clone();
            async move {
                let token = self.sa_token().await?;
                Ok(self.http().get(url).bearer_auth(token).send().await?)
            }
        })
        .await?;
        let parsed: Spreadsheet = serde_json::from_str(&body)?;
        Ok(parsed
            .sheets
            .unwrap_or_default()
            .into_iter()
            .next()
            .and_then(|s| s.data)
            .and_then(|d| d.into_iter().next())
            .and_then(|g| g.row_data)
            .unwrap_or_default())
    }
}

const CONNECTOR_LINK_LABELS: [&str; 3] = [
    "Resume Link",
    "Job Description Link",
    "Drive Folder Link",
];
const PHONE_LINK_LABELS: [&str; 3] = [
    "Job Description Link",
    "Resume Link",
    "Drive Folder Link",
];

#[derive(Clone, Copy)]
enum LinkKind {
    Connector,
    Phone,
}

#[derive(Clone)]
pub struct RichTextLinkCell {
    pub column1: i32,
    pub url: String,
    pub label: String,
}

pub fn hyperlink_formula(url: &str, label: &str) -> String {
    let href = url.trim();
    let text = if label.trim().is_empty() { href } else { label.trim() };
    format!(
        "=HYPERLINK(\"{}\",\"{}\")",
        href.replace('"', "\"\""),
        text.replace('"', "\"\"")
    )
}

fn formula_link_request(sheet_id: i64, row1: i32, column1: i32, url: &str, label: &str) -> Option<Value> {
    let href = url.trim();
    if href.is_empty() {
        return None;
    }
    Some(json!({
        "updateCells": {
            "start": {
                "sheetId": sheet_id,
                "rowIndex": row1 - 1,
                "columnIndex": column1 - 1
            },
            "rows": [{
                "values": [{
                    "userEnteredValue": { "formulaValue": hyperlink_formula(href, label) }
                }]
            }],
            "fields": "userEnteredValue"
        }
    }))
}

fn link_request(sheet_id: i64, row1: i32, cell: &RichTextLinkCell) -> Option<Value> {
    let url = cell.url.trim();
    if url.is_empty() {
        return None;
    }
    let label = if cell.label.trim().is_empty() {
        url
    } else {
        cell.label.trim()
    };
    Some(json!({
        "updateCells": {
            "start": {
                "sheetId": sheet_id,
                "rowIndex": row1 - 1,
                "columnIndex": cell.column1 - 1
            },
            "rows": [{
                "values": [{
                    "userEnteredValue": { "stringValue": label },
                    "textFormatRuns": [{
                        "startIndex": 0,
                        "format": {
                            "link": { "uri": url },
                            "foregroundColor": {"red": 0.067, "green": 0.333, "blue": 0.8},
                            "underline": true
                        }
                    }]
                }]
            }],
            "fields": "userEnteredValue,textFormatRuns"
        }
    }))
}

impl GoogleClient {
    pub async fn write_rich_text_hyperlinks(
        &self,
        spreadsheet_id: &str,
        sheet_id: i64,
        row1: i32,
        cells: &[RichTextLinkCell],
    ) -> anyhow::Result<()> {
        let requests: Vec<Value> = cells.iter().filter_map(|c| link_request(sheet_id, row1, c)).collect();
        self.batch_update(spreadsheet_id, requests).await
    }

    pub async fn write_rich_text_hyperlinks_batch(
        &self,
        spreadsheet_id: &str,
        sheet_id: i64,
        rows: &[(i32, Vec<RichTextLinkCell>)],
    ) -> anyhow::Result<usize> {
        let mut requests = Vec::new();
        for (row1, cells) in rows {
            for cell in cells {
                if let Some(req) = link_request(sheet_id, *row1, cell) {
                    requests.push(req);
                }
            }
        }
        let n = requests.len();
        for chunk in requests.chunks(80) {
            self.batch_update(spreadsheet_id, chunk.to_vec()).await?;
        }
        Ok(n)
    }

    pub async fn write_hidden_urls(
        &self,
        spreadsheet_id: &str,
        sheet_name: &str,
        row1: i32,
        start_idx0: usize,
        urls: [&str; 3],
    ) -> anyhow::Result<()> {
        let start = crate::headers::a1_col(start_idx0);
        let end = crate::headers::a1_col(start_idx0 + 2);
        self.sheets_values_update(
            spreadsheet_id,
            &format!("'{sheet_name}'!{start}{row1}:{end}{row1}"),
            vec![vec![
                json!(urls[0].trim()),
                json!(urls[1].trim()),
                json!(urls[2].trim()),
            ]],
            "RAW",
        )
        .await
    }

    pub async fn write_connector_hidden_urls(
        &self,
        spreadsheet_id: &str,
        sheet_name: &str,
        row1: i32,
        resume_url: &str,
        jd_url: &str,
        folder_url: &str,
    ) -> anyhow::Result<()> {
        let headers = self.load_headers(spreadsheet_id, sheet_name).await?;
        let start = headers.hidden_run(3).into_iter().next().unwrap_or_else(|| headers.last_named_idx() + 1);
        self.write_hidden_urls(
            spreadsheet_id,
            sheet_name,
            row1,
            start,
            [resume_url, jd_url, folder_url],
        )
        .await
    }

    /// Re-link Resume / JD / Folder cells that lost their link (an insertRow
    /// flattens formulas) from the hidden URL backup to the right of the last
    /// named header. See `links_to_restore` for what may be filled.
    pub async fn restore_connector_hyperlinks(
        &self,
        spreadsheet_id: &str,
        sheet_name: &str,
        data_start_row: i32,
    ) -> anyhow::Result<usize> {
        self.restore_links_from_hidden(
            spreadsheet_id,
            sheet_name,
            data_start_row,
            LinkKind::Connector,
        )
        .await
    }

    pub async fn write_phone_calls_hidden_urls(
        &self,
        spreadsheet_id: &str,
        sheet_name: &str,
        row1: i32,
        jd_url: &str,
        resume_url: &str,
        folder_url: &str,
    ) -> anyhow::Result<()> {
        let headers = self.load_headers(spreadsheet_id, sheet_name).await?;
        let start = headers.hidden_run(3).into_iter().next().unwrap_or_else(|| headers.last_named_idx() + 1);
        self.write_hidden_urls(
            spreadsheet_id,
            sheet_name,
            row1,
            start,
            [jd_url, resume_url, folder_url],
        )
        .await
    }

    /// First free row on the Phone calls tab: one below the last row that names a
    /// Candidate or Client (by header). Other columns don't count — a Support
    /// value dragged down once pushed new bookings 30 rows below the data.
    pub async fn phone_calls_next_append_row(
        &self,
        spreadsheet_id: &str,
        sheet_name: &str,
        data_start_row: i32,
    ) -> anyhow::Result<i32> {
        let headers = self.load_headers(spreadsheet_id, sheet_name).await.ok();
        let rows = self
            .sheets_values_get(
                spreadsheet_id,
                &format!("'{sheet_name}'!A{data_start_row}:Z"),
                None,
            )
            .await?;
        let mut last_booking = 0i32;
        for (i, row) in rows.iter().enumerate() {
            let filled = if let Some(h) = &headers {
                let cand = h.get(row, crate::headers::col::CANDIDATE);
                let client = h.get(row, crate::headers::col::CLIENT);
                !cand.is_empty() || !client.is_empty()
            } else {
                row.iter().any(|c| !c.trim().is_empty())
            };
            if filled {
                last_booking = i as i32 + 1;
            }
        }
        Ok(data_start_row + last_booking)
    }

    /// Re-link JD / Resume / Folder cells that lost their link from the hidden
    /// URL backup to the right of the last named header. See `links_to_restore`
    /// for what may be filled.
    pub async fn restore_phone_calls_hyperlinks(
        &self,
        spreadsheet_id: &str,
        sheet_name: &str,
        data_start_row: i32,
    ) -> anyhow::Result<usize> {
        self.restore_links_from_hidden(
            spreadsheet_id,
            sheet_name,
            data_start_row,
            LinkKind::Phone,
        )
        .await
    }

    async fn restore_links_from_hidden(
        &self,
        spreadsheet_id: &str,
        sheet_name: &str,
        data_start_row: i32,
        kind: LinkKind,
    ) -> anyhow::Result<usize> {
        let headers = self.load_headers(spreadsheet_id, sheet_name).await?;
        let vis_idx = visible_link_indexes(&headers, kind);
        let present: Vec<usize> = vis_idx.iter().copied().flatten().collect();
        if present.is_empty() {
            return Ok(0);
        }
        let vis_min = *present.iter().min().unwrap();
        let vis_max = *present.iter().max().unwrap();
        let hidden_idxs = hidden_url_indexes(&headers, kind);
        let key_idxs = booking_key_indexes(&headers, kind);
        let hid_start = crate::headers::a1_col(hidden_idxs[0]);
        let hid_end = crate::headers::a1_col(hidden_idxs[2]);
        let vis_start = crate::headers::a1_col(vis_min);
        let vis_end = crate::headers::a1_col(vis_max);
        let key_last = crate::headers::a1_col(
            *key_idxs.iter().max().unwrap_or(&headers.last_named_idx()),
        );
        let labels = match kind {
            LinkKind::Connector => CONNECTOR_LINK_LABELS,
            LinkKind::Phone => PHONE_LINK_LABELS,
        };

        // Nothing past the last backup URL can be restored, so it sizes the scan.
        let hidden = self
            .sheets_values_get(
                spreadsheet_id,
                &format!("'{sheet_name}'!{hid_start}{data_start_row}:{hid_end}"),
                None,
            )
            .await?;
        if hidden.is_empty() {
            return Ok(0);
        }
        let last_row = data_start_row + hidden.len() as i32 - 1;
        let keys = self
            .sheets_values_get(
                spreadsheet_id,
                &format!("'{sheet_name}'!A{data_start_row}:{key_last}{last_row}"),
                None,
            )
            .await?;
        // Grid data, not FORMULA values: a rich-text link reads back as its bare
        // label through FORMULA and would look unlinked.
        let grid = self
            .grid_data(
                spreadsheet_id,
                &format!("'{sheet_name}'!{vis_start}{data_start_row}:{vis_end}{last_row}"),
                "sheets(data(rowData(values(hyperlink,userEnteredValue,textFormatRuns))))",
            )
            .await?;

        let rows = hidden.len();
        let visible: Vec<[String; 3]> = (0..rows)
            .map(|i| {
                let cells = grid.get(i).and_then(|r| r.values.as_ref());
                std::array::from_fn(|c| {
                    match vis_idx[c] {
                        Some(idx) => {
                            let offset = idx.saturating_sub(vis_min);
                            url_from_grid_cell(cells.and_then(|v| v.get(offset)))
                        }
                        None => String::new(),
                    }
                })
            })
            .collect();
        let backup: Vec<[String; 3]> = hidden
            .iter()
            .map(|r| std::array::from_fn(|c| r.get(c).map(|v| v.trim().to_string()).unwrap_or_default()))
            .collect();
        let is_booking: Vec<bool> = (0..rows)
            .map(|i| {
                key_idxs.iter().any(|&k| {
                    keys.get(i)
                        .and_then(|r| r.get(k))
                        .is_some_and(|v| !v.trim().is_empty())
                })
            })
            .collect();

        let sheet_id = self.sheet_id(spreadsheet_id, sheet_name).await?;
        let requests: Vec<Value> = links_to_restore(&visible, &backup, &is_booking)
            .into_iter()
            .filter_map(|(i, c, url)| {
                let col = vis_idx[c]?;
                formula_link_request(
                    sheet_id,
                    data_start_row + i as i32,
                    col as i32 + 1,
                    &url,
                    labels[c],
                )
            })
            .collect();
        let restored = requests.len();
        for chunk in requests.chunks(80) {
            self.batch_update(spreadsheet_id, chunk.to_vec()).await?;
        }
        Ok(restored)
    }
}

fn visible_link_indexes(headers: &crate::headers::SheetHeaders, kind: LinkKind) -> [Option<usize>; 3] {
    match kind {
        LinkKind::Connector => [
            headers.idx(crate::headers::col::RESUME),
            headers.idx(crate::headers::col::JD),
            headers.idx(crate::headers::col::FOLDER),
        ],
        LinkKind::Phone => [
            headers.idx(crate::headers::col::JD),
            headers.idx(crate::headers::col::RESUME),
            headers.idx(crate::headers::col::FOLDER),
        ],
    }
}

fn hidden_url_indexes(headers: &crate::headers::SheetHeaders, _kind: LinkKind) -> [usize; 3] {
    let run = headers.hidden_run(3);
    if run.len() >= 3 {
        return [run[0], run[1], run[2]];
    }
    let start = headers.last_named_idx() + 1;
    [start, start + 1, start + 2]
}

fn booking_key_indexes(headers: &crate::headers::SheetHeaders, kind: LinkKind) -> Vec<usize> {
    let mut keys = Vec::new();
    if let Some(i) = headers.idx(crate::headers::col::CANDIDATE) {
        keys.push(i);
    }
    if let Some(i) = headers.idx(crate::headers::col::CLIENT) {
        keys.push(i);
    }
    if matches!(kind, LinkKind::Phone) {
        if let Some(i) = headers.idx(crate::headers::col::DATE) {
            keys.push(i);
        }
    }
    keys
}

/// Same file however the link is spelled (`open?id=`, `/file/d/`, `/folders/`).
pub fn link_identity(url: &str) -> String {
    let u = url.trim();
    for marker in ["id=", "/d/", "/folders/"] {
        if let Some(pos) = u.find(marker) {
            let id: String = u[pos + marker.len()..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                .collect();
            if id.len() >= 20 {
                return id;
            }
        }
    }
    u.to_string()
}

/// Which visible link cells may be filled from their hidden backup, as
/// `(row index, link index, url)`. Links are ordered with the two uploaded
/// files first and the Drive folder last, on both tab layouts.
///
/// The visible link is the source of truth; the backup only fills a cell that
/// has no link at all. Rows get moved by staff and by jobs that rewrite the
/// visible columns only, so a backup can belong to a different booking than
/// the row it now sits on. Letting it overwrite put one candidate's resume and
/// JD on another candidate's booking (Phone calls, 2026-09).
///
/// A backup is also refused outright when its resume or JD is already linked
/// on another row — the telltale of a backup left behind by a row that moved.
/// Only the files are checked: every booking uploads its own, but a repeat
/// booking with the same client and stage legitimately shares the folder.
pub fn links_to_restore(
    visible: &[[String; 3]],
    backup: &[[String; 3]],
    is_booking: &[bool],
) -> Vec<(usize, usize, String)> {
    let mut file_owner: HashMap<String, usize> = HashMap::new();
    for (i, row) in visible.iter().enumerate() {
        for url in row[..2].iter().filter(|u| !u.is_empty()) {
            file_owner.entry(link_identity(url)).or_insert(i);
        }
    }
    let is_url = |u: &str| u.starts_with("http://") || u.starts_with("https://");
    let mut out = Vec::new();
    for (i, row) in backup.iter().enumerate() {
        if !is_booking.get(i).copied().unwrap_or(false) {
            continue;
        }
        let foreign = row[..2]
            .iter()
            .filter(|u| is_url(u))
            .any(|u| file_owner.get(&link_identity(u)).is_some_and(|&owner| owner != i));
        if foreign {
            continue;
        }
        for (c, url) in row.iter().enumerate() {
            let already = visible.get(i).is_some_and(|v| !v[c].is_empty());
            if is_url(url) && !already {
                out.push((i, c, url.clone()));
            }
        }
    }
    out
}

#[cfg(test)]
mod restore_policy_tests {
    use super::{link_identity, links_to_restore};

    fn row(a: &str, b: &str, c: &str) -> [String; 3] {
        [a.into(), b.into(), c.into()]
    }
    const SANKET: &str = "https://drive.google.com/open?id=1uUZ83SjEIMEZ09XWKpLZn54jrOnvQaZd";
    const MONIKA: &str = "https://drive.google.com/open?id=1bL1Q088TUtKu96Y1vnD62fnYuaHtbz-z";

    /// The 2026-09 incident: row 2's backup was Sanket's (left behind when his
    /// row moved up) while row 2 visibly linked Monika's. Nothing may change.
    #[test]
    fn an_existing_link_is_never_replaced() {
        let visible = vec![row(SANKET, "", ""), row(MONIKA, "", "")];
        let backup = vec![row(SANKET, "", ""), row(SANKET, "", "")];
        assert!(links_to_restore(&visible, &backup, &[true, true]).is_empty());
    }

    #[test]
    fn a_flattened_link_is_refilled_from_its_own_backup() {
        let visible = vec![row("", "", "")];
        let backup = vec![row(MONIKA, "", "")];
        assert_eq!(
            links_to_restore(&visible, &backup, &[true]),
            vec![(0, 0, MONIKA.to_string())]
        );
    }

    /// A stale backup on a row whose link is gone must not borrow a file that
    /// another booking visibly owns — even spelled as a different URL.
    #[test]
    fn a_backup_owned_by_another_row_is_refused() {
        let other_spelling = "https://drive.google.com/file/d/1uUZ83SjEIMEZ09XWKpLZn54jrOnvQaZd/view";
        let visible = vec![row(other_spelling, "", ""), row("", "", "")];
        let backup = vec![row("", "", ""), row(SANKET, "", "")];
        assert!(links_to_restore(&visible, &backup, &[true, true]).is_empty());
    }

    /// Once one of a backup's files is proven foreign, none of it is trusted.
    #[test]
    fn a_foreign_backup_is_refused_as_a_whole() {
        let folder = "https://drive.google.com/drive/folders/1W89xhcFPnKMoJ8F3K7CB6JDr3STfjcnk";
        let visible = vec![row(SANKET, "", ""), row("", "", "")];
        let backup = vec![row("", "", ""), row(SANKET, "", folder)];
        assert!(links_to_restore(&visible, &backup, &[true, true]).is_empty());
    }

    /// Nirbhik booked Amerilife phone calls twice: new files, same folder.
    #[test]
    fn a_repeat_booking_may_share_the_folder() {
        let folder = "https://drive.google.com/drive/folders/1_81DkLXt0qI9e4PAGmkoDWDGw08ubC3Q";
        let visible = vec![row(SANKET, MONIKA, folder), row("", "", "")];
        let backup = vec![
            row("", "", ""),
            row("https://drive.google.com/open?id=1mJ-85v68Rxr5CDmv4rgA38kx26p0_Sb_", "https://drive.google.com/open?id=1fDFtEBbGvhJq3GzvUoCDRNv2dUbQ5gYO", folder),
        ];
        assert_eq!(links_to_restore(&visible, &backup, &[true, true]).len(), 3);
    }

    #[test]
    fn rows_without_a_booking_are_left_alone() {
        let visible = vec![row("", "", "")];
        let backup = vec![row(SANKET, "", "")];
        assert!(links_to_restore(&visible, &backup, &[false]).is_empty());
    }

    #[test]
    fn drive_links_compare_by_file_id() {
        assert_eq!(
            link_identity("https://drive.google.com/drive/folders/1W89xhcFPnKMoJ8F3K7CB6JDr3STfjcnk?usp=sharing"),
            "1W89xhcFPnKMoJ8F3K7CB6JDr3STfjcnk"
        );
        assert_eq!(link_identity(SANKET), link_identity("https://drive.google.com/file/d/1uUZ83SjEIMEZ09XWKpLZn54jrOnvQaZd/view"));
    }
}

#[cfg(test)]
mod append_row_tests {
    use crate::google::GoogleClient;

    /// Read-only: prove connector_next_append_row lands at the BOTTOM of each
    /// live POC tab. Run with:
    ///   cargo test --manifest-path backend/Cargo.toml -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn next_append_row_is_bottom_of_each_tab() {
        let cfg = crate::config::load();
        let connector = cfg.connector_spreadsheet_id.clone();
        let data_start = cfg.connector_data_start_row;
        let phone_ss = cfg.data_interview_spreadsheet_id.clone();
        let phone_tab = cfg.phone_calls_sheet.clone();
        let g = GoogleClient::new(cfg);

        for tab in ["Prasanna", "Sajit", "Saksham"] {
            let used = g
                .sheets_values_get(&connector, &format!("'{tab}'!A{data_start}:R"), None)
                .await
                .unwrap();
            let trailing_blank = used
                .iter()
                .rev()
                .take_while(|r| r.iter().all(|c| c.trim().is_empty()))
                .count() as i32;
            // First row past the last non-empty data row.
            let expected = data_start + used.len() as i32 - trailing_blank;
            let last_used = expected - 1;
            let next = g
                .connector_next_append_row(&connector, tab, data_start, "R")
                .await
                .unwrap();
            println!("{tab}: last used row = {last_used}, next append = {next}");
            assert_eq!(next, expected, "{tab} must append directly below data");
        }

        let next_phone = g
            .connector_next_append_row(&phone_ss, &phone_tab, 2, "S")
            .await
            .unwrap();
        println!("{phone_tab}: next append = {next_phone}");
        assert!(next_phone >= 2);
    }
}
