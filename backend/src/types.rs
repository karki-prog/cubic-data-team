use crate::dates::{meeting_time_key, normalize_key_part};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SheetKind {
    Phone,
    Interview,
}

impl SheetKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SheetKind::Phone => "phone",
            SheetKind::Interview => "interview",
        }
    }
}

impl From<&str> for SheetKind {
    fn from(value: &str) -> Self {
        if value.eq_ignore_ascii_case("phone") {
            SheetKind::Phone
        } else {
            SheetKind::Interview
        }
    }
}

#[derive(Clone, Debug)]
pub struct BookingRecord {
    pub kind: SheetKind,
    pub sheet_name: String,
    pub row: i32,
    pub candidate_name: String,
    pub client: String,
    pub interview_stage: String,
    pub location: String,
    pub interview_platform: String,
    pub poc: String,
    pub meeting_date: String,
    pub meeting_time: String,
    pub meeting_duration: String,
    pub panel: String,
    pub vendor: String,
    pub support: String,
    pub tech: String,
    pub visa: String,
    pub status: String,
    pub special_note: String,
    pub resume_link: String,
    pub jd_link: String,
    pub folder_link: String,
    pub submitter_email: String,
    pub date_key: String,
}

/// One historical interview row from a Data Interview Sheet month tab.
///
/// Those tabs pre-date the booking tool: they carry no interview stage, status,
/// resume or JD columns, so they are kept apart from `BookingRecord` instead of
/// being padded with empty fields.
#[derive(Clone, Debug)]
pub struct HistoryRecord {
    pub sheet_name: String,
    pub candidate_name: String,
    pub client: String,
    pub vendor: String,
    pub tech: String,
    pub location: String,
    pub poc: String,
    pub support: String,
    pub panel: String,
    pub meeting_date: String,
    pub meeting_time: String,
    pub meeting_duration: String,
    pub note: String,
    pub date_key: String,
}

#[derive(Clone, Debug)]
pub struct BackendState {
    pub key: String,
    pub kind: SheetKind,
    pub status: String,
    pub calendar_event_id: String,
    pub email_status: String,
    pub cubic_synced: bool,
}

pub fn record_fingerprint(
    sheet_name: &str,
    candidate_name: &str,
    client: &str,
    date_key: &str,
    meeting_time: &str,
    interview_stage: &str,
) -> String {
    [
        sheet_name,
        candidate_name,
        client,
        date_key,
        &meeting_time_key(meeting_time),
        interview_stage,
    ]
    .into_iter()
    .map(normalize_key_part)
    .collect::<Vec<_>>()
    .join("|")
}
