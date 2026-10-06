use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
use chrono::Datelike;
use serde_json::json;

use crate::auth::require_session;
use crate::dates::{
    add_days_local, chicago_today_parts, format_date_key_mdy, format_day_label, mins_to_hhmm,
    parse_date_key,
};
use crate::error::AppError;
use crate::types::SheetKind;
use crate::AppState;

pub const PUBLIC_HOLIDAYS: &[&str] = &[
    "1/1/2026",
    "1/19/2026",
    "2/16/2026",
    "5/25/2026",
    "6/19/2026",
    "7/4/2026",
    "9/7/2026",
    "10/12/2026",
    "11/11/2026",
    "11/26/2026",
    "12/25/2026",
    "1/1/2027",
    "1/18/2027",
    "2/15/2027",
    "5/31/2027",
    "6/19/2027",
    "7/4/2027",
    "9/6/2027",
    "10/11/2027",
    "11/11/2027",
    "11/25/2027",
    "12/25/2027",
];

const AVAILABILITY_HORIZON_DAYS: i64 = 14;
pub(crate) const MEETING_CAPACITY_APPT: i32 = 3;
pub(crate) const MEETING_CAPACITY_PHONE: i32 = 1;
const DAY_START_MIN: i32 = 8 * 60;
const LUNCH_START_MIN: i32 = 12 * 60;
const LUNCH_END_MIN: i32 = 13 * 60;
const WEEKDAY_END_MIN: i32 = 17 * 60;
const FRIDAY_END_MIN: i32 = 15 * 60;
const EMERGENCY_SLOT_DURATION_MIN: i32 = 60;
const PHONE_SLOT_MINUTES: i32 = 30;
const PHONE_WINDOWS: [(i32, i32); 2] = [(10 * 60, 12 * 60), (13 * 60, 15 * 60)];
const MONTH_NAMES: [&str; 12] = [
    "January", "February", "March", "April", "May", "June", "July", "August", "September",
    "October", "November", "December",
];

fn slot_block(
    start: i32,
    end: i32,
    typ: &str,
    min_free: i32,
    capacity: i32,
    label: Option<&str>,
) -> serde_json::Value {
    let mut value = json!({
        "start": start,
        "end": end,
        "type": typ,
        "minFree": min_free,
        "capacity": capacity,
        "inputTime": mins_to_hhmm(start)
    });
    if let Some(label) = label {
        value["label"] = json!(label);
    }
    value
}

#[derive(Clone)]
struct SlotBooking {
    date_str: String,
    start_min: i32,
    end_min: i32,
}

fn normalize_date(date_str: &str) -> String {
    let parts: Vec<&str> = date_str.trim().split('/').collect();
    if parts.len() != 3 {
        return date_str.trim().to_string();
    }
    let m: i32 = parts[0].parse().unwrap_or(0);
    let d: i32 = parts[1].parse().unwrap_or(0);
    let mut y: i32 = parts[2].parse().unwrap_or(0);
    if y < 100 {
        y += 2000;
    }
    if m == 0 || d == 0 {
        return date_str.trim().to_string();
    }
    format!("{m}/{d}/{y}")
}

fn parse_date_cell(raw: &str) -> Option<String> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    Some(normalize_date(s))
}

fn parse_time_cst_to_minutes(raw: &str) -> Option<i32> {
    crate::dates::parse_time_minutes(raw)
}

fn parse_duration(raw: &str) -> Option<i32> {
    let s = raw.trim().to_lowercase();
    if s.is_empty() {
        return None;
    }
    let hr = regex::Regex::new(r"(\d+)\s*hr")
        .unwrap()
        .captures(&s)
        .and_then(|c| c[1].parse().ok())
        .unwrap_or(0);
    let min = regex::Regex::new(r"(\d+)\s*min")
        .unwrap()
        .captures(&s)
        .and_then(|c| c[1].parse().ok())
        .unwrap_or(0);
    let total = hr * 60 + min;
    if total > 0 {
        Some(total)
    } else {
        None
    }
}

fn is_holiday(date_key: &str) -> bool {
    PUBLIC_HOLIDAYS.contains(&date_key)
}

