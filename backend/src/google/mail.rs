use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;

use super::token::GoogleClient;

pub struct SendMailInput {
    pub to: String,
    pub cc: Vec<String>,
    pub subject: String,
    pub plain: String,
    pub html: String,
    /// Reply-To header. Empty = none (replies go to From).
    pub reply_to: String,
}

fn normalize_email(raw: &str) -> String {
    let email = raw.trim().to_lowercase();
    if email.contains('@') {
        email
    } else {
        String::new()
    }
}

fn encode_subject(subject: &str) -> String {
    let text = subject.replace(['\r', '\n'], " ").trim().to_string();
    if text.chars().all(|c| c >= ' ' && c <= '~') {
        text
    } else {
        format!("=?UTF-8?B?{}?=", STANDARD.encode(text.as_bytes()))
    }
}

fn build_raw(input: &SendMailInput, from_email: &str, sender_name: &str) -> String {
    let boundary = format!("cubic_{:x}", chrono::Utc::now().timestamp_millis());
    let cc: Vec<String> = input.cc.iter().map(|e| normalize_email(e)).filter(|e| !e.is_empty()).collect();
    let mut headers = vec![
        format!("From: {sender_name} <{from_email}>"),
        format!("To: {}", input.to),
    ];
    let reply_to = normalize_email(&input.reply_to);
    if !reply_to.is_empty() {
        headers.push(format!("Reply-To: {reply_to}"));
    }
    if !cc.is_empty() {
        headers.push(format!("Cc: {}", cc.join(", ")));
    }
    headers.push(format!("Subject: {}", encode_subject(&input.subject)));
    headers.push("MIME-Version: 1.0".into());
    headers.push(format!(r#"Content-Type: multipart/alternative; boundary="{boundary}""#));

    let body = format!(
        "--{boundary}\r\nContent-Type: text/plain; charset=\"UTF-8\"\r\n\r\n{}\r\n--{boundary}\r\nContent-Type: text/html; charset=\"UTF-8\"\r\n\r\n{}\r\n--{boundary}--",
        input.plain, input.html
    );
    let raw = format!("{}\r\n\r\n{body}", headers.join("\r\n"));
    URL_SAFE_NO_PAD.encode(raw.as_bytes())
}

impl GoogleClient {
    pub async fn send_mail(&self, mut input: SendMailInput) -> anyhow::Result<(bool, String, Vec<String>)> {
        input.to = normalize_email(&input.to);
        if input.to.is_empty() {
            anyhow::bail!("Mail recipient is required.");
        }
        let token = self.user_token("gmail").await?;
        let from = {
            let n = normalize_email(&self.cfg().mail_from_email);
            if n.is_empty() {
                "reminder@cubicit.net".into()
            } else {
                n
            }
        };
        let raw = build_raw(&input, &from, &self.cfg().mail_sender_name);
        let res = self
            .http()
            .post("https://gmail.googleapis.com/gmail/v1/users/me/messages/send")
            .bearer_auth(token)
            .json(&serde_json::json!({ "raw": raw }))
            .send()
            .await?;
        if !res.status().is_success() {
            anyhow::bail!("gmail send failed: {}", res.text().await?);
        }
        Ok((true, input.to, input.cc))
    }

    pub async fn gmail_probe(&self) -> anyhow::Result<()> {
        let token = self.user_token("gmail").await?;
        let res = self
            .http()
            .get("https://gmail.googleapis.com/gmail/v1/users/me/messages?maxResults=1")
            .bearer_auth(token)
            .send()
            .await?;
        let status = res.status();
        let body = res.text().await?;
        if status.is_success() {
            return Ok(());
        }
        if body.to_lowercase().contains("insufficient authentication scopes") {
            return Ok(());
        }
        anyhow::bail!("{body}");
    }
}

#[cfg(test)]
mod reply_to_tests {
    use super::*;

    #[test]
    fn raw_has_from_reply_to_and_no_cc() {
        let input = SendMailInput {
            to: "cand@example.com".into(),
            cc: Vec::new(),
            subject: "Hi".into(),
            plain: "p".into(),
            html: "<p>p</p>".into(),
            reply_to: "karki@cubicit.net".into(),
        };
        let b64 = build_raw(&input, "reminder@cubicit.net", "Cubic Interview Team");
        let raw = String::from_utf8(
            base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(b64).unwrap(),
        )
        .unwrap();
        assert!(raw.contains("From: Cubic Interview Team <reminder@cubicit.net>"), "{raw}");
        assert!(raw.contains("Reply-To: karki@cubicit.net"), "{raw}");
        assert!(!raw.contains("\r\nCc:"), "{raw}");
    }
}
