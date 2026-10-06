use serde::Deserialize;
use serde_json::json;

use super::token::GoogleClient;

pub struct CalendarEventInput {
    pub title: String,
    pub description_html: String,
    pub start_iso: String,
    pub end_iso: String,
    pub attendees: Vec<String>,
    pub attachments: Vec<(String, String)>,
    pub existing_event_id: String,
}

#[derive(Deserialize)]
struct EventRes {
    id: Option<String>,
}

#[derive(Deserialize)]
struct CalList {
    items: Option<Vec<CalItem>>,
}

#[derive(Deserialize)]
struct CalItem {
    summary: Option<String>,
}

fn is_auth_error(msg: &str) -> bool {
    let m = msg.to_lowercase();
    m.contains("unauthorized_client") || m.contains("invalid_grant") || m.contains("invalid_client")
}

impl GoogleClient {
    pub async fn upsert_calendar_event(&self, input: CalendarEventInput) -> anyhow::Result<String> {
        let token = self.user_token("calendar").await?;
        let owner = self.cfg().calendar_owner_email.clone();
        let mut seen = std::collections::HashSet::new();
        let attendees: Vec<serde_json::Value> = input
            .attendees
            .iter()
            .map(|e| e.trim().to_lowercase())
            .filter(|e| !e.is_empty() && *e != owner)
            .filter(|e| seen.insert(e.clone()))
            .map(|email| json!({ "email": email }))
            .collect();

        let attachments = if input.attachments.is_empty() {
            None
        } else {
            Some(
                input
                    .attachments
                    .iter()
                    .map(|(url, title)| json!({ "fileUrl": url, "title": title }))
                    .collect::<Vec<_>>(),
            )
        };

        let resource = json!({
            "summary": input.title,
            "description": input.description_html,
            "start": { "dateTime": input.start_iso, "timeZone": self.cfg().timezone },
            "end": { "dateTime": input.end_iso, "timeZone": self.cfg().timezone },
            "attendees": attendees,
            "guestsCanModify": false,
            "guestsCanInviteOthers": false,
            "reminders": { "useDefault": true },
            "attachments": attachments
        });

        let cal_id = urlencoding::encode(&self.cfg().calendar_id);
        if !input.existing_event_id.is_empty() {
            let event_id = input.existing_event_id.replace("@google.com", "");
            let url = format!(
                "https://www.googleapis.com/calendar/v3/calendars/{cal_id}/events/{}?sendUpdates=all&supportsAttachments=true",
                urlencoding::encode(&event_id)
            );
            let res = self
                .http()
                .patch(url)
                .bearer_auth(&token)
                .json(&resource)
                .send()
                .await?;
            if res.status().is_success() {
                let parsed: EventRes = res.json().await?;
                return Ok(parsed.id.unwrap_or(input.existing_event_id));
            }
            let body = res.text().await?;
            if is_auth_error(&body) {
                anyhow::bail!("{body}");
            }
            tracing::warn!("[calendar] patch missed, inserting new event: {body}");
        }

        let url = format!(
            "https://www.googleapis.com/calendar/v3/calendars/{cal_id}/events?sendUpdates=all&supportsAttachments=true"
        );
        let res = self.http().post(url).bearer_auth(token).json(&resource).send().await?;
        if !res.status().is_success() {
            anyhow::bail!("calendar insert failed: {}", res.text().await?);
        }
        let parsed: EventRes = res.json().await?;
        parsed
            .id
            .ok_or_else(|| anyhow::anyhow!("Calendar insert returned no event id."))
    }

    pub async fn delete_calendar_event(&self, event_id: &str) {
        if event_id.is_empty() {
            return;
        }
        let Ok(token) = self.user_token("calendar").await else {
            return;
        };
        let cal_id = urlencoding::encode(&self.cfg().calendar_id);
        let event_id = event_id.replace("@google.com", "");
        let url = format!(
            "https://www.googleapis.com/calendar/v3/calendars/{cal_id}/events/{}?sendUpdates=all",
            urlencoding::encode(&event_id)
        );
        if let Err(err) = self.http().delete(url).bearer_auth(token).send().await {
            tracing::warn!("[calendar] delete skipped: {err}");
        }
    }

    pub async fn calendar_probe(&self) -> anyhow::Result<String> {
        let token = self.user_token("calendar").await?;
        let res = self
            .http()
            .get("https://www.googleapis.com/calendar/v3/users/me/calendarList?maxResults=1")
            .bearer_auth(token)
            .send()
            .await?;
        if !res.status().is_success() {
            anyhow::bail!("{}", res.text().await?);
        }
        let parsed: CalList = res.json().await?;
        Ok(parsed
            .items
            .unwrap_or_default()
            .into_iter()
            .next()
            .and_then(|i| i.summary)
            .unwrap_or_else(|| "primary".into()))
    }
}