pub(crate) fn meeting_capacity(kind: SheetKind) -> i32 {
    match kind {
        SheetKind::Interview => MEETING_CAPACITY_APPT,
        SheetKind::Phone => MEETING_CAPACITY_PHONE,
    }
}

pub(crate) fn regular_slot_end(kind: SheetKind, start_min: i32) -> i32 {
    start_min + slot_block_minutes(kind)
}

/// One booking grid cell. A slot hold reserves exactly this — never the meeting's
/// full duration — so one long booking doesn't grey out the rest of the day.
pub(crate) fn slot_block_minutes(kind: SheetKind) -> i32 {
    match kind {
        SheetKind::Interview => 60,
        SheetKind::Phone => PHONE_SLOT_MINUTES,
    }
}

pub(crate) fn is_valid_regular_slot(kind: SheetKind, date: chrono::NaiveDate, start_min: i32) -> bool {
    let weekday = date.weekday().num_days_from_sunday();
    let date_key = format_date_key_mdy(date);
    if weekday == 0 || weekday == 6 || is_holiday(&date_key) {
        return false;
    }
    match kind {
        SheetKind::Interview => {
            let day_end = if weekday == 5 {
                FRIDAY_END_MIN
            } else {
                WEEKDAY_END_MIN
            };
            if start_min < DAY_START_MIN || start_min >= day_end {
                return false;
            }
            if start_min >= LUNCH_START_MIN && start_min < LUNCH_END_MIN {
                return false;
            }
            start_min % 60 == 0
        }
        SheetKind::Phone => PHONE_WINDOWS.iter().any(|(win_start, win_end)| {
            start_min >= *win_start
                && start_min + PHONE_SLOT_MINUTES <= *win_end
                && (start_min - *win_start) % PHONE_SLOT_MINUTES == 0
        }),
    }
}

/// Any CST minute in working hours (form time picker). Grid slots stay on `is_valid_regular_slot`.
pub(crate) fn is_valid_start_time(kind: SheetKind, date: chrono::NaiveDate, start_min: i32) -> bool {
    let weekday = date.weekday().num_days_from_sunday();
    let date_key = format_date_key_mdy(date);
    if weekday == 0 || weekday == 6 || is_holiday(&date_key) {
        return false;
    }
    match kind {
        SheetKind::Interview => {
            let day_end = if weekday == 5 {
                FRIDAY_END_MIN
            } else {
                WEEKDAY_END_MIN
            };
            if start_min < DAY_START_MIN || start_min >= day_end {
                return false;
            }
            !(start_min >= LUNCH_START_MIN && start_min < LUNCH_END_MIN)
        }
        // Same weekday hours as interviews: the form time picker is free, not 30-min aligned.
        SheetKind::Phone => {
            let day_end = if weekday == 5 {
                FRIDAY_END_MIN
            } else {
                WEEKDAY_END_MIN
            };
            if start_min < DAY_START_MIN || start_min >= day_end {
                return false;
            }
            !(start_min >= LUNCH_START_MIN && start_min < LUNCH_END_MIN)
        }
    }
}

pub(crate) fn peak_occupancy(
    intervals: &[(String, i32, i32)],
    date_key: &str,
    range_start: i32,
    range_end: i32,
) -> i32 {
    if range_end <= range_start {
        return 0;
    }
    let mut peak = 0;
    let mut t = range_start;
    while t < range_end {
        let mut occ = 0i32;
        for (d, start, end) in intervals {
            if d == date_key && t >= *start && t < *end {
                occ += 1;
            }
        }
        peak = peak.max(occ);
        t += 5;
    }
    peak
}

fn parse_mdy_date(date_key: &str) -> Option<chrono::NaiveDate> {
    let iso = parse_date_key(date_key);
    chrono::NaiveDate::parse_from_str(&iso, "%Y-%m-%d").ok()
}

