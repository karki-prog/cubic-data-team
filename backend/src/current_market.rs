//! Current_Market (Data Candidate sheet), with columns resolved by header name.
//!
//! Staff insert columns on this tab. On 2026-09-11 one landed left of "Nepal POC",
//! and every fixed-index reader shifted by one: bookings got POC "Remote" (the
//! Marketing Location) and no email (LinkedIn sat where the email used to be).
//! Never index this tab by position — load it through here.

use std::time::Duration;

use crate::AppState;

const RANGE: &str = "Current_Market!A1:Z";

pub struct Market {
    /// Every row below the header (row 2 is a blank spacer; data starts row 3).
    pub rows: Vec<Vec<String>>,
    pub name: Option<usize>,
    pub email: Option<usize>,
    pub poc: Option<usize>,
    pub visa: Option<usize>,
    pub location: Option<usize>,
    /// "Otter Rating" dropdown (A / B+ / B / C / D / F).
    pub otter: Option<usize>,
}

impl Market {
    pub fn cell<'a>(&self, row: &'a [String], col: Option<usize>) -> &'a str {
        col.and_then(|i| row.get(i)).map(|s| s.trim()).unwrap_or("")
    }
}

pub async fn load(state: &AppState, cache: Option<Duration>) -> anyhow::Result<Market> {
    let id = &state.cfg.data_candidate_spreadsheet_id;
    let mut rows = match cache {
        Some(ttl) => {
            state
                .google
                .sheets_values_get_cached(id, RANGE, Some("FORMATTED_VALUE"), ttl)
                .await?
        }
        None => {
            state
                .google
                .sheets_values_get(id, RANGE, Some("FORMATTED_VALUE"))
                .await?
        }
    };
    if rows.is_empty() {
        anyhow::bail!("Current_Market returned no rows");
    }
    let headers: Vec<String> = rows
        .remove(0)
        .iter()
        .map(|h| h.trim().to_lowercase())
        .collect();
    let find = |needle: &str| headers.iter().position(|h| h.contains(needle));
    let name = find("full name");
    if name.is_none() {
        anyhow::bail!("Current_Market header has no \"Full Name\" column: {headers:?}");
    }
    Ok(Market {
        name,
        email: find("email"),
        poc: find("nepal poc"),
        // "Status" is the visa status; match it exactly so "Stage" etc. never hit.
        visa: headers.iter().position(|h| h == "status"),
        location: find("marketing location"),
        otter: find("otter"),
        rows,
    })
}
