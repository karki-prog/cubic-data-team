use crate::types::{BackendState, SheetKind};
use crate::AppState;

const HEADERS: [&str; 7] = [
    "key",
    "kind",
    "status",
    "calendarEventId",
    "emailStatus",
    "cubicSynced",
    "updatedAt",
];

async fn ensure_state_sheet(state: &AppState) -> anyhow::Result<()> {
    let titles = state
        .google
        .sheet_titles_cached(&state.cfg.connector_spreadsheet_id)
        .await?;
    if titles.iter().any(|t| t == &state.cfg.backend_state_sheet) {
        return Ok(());
    }
    state
        .google
        .add_hidden_sheet(&state.cfg.connector_spreadsheet_id, &state.cfg.backend_state_sheet)
        .await?;
    state
        .google
        .sheets_values_update(
            &state.cfg.connector_spreadsheet_id,
            &format!("'{}'!A1:G1", state.cfg.backend_state_sheet),
            vec![HEADERS.iter().map(|h| serde_json::json!(h)).collect()],
            "RAW",
        )
        .await
}

pub async fn load_backend_state(state: &AppState) -> anyhow::Result<std::collections::HashMap<String, BackendState>> {
    ensure_state_sheet(state).await?;
    let rows = state
        .google
        .sheets_values_get(
            &state.cfg.connector_spreadsheet_id,
            &format!("'{}'!A1:G", state.cfg.backend_state_sheet),
            None,
        )
        .await?;
    let headers = crate::headers::SheetHeaders::detect(&rows);
    let header_idx = (headers.row_1 as usize).saturating_sub(1);
    let mut map = std::collections::HashMap::new();
    for (i, row) in rows.iter().enumerate() {
        if i == header_idx {
            continue;
        }
        let key = headers.get(row, &["key"]).to_string();
        let key = if key.is_empty() {
            row.first().cloned().unwrap_or_default().trim().to_string()
        } else {
            key
        };
        if key.is_empty() {
            continue;
        }
        map.insert(
            key.clone(),
            BackendState {
                key,
                kind: SheetKind::from(headers.get(row, &["kind"])),
                status: headers.get_owned(row, &["status"]),
                calendar_event_id: headers.get_owned(row, &["calendareventid", "calendar event id"]),
                email_status: headers.get_owned(row, &["emailstatus", "email status"]),
                cubic_synced: headers
                    .get(row, &["cubicsynced", "cubic synced"])
                    .eq_ignore_ascii_case("true"),
            },
        );
    }
    Ok(map)
}

pub async fn save_backend_state(
    state: &AppState,
    states: &std::collections::HashMap<String, BackendState>,
) -> anyhow::Result<()> {
    ensure_state_sheet(state).await?;
    let now = chrono::Utc::now().to_rfc3339();
    let values: Vec<Vec<serde_json::Value>> = states
        .values()
        .map(|s| {
            vec![
                serde_json::json!(s.key),
                serde_json::json!(s.kind.as_str()),
                serde_json::json!(s.status),
                serde_json::json!(s.calendar_event_id),
                serde_json::json!(s.email_status),
                serde_json::json!(if s.cubic_synced { "true" } else { "false" }),
                serde_json::json!(now),
            ]
        })
        .collect();
    state
        .google
        .sheets_values_clear(
            &state.cfg.connector_spreadsheet_id,
            &format!("'{}'!A2:G", state.cfg.backend_state_sheet),
        )
        .await?;
    if !values.is_empty() {
        let end = values.len() + 1;
        state
            .google
            .sheets_values_update(
                &state.cfg.connector_spreadsheet_id,
                &format!("'{}'!A2:G{end}", state.cfg.backend_state_sheet),
                values,
                "RAW",
            )
            .await?;
    }
    Ok(())
}

pub async fn remember_pending_booking(
    state: &AppState,
    key: &str,
    kind: SheetKind,
    status: &str,
) -> anyhow::Result<()> {
    let mut map = load_backend_state(state).await?;
    map.insert(
        key.to_string(),
        BackendState {
            key: key.into(),
            kind,
            status: if status.is_empty() {
                "Pending".into()
            } else {
                status.into()
            },
            calendar_event_id: String::new(),
            email_status: String::new(),
            cubic_synced: false,
        },
    );
    save_backend_state(state, &map).await
}