pub(crate) async fn sheet_and_queue_intervals(
    state: &AppState,
    kind: SheetKind,
    date_key: &str,
) -> anyhow::Result<Vec<(String, i32, i32)>> {
    let mut intervals = match kind {
        SheetKind::Interview => {
            let date = parse_mdy_date(date_key)
                .ok_or_else(|| anyhow::anyhow!("Invalid meeting date."))?;
            let (bookings, _) = fetch_appointment_bookings(state, date, date).await?;
            bookings
                .into_iter()
                .map(|b| (b.date_str, b.start_min, b.end_min))
                .collect::<Vec<_>>()
        }
        SheetKind::Phone => fetch_phone_bookings(state)
            .await?
            .into_iter()
            .map(|b| (b.date_str, b.start_min, b.end_min))
            .collect(),
    };
    intervals.extend(crate::booking::queued_slot_intervals(state, kind).await);
    Ok(intervals)
}

async fn merge_pending_intervals(
    state: &AppState,
    kind: SheetKind,
    bookings: &mut Vec<SlotBooking>,
) {
    for (date_str, start_min, end_min) in state.holds.intervals(kind, None).await {
        bookings.push(SlotBooking {
            date_str,
            start_min,
            end_min,
        });
    }
    for (date_str, start_min, end_min) in crate::booking::queued_slot_intervals(state, kind).await {
        bookings.push(SlotBooking {
            date_str,
            start_min,
            end_min,
        });
    }
}

fn bookings_fingerprint(bookings: &[SlotBooking]) -> String {
    let mut parts: Vec<String> = bookings
        .iter()
        .map(|b| format!("{}|{}|{}", b.date_str, b.start_min, b.end_min))
        .collect();
    parts.sort();
    parts.join(";")
}

async fn fetch_appointment_bookings(
    state: &AppState,
    start: chrono::NaiveDate,
    end: chrono::NaiveDate,
) -> anyhow::Result<(Vec<SlotBooking>, Vec<String>)> {
    let titles = state
        .google
        .sheet_titles_cached(&state.cfg.appointment_spreadsheet_id)
        .await?;
    let existing: std::collections::HashSet<_> = titles
        .into_iter()
        .filter(|t| regex::Regex::new(r"^[A-Za-z]+_\d{4}$").unwrap().is_match(t))
        .collect();
    let mut bookings = Vec::new();
    let mut sheets_used = Vec::new();
    let mut y = start.year();
    let mut m = start.month0();
    let end_y = end.year();
    let end_m = end.month0();
    while y < end_y || (y == end_y && m <= end_m) {
        let name = format!("{}_{y}", MONTH_NAMES[m as usize]);
        if existing.contains(&name) {
            sheets_used.push(name.clone());
            let rows = state
                .google
                .sheets_values_get_cached(
                    &state.cfg.appointment_spreadsheet_id,
                    &format!("'{name}'!A1:Z"),
                    Some("FORMATTED_VALUE"),
                    std::time::Duration::from_secs(25),
                )
                .await?;
            let headers = crate::headers::SheetHeaders::detect(&rows);
            let header_idx = (headers.row_1 as usize).saturating_sub(1);
            for (i, row) in rows.iter().enumerate() {
                if i == header_idx {
                    continue;
                }
                let Some(date_str) = parse_date_cell(headers.get(row, crate::headers::col::DATE)) else {
                    continue;
                };
                let Some(start_min) = parse_time_cst_to_minutes(headers.get(row, crate::headers::col::TIME)) else {
                    continue;
                };
                let Some(duration) = parse_duration(headers.get(row, crate::headers::col::DURATION)) else {
                    continue;
                };
                bookings.push(SlotBooking {
                    date_str,
                    start_min,
                    end_min: start_min + duration,
                });
            }
        }
        m += 1;
        if m > 11 {
            m = 0;
            y += 1;
        }
    }
    Ok((bookings, sheets_used))
}

