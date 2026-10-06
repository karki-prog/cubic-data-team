use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::auth::require_session;
use crate::availability::{
    is_valid_start_time, meeting_capacity, peak_occupancy, sheet_and_queue_intervals,
};
use crate::dates::format_date_key_mdy;
use crate::error::AppError;
use crate::types::SheetKind;
use crate::AppState;

const HOLD_TTL_SECS: i64 = 4 * 60;
const HOLD_MAX_SECS: i64 = 12 * 60;

#[derive(Clone, Debug)]
pub struct SlotHold {
    pub id: String,
    pub owner_email: String,
    pub kind: SheetKind,
    pub date_iso: String,
    pub date_key: String,
    pub start_min: i32,
    pub end_min: i32,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct SlotHoldStore {
    inner: Arc<Mutex<HashMap<String, SlotHold>>>,
}

impl SlotHoldStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn intervals(&self, kind: SheetKind, except_id: Option<&str>) -> Vec<(String, i32, i32)> {
        let mut map = self.inner.lock().await;
        prune_locked(&mut map, Utc::now());
        map.values()
            .filter(|h| h.kind == kind && except_id.map(|id| h.id != id).unwrap_or(true))
            .map(|h| (h.date_key.clone(), h.start_min, h.end_min))
            .collect()
    }

    pub async fn try_acquire(
        &self,
        owner_email: &str,
        kind: SheetKind,
        date_iso: &str,
        date_key: &str,
        start_min: i32,
        end_min: i32,
        external: &[(String, i32, i32)],
        capacity: i32,
    ) -> Result<SlotHold, AppError> {
        let mut map = self.inner.lock().await;
        let now = Utc::now();
        prune_locked(&mut map, now);

        if let Some(existing) = map.values().find(|h| {
            h.owner_email == owner_email
                && h.kind == kind
                && h.date_key == date_key
                && h.start_min == start_min
                && h.end_min == end_min
        }) {
            let id = existing.id.clone();
            return touch_locked(&mut map, &id, owner_email, now);
        }

        map.retain(|_, h| !(h.owner_email == owner_email && h.kind == kind));

        let mut combined = external.to_vec();
        combined.extend(hold_intervals_locked(&map, kind, None));
        let peak = peak_occupancy(&combined, date_key, start_min, end_min);
        if peak >= capacity {
            return Err(AppError::Conflict(
                "This time is already being booked. Choose another slot.".into(),
            ));
        }

        let created_at = now;
        let hold = SlotHold {
            id: Uuid::new_v4().to_string(),
            owner_email: owner_email.to_string(),
            kind,
            date_iso: date_iso.to_string(),
            date_key: date_key.to_string(),
            start_min,
            end_min,
            created_at,
            expires_at: ttl_from(created_at, now),
        };
        map.insert(hold.id.clone(), hold.clone());
        Ok(hold)
    }

    pub async fn heartbeat(&self, id: &str, owner_email: &str) -> Result<SlotHold, AppError> {
        let mut map = self.inner.lock().await;
        let now = Utc::now();
        prune_locked(&mut map, now);
        touch_locked(&mut map, id, owner_email, now)
    }

    pub async fn release(&self, id: &str, owner_email: &str) -> bool {
        let mut map = self.inner.lock().await;
        prune_locked(&mut map, Utc::now());
        match map.get(id) {
            Some(h) if h.owner_email == owner_email => {
                map.remove(id);
                true
            }
            _ => false,
        }
    }

    pub async fn peek(
        &self,
        id: &str,
        owner_email: &str,
        kind: SheetKind,
    ) -> Result<SlotHold, AppError> {
        let mut map = self.inner.lock().await;
        let now = Utc::now();
        prune_locked(&mut map, now);
        let hold = map.get(id).cloned().ok_or_else(|| {
            AppError::Conflict("This slot is no longer reserved. Close and pick it again.".into())
        })?;
        if hold.owner_email != owner_email || hold.kind != kind {
            return Err(AppError::Conflict(
                "This slot is no longer reserved. Close and pick it again.".into(),
            ));
        }
        if hold.expires_at <= now {
            map.remove(id);
            return Err(AppError::Conflict(
                "Your hold on this slot expired. Close and pick it again.".into(),
            ));
        }
        Ok(hold)
    }

    pub async fn verify_for_commit(
        &self,
        id: &str,
        owner_email: &str,
        kind: SheetKind,
        external: &[(String, i32, i32)],
        capacity: i32,
    ) -> Result<SlotHold, AppError> {
        let mut map = self.inner.lock().await;
        let now = Utc::now();
        prune_locked(&mut map, now);
        let hold = map.get(id).cloned().ok_or_else(|| {
            AppError::Conflict("This slot is no longer reserved. Close and pick it again.".into())
        })?;
        if hold.owner_email != owner_email || hold.kind != kind {
            return Err(AppError::Conflict(
                "This slot is no longer reserved. Close and pick it again.".into(),
            ));
        }
        if hold.expires_at <= now {
            map.remove(id);
            return Err(AppError::Conflict(
                "Your hold on this slot expired. Close and pick it again.".into(),
            ));
        }
        let mut combined = external.to_vec();
        combined.extend(hold_intervals_locked(&map, kind, Some(&hold.id)));
        let peak = peak_occupancy(&combined, &hold.date_key, hold.start_min, hold.end_min);
        if peak >= capacity {
            map.remove(id);
            return Err(AppError::Conflict(
                "This time was just taken. Pick another slot.".into(),
            ));
        }
        Ok(hold)
    }