fn build_appointment_days(
    start: chrono::NaiveDate,
    end: chrono::NaiveDate,
    bookings: &[SlotBooking],
) -> Vec<serde_json::Value> {
    let mut days = Vec::new();
    let mut current = start;
    while current <= end {
        let weekday = current.weekday().num_days_from_sunday();
        let date_key = format_date_key_mdy(current);
        if weekday == 0 || weekday == 6 || is_holiday(&date_key) {
            current += chrono::Duration::days(1);
            continue;
        }
        let day_end = if weekday == 5 {
            FRIDAY_END_MIN
        } else {
            WEEKDAY_END_MIN
        };
        let day_bookings: Vec<_> = bookings.iter().filter(|b| b.date_str == date_key).collect();
        let mut occupancy = std::collections::HashMap::new();
        let mut m = DAY_START_MIN;
        while m < day_end {
            occupancy.insert(m, 0);
            m += 5;
        }
        for b in &day_bookings {
            let mut t = DAY_START_MIN;
            while t < day_end {
                if t >= b.start_min && t < b.end_min {
                    *occupancy.entry(t).or_insert(0) += 1;
                }
                t += 5;
            }
        }
        let mut blocks = Vec::new();
        let mut h = DAY_START_MIN;
        while h < day_end {
            let block_end = (h + 60).min(day_end);
            if h == LUNCH_START_MIN || (h >= LUNCH_START_MIN && h < LUNCH_END_MIN) {
                h += 60;
                continue;
            }
            let mut min_free = MEETING_CAPACITY_APPT;
            let mut has_data = false;
            let mut t = h;
            while t < block_end {
                if let Some(occ) = occupancy.get(&t) {
                    has_data = true;
                    min_free = min_free.min((MEETING_CAPACITY_APPT - occ).max(0));
                }
                t += 5;
            }
            if !has_data {
                min_free = MEETING_CAPACITY_APPT;
            }
            let typ = if min_free <= 0 {
                "full"
            } else if min_free == MEETING_CAPACITY_APPT {
                "open"
            } else {
                "partial"
            };
            blocks.push(slot_block(
                h,
                block_end,
                typ,
                min_free.max(0),
                MEETING_CAPACITY_APPT,
                None,
            ));
            h += 60;
        }
        blocks.push(slot_block(
            day_end,
            day_end + EMERGENCY_SLOT_DURATION_MIN,
            "emergency",
            1,
            1,
            Some("Emergency"),
        ));
        days.push(json!({
            "label": format_day_label(current),
            "dateIso": format!("{:04}-{:02}-{:02}", current.year(), current.month(), current.day()),
            "blocks": blocks
        }));
        current += chrono::Duration::days(1);
    }
    days
}

async fn fetch_phone_bookings(state: &AppState) -> anyhow::Result<Vec<SlotBooking>> {
    let rows = state
        .google
        .sheets_values_get_cached(
            &state.cfg.data_interview_spreadsheet_id,
            "'Phone calls'!A1:Z",
            Some("FORMATTED_VALUE"),
            std::time::Duration::from_secs(25),
        )
        .await?;
    let headers = crate::headers::SheetHeaders::detect(&rows);
    let header_idx = (headers.row_1 as usize).saturating_sub(1);
    let mut out = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        if i == header_idx {
            continue;
        }
        let status = headers.get(row, crate::headers::col::STATUS);
        if status != "Pending" && status != "Accepted" {
            continue;
        }
        let Some(date_str) = parse_date_cell(headers.get(row, crate::headers::col::DATE)) else {
            continue;
        };
        let Some(start_min) = parse_time_cst_to_minutes(headers.get(row, crate::headers::col::TIME)) else {
            continue;
        };
        let duration = parse_duration(headers.get(row, crate::headers::col::DURATION)).unwrap_or(PHONE_SLOT_MINUTES);
        out.push(SlotBooking {
            date_str,
            start_min,
            end_min: start_min + duration,
        });
    }
    Ok(out)
}

fn build_phone_days(start: chrono::NaiveDate, end: chrono::NaiveDate, bookings: &[SlotBooking]) -> Vec<serde_json::Value> {
    let mut days = Vec::new();
    let mut current = start;
    while current <= end {
        let weekday = current.weekday().num_days_from_sunday();
        let date_key = format_date_key_mdy(current);
        if weekday == 0 || weekday == 6 || is_holiday(&date_key) {
            current += chrono::Duration::days(1);
            continue;
        }
        let day_bookings: Vec<_> = bookings.iter().filter(|b| b.date_str == date_key).collect();
        let mut blocks = Vec::new();
        let day_end = PHONE_WINDOWS.last().map(|w| w.1).unwrap_or(15 * 60);
        for (win_start, win_end) in PHONE_WINDOWS {
            let mut occupancy = std::collections::HashMap::new();
            let mut m = win_start;
            while m < win_end {
                occupancy.insert(m, 0);
                m += 5;
            }
            for b in &day_bookings {
                let mut t = win_start;
                while t < win_end {
                    if t >= b.start_min && t < b.end_min {
                        *occupancy.entry(t).or_insert(0) += 1;
                    }
                    t += 5;
                }
            }
            let mut h = win_start;
            while h < win_end {
                let block_end = (h + PHONE_SLOT_MINUTES).min(win_end);
                let mut min_free = MEETING_CAPACITY_PHONE;
                let mut has_data = false;
                let mut t = h;
                while t < block_end {
                    if let Some(occ) = occupancy.get(&t) {
                        has_data = true;
                        min_free = min_free.min((MEETING_CAPACITY_PHONE - occ).max(0));
                    }
                    t += 5;
                }
                if !has_data {
                    min_free = MEETING_CAPACITY_PHONE;
                }
                let typ = if min_free <= 0 {
                    "full"
                } else if min_free == MEETING_CAPACITY_PHONE {
                    "open"
                } else {
                    "partial"
                };
                blocks.push(slot_block(
                    h,
                    block_end,
                    typ,
                    min_free.max(0),
                    MEETING_CAPACITY_PHONE,
                    None,
                ));
                h += PHONE_SLOT_MINUTES;
            }
        }
        blocks.push(slot_block(
            day_end,
            day_end + EMERGENCY_SLOT_DURATION_MIN,
            "emergency",
            1,
            1,
            Some("Emergency"),
        ));
        days.push(json!({
            "label": format_day_label(current),
            "dateIso": format!("{:04}-{:02}-{:02}", current.year(), current.month(), current.day()),
            "blocks": blocks
        }));
        current += chrono::Duration::days(1);
    }
    days
}

pub async fn appointment_availability(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    require_session(&state.cfg, &headers)?;
    let (y, m, d) = chicago_today_parts();
    let start = add_days_local(y, m, d, 1);
    let end = add_days_local(y, m, d, AVAILABILITY_HORIZON_DAYS);
    let (mut bookings, sheets_used) = fetch_appointment_bookings(&state, start, end)
        .await
        .map_err(AppError::from)?;
    let sheet_count = bookings.len();
    merge_pending_intervals(&state, SheetKind::Interview, &mut bookings).await;
    let days = build_appointment_days(start, end, &bookings);
    Ok(Json(json!({
        "ok": true,
        "generatedAt": chrono::Utc::now().to_rfc3339(),
        "timezone": "America/Chicago",
        "horizonDays": AVAILABILITY_HORIZON_DAYS,
        "bookingCount": sheet_count,
        "sheetsUsed": sheets_used,
        "signature": format!("{}|{}|{}", bookings_fingerprint(&bookings), format_date_key_mdy(start), format_date_key_mdy(end)),
        "days": days
    })))
}

pub async fn phone_call_availability(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    require_session(&state.cfg, &headers)?;
    let (y, m, d) = chicago_today_parts();
    let start = add_days_local(y, m, d, 1);
    let end = add_days_local(y, m, d, AVAILABILITY_HORIZON_DAYS);
    let mut bookings = fetch_phone_bookings(&state).await.map_err(AppError::from)?;
    let sheet_count = bookings.len();
    merge_pending_intervals(&state, SheetKind::Phone, &mut bookings).await;
    let days = build_phone_days(start, end, &bookings);
    Ok(Json(json!({
        "ok": true,
        "generatedAt": chrono::Utc::now().to_rfc3339(),
        "timezone": "America/Chicago",
        "horizonDays": AVAILABILITY_HORIZON_DAYS,
        "bookingCount": sheet_count,
        "signature": format!("{}|{}|{}", bookings_fingerprint(&bookings), format_date_key_mdy(start), format_date_key_mdy(end)),
        "days": days
    })))
}