    pub async fn consume(&self, id: &str, owner_email: &str) {
        let mut map = self.inner.lock().await;
        if map.get(id).is_some_and(|h| h.owner_email == owner_email) {
            map.remove(id);
        }
    }
}

fn hold_intervals_locked(
    map: &HashMap<String, SlotHold>,
    kind: SheetKind,
    except_id: Option<&str>,
) -> Vec<(String, i32, i32)> {
    map.values()
        .filter(|h| h.kind == kind && except_id.map(|id| h.id != id).unwrap_or(true))
        .map(|h| (h.date_key.clone(), h.start_min, h.end_min))
        .collect()
}

fn prune_locked(map: &mut HashMap<String, SlotHold>, now: DateTime<Utc>) {
    map.retain(|_, h| h.expires_at > now);
}

fn ttl_from(created_at: DateTime<Utc>, now: DateTime<Utc>) -> DateTime<Utc> {
    let max_at = created_at + Duration::seconds(HOLD_MAX_SECS);
    let next = now + Duration::seconds(HOLD_TTL_SECS);
    if next < max_at {
        next
    } else {
        max_at
    }
}

fn touch_locked(
    map: &mut HashMap<String, SlotHold>,
    id: &str,
    owner_email: &str,
    now: DateTime<Utc>,
) -> Result<SlotHold, AppError> {
    let (owner_ok, expired) = match map.get(id) {
        Some(hold) => (
            hold.owner_email == owner_email,
            hold.created_at + Duration::seconds(HOLD_MAX_SECS) <= now,
        ),
        None => {
            return Err(AppError::Conflict(
                "Your hold on this slot expired. Close and pick it again.".into(),
            ));
        }
    };
    if !owner_ok {
        return Err(AppError::Forbidden("That hold belongs to someone else.".into()));
    }
    if expired {
        map.remove(id);
        return Err(AppError::Conflict(
            "Your hold on this slot expired. Close and pick it again.".into(),
        ));
    }
    let hold = map.get_mut(id).expect("hold present after checks");
    hold.expires_at = ttl_from(hold.created_at, now);
    Ok(hold.clone())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcquireBody {
    kind: String,
    date_iso: String,
    start_min: i32,
    #[serde(default)]
    duration_min: i32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HoldIdBody {
    hold_id: String,
}

fn parse_kind(raw: &str) -> Result<SheetKind, AppError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "interview" => Ok(SheetKind::Interview),
        "phone" => Ok(SheetKind::Phone),
        _ => Err(AppError::BadRequest("Invalid slot kind.".into())),
    }
}

fn hold_json(hold: &SlotHold) -> serde_json::Value {
    json!({
        "ok": true,
        "holdId": hold.id,
        "expiresAt": hold.expires_at.to_rfc3339(),
        "dateIso": hold.date_iso,
        "startMin": hold.start_min,
        "endMin": hold.end_min
    })
}

pub async fn acquire(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<AcquireBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let user = require_session(&state.cfg, &headers)?;
    let kind = parse_kind(&body.kind)?;
    let date = chrono::NaiveDate::parse_from_str(body.date_iso.trim(), "%Y-%m-%d")
        .map_err(|_| AppError::BadRequest("Invalid meeting date.".into()))?;
    if !is_valid_start_time(kind, date, body.start_min) {
        let msg = match kind {
            SheetKind::Interview | SheetKind::Phone => {
                "Pick a CST time during working hours (skip 12–1 lunch)."
            }
        };
        return Err(AppError::BadRequest(msg.into()));
    }
    // A hold reserves exactly one grid cell, regardless of how long the meeting
    // itself is — otherwise a 4-hour interview greys out the whole morning for
    // everyone else.
    let end_min = crate::availability::regular_slot_end(kind, body.start_min);
    let date_key = format_date_key_mdy(date);
    let external = sheet_and_queue_intervals(&state, kind, &date_key)
        .await
        .map_err(AppError::from)?;
    let hold = state
        .holds
        .try_acquire(
            &user.email,
            kind,
            body.date_iso.trim(),
            &date_key,
            body.start_min,
            end_min,
            &external,
            meeting_capacity(kind),
        )
        .await?;
    Ok(Json(hold_json(&hold)))
}

pub async fn heartbeat(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<HoldIdBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let user = require_session(&state.cfg, &headers)?;
    if body.hold_id.trim().is_empty() {
        return Err(AppError::BadRequest("holdId is required.".into()));
    }
    let hold = state.holds.heartbeat(body.hold_id.trim(), &user.email).await?;
    Ok(Json(hold_json(&hold)))
}

pub async fn release(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<HoldIdBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let user = require_session(&state.cfg, &headers)?;
    if !body.hold_id.trim().is_empty() {
        state.holds.release(body.hold_id.trim(), &user.email).await;
    }
    Ok(Json(json!({ "ok": true })))
}