pub async fn phone_call_candidates(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    require_session(&state.cfg, &headers)?;
    match list_candidate_names(&state).await {
        Ok(candidates) => Ok(Json(json!({ "ok": true, "candidates": candidates }))),
        Err(err) => Ok(Json(json!({
            "ok": false,
            "error": err.to_string(),
            "candidates": []
        }))),
    }
}

pub async fn list_candidate_names(state: &AppState) -> anyhow::Result<Vec<String>> {
    let q = format!(
        "'{}' in parents and mimeType='application/vnd.google-apps.folder' and trashed=false",
        state.cfg.resumes_data_folder_id
    );
    let mut names = Vec::new();
    let mut page = None;
    loop {
        let (files, next) = state
            .google
            .drive_list(&q, "nextPageToken, files(name)", 1000, page.as_deref(), Some("name"))
            .await?;
        for f in files {
            let raw = f.name.unwrap_or_default().trim().replace('_', " ");
            let raw = raw.split_whitespace().collect::<Vec<_>>().join(" ");
            if !raw.is_empty() {
                names.push(raw);
            }
        }
        match next {
            Some(t) if !t.is_empty() => page = Some(t),
            _ => break,
        }
    }
    names.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
    names.dedup();
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::peak_occupancy;
    use super::{is_valid_regular_slot, SheetKind};

    /// Emergency bookings must be accepted even when the chosen time lands on a
    /// normal grid hour — the emergency form is a hold bypass, not an after-hours
    /// clock. Advertised emergency starts (17:00 / Fri 15:00 interview; 15:00 phone)
    /// are also never regular slots.
    #[test]
    fn emergency_times_are_not_confused_with_grid_holds() {
        let tue = chrono::NaiveDate::from_ymd_opt(2026, 9, 8).unwrap();
        let fri = chrono::NaiveDate::from_ymd_opt(2026, 9, 11).unwrap();
        // Regular grid hours still identify as regular (holds still apply there).
        for hour in [8, 9, 10, 11, 13, 14, 15, 16] {
            assert!(is_valid_regular_slot(SheetKind::Interview, tue, hour * 60));
        }
        // Advertised emergency starts are outside the regular grid.
        assert!(!is_valid_regular_slot(SheetKind::Interview, tue, 17 * 60));
        assert!(!is_valid_regular_slot(SheetKind::Interview, fri, 15 * 60));
        assert!(!is_valid_regular_slot(SheetKind::Phone, tue, 15 * 60));
        // Phone regular windows stay 30-min aligned inside 10–12 / 13–15.
        assert!(is_valid_regular_slot(SheetKind::Phone, tue, 10 * 60));
        assert!(is_valid_regular_slot(SheetKind::Phone, tue, 14 * 60 + 30));
        assert!(!is_valid_regular_slot(SheetKind::Phone, tue, 12 * 60));
    }

    #[test]
    fn advertised_emergency_slot_is_bookable() {
        let tue = chrono::NaiveDate::from_ymd_opt(2026, 9, 8).unwrap();
        let fri = chrono::NaiveDate::from_ymd_opt(2026, 9, 11).unwrap();
        assert!(!is_valid_regular_slot(SheetKind::Interview, tue, 17 * 60));
        assert!(!is_valid_regular_slot(SheetKind::Interview, fri, 15 * 60));
        assert!(!is_valid_regular_slot(SheetKind::Phone, tue, 15 * 60));
    }

    #[test]
    fn peak_counts_tick_overlap_not_distinct_meetings() {
        let date = "8/26/2026";
        let intervals = vec![
            (date.into(), 9 * 60, 9 * 60 + 30),
            (date.into(), 9 * 60 + 30, 10 * 60),
        ];
        assert_eq!(peak_occupancy(&intervals, date, 9 * 60, 10 * 60), 1);
        let stacked = vec![
            (date.into(), 9 * 60, 10 * 60),
            (date.into(), 9 * 60, 10 * 60),
        ];
        assert_eq!(peak_occupancy(&stacked, date, 9 * 60, 10 * 60), 2);
    }
}
