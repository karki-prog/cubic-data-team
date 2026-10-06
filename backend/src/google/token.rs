use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::config::Config;

const SA_SCOPES: &str = "https://www.googleapis.com/auth/spreadsheets https://www.googleapis.com/auth/drive";

#[derive(Clone)]
pub struct GoogleClient {
    http: reqwest::Client,
    cfg: Config,
    sa: Arc<Mutex<Option<CachedToken>>>,
    user: Arc<Mutex<HashMap<String, CachedToken>>>,
    /// spreadsheet_id -> (fetched_at, tab titles). Titles change rarely; caching
    /// them keeps every fetch_* helper from spending a Sheets metadata call.
    titles: Arc<Mutex<HashMap<String, (Instant, Vec<String>)>>>,
    /// "spreadsheet_id|range|render" -> (fetched_at, rows). Short-TTL cache for
    /// hot reads (booking availability + commit hit the same ranges seconds apart).
    reads: Arc<Mutex<HashMap<String, (Instant, Vec<Vec<String>>)>>>,
}

#[derive(Clone)]
struct CachedToken {
    access: String,
    expires_at: Instant,
}

#[derive(Serialize)]
struct SaClaims {
    iss: String,
    scope: String,
    aud: String,
    exp: usize,
    iat: usize,
}

/// Service-account assertion that acts as a Workspace user (domain-wide delegation).
#[derive(Serialize)]
struct SaSubjectClaims {
    iss: String,
    sub: String,
    scope: String,
    aud: String,
    exp: usize,
    iat: usize,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: Option<u64>,
}

impl GoogleClient {
    pub fn new(cfg: Config) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .expect("http client"),
            cfg,
            sa: Arc::new(Mutex::new(None)),
            user: Arc::new(Mutex::new(HashMap::new())),
            titles: Arc::new(Mutex::new(HashMap::new())),
            reads: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// `sheets_values_get` with a short in-process cache. Use ONLY for reads that
    /// tolerate being a few seconds stale (booking availability / capacity checks).
    pub async fn sheets_values_get_cached(
        &self,
        spreadsheet_id: &str,
        range: &str,
        render: Option<&str>,
        ttl: Duration,
    ) -> anyhow::Result<Vec<Vec<String>>> {
        let key = format!("{spreadsheet_id}|{range}|{}", render.unwrap_or(""));
        {
            let map = self.reads.lock().await;
            if let Some((at, rows)) = map.get(&key) {
                if at.elapsed() < ttl {
                    return Ok(rows.clone());
                }
            }
        }
        let rows = self.sheets_values_get(spreadsheet_id, range, render).await?;
        self.reads
            .lock()
            .await
            .insert(key, (Instant::now(), rows.clone()));
        Ok(rows)
    }

    /// Cached tab-title lookup (5 min TTL). Falls back to a stale copy if the
    /// live call fails so a transient quota error doesn't break booking.
    pub async fn sheet_titles_cached(&self, spreadsheet_id: &str) -> anyhow::Result<Vec<String>> {
        const TTL: Duration = Duration::from_secs(300);
        {
            let map = self.titles.lock().await;
            if let Some((at, titles)) = map.get(spreadsheet_id) {
                if at.elapsed() < TTL {
                    return Ok(titles.clone());
                }
            }
        }
        match self.sheet_titles(spreadsheet_id).await {
            Ok(titles) => {
                self.titles
                    .lock()
                    .await
                    .insert(spreadsheet_id.to_string(), (Instant::now(), titles.clone()));
                Ok(titles)
            }
            Err(err) => {
                let map = self.titles.lock().await;
                if let Some((_, titles)) = map.get(spreadsheet_id) {
                    tracing::warn!(error = %err, "sheet_titles failed — serving cached copy");
                    Ok(titles.clone())
                } else {
                    Err(err)
                }
            }
        }
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    pub fn cfg(&self) -> &Config {
        &self.cfg
    }

    pub async fn sa_token(&self) -> anyhow::Result<String> {
        {
            let guard = self.sa.lock().await;
            if let Some(tok) = guard.as_ref() {
                if tok.expires_at > Instant::now() + Duration::from_secs(60) {
                    return Ok(tok.access.clone());
                }
            }
        }
        let sa = self.cfg.load_service_account()?;
        let now = chrono::Utc::now().timestamp() as usize;
        let claims = SaClaims {
            iss: sa.client_email,
            scope: SA_SCOPES.into(),
            aud: "https://oauth2.googleapis.com/token".into(),
            iat: now,
            exp: now + 3600,
        };
        let key = EncodingKey::from_rsa_pem(sa.private_key.as_bytes())?;
        let mut header = Header::new(Algorithm::RS256);
        header.typ = Some("JWT".into());
        let assertion = encode(&header, &claims, &key)?;
        let res = self
            .http
            .post("https://oauth2.googleapis.com/token")
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", &assertion),
            ])
            .send()
            .await?;
        let status = res.status();
        let body = res.text().await?;
        if !status.is_success() {
            anyhow::bail!("service account token failed: {body}");
        }
        let parsed: TokenResponse = serde_json::from_str(&body)?;
        let mut guard = self.sa.lock().await;
        *guard = Some(CachedToken {
            access: parsed.access_token.clone(),
            expires_at: Instant::now()
                + Duration::from_secs(parsed.expires_in.unwrap_or(3500).saturating_sub(60)),
        });
        Ok(parsed.access_token)
    }

/// Token for the service account acting as `subject` with `scope`. Works only when a
    /// Workspace admin has granted the service account domain-wide delegation for that scope.
    pub async fn sa_token_as(&self, subject: &str, scope: &str) -> anyhow::Result<String> {
        let cache_key = format!("sa:{subject}:{scope}");
        {
            let guard = self.user.lock().await;
            if let Some(tok) = guard.get(&cache_key) {
                if tok.expires_at > Instant::now() + Duration::from_secs(60) {
                    return Ok(tok.access.clone());
                }
            }
        }
        let sa = self.cfg.load_service_account()?;
        let now = chrono::Utc::now().timestamp() as usize;
        let claims = SaSubjectClaims {
            iss: sa.client_email,
            sub: subject.to_string(),
            scope: scope.to_string(),
            aud: "https://oauth2.googleapis.com/token".into(),
            iat: now,
            exp: now + 3600,
        };
        let key = EncodingKey::from_rsa_pem(sa.private_key.as_bytes())?;
        let mut header = Header::new(Algorithm::RS256);
        header.typ = Some("JWT".into());
        let assertion = encode(&header, &claims, &key)?;
        let res = self
            .http
            .post("https://oauth2.googleapis.com/token")
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", &assertion),
            ])
            .send()
            .await?;
        let status = res.status();
        let body = res.text().await?;
        if !status.is_success() {
            anyhow::bail!("service account cannot act as {subject} for {scope}: {body}");
        }
        let parsed: TokenResponse = serde_json::from_str(&body)?;
        self.user.lock().await.insert(
            cache_key,
            CachedToken {
                access: parsed.access_token.clone(),
                expires_at: Instant::now()
                    + Duration::from_secs(parsed.expires_in.unwrap_or(3500).saturating_sub(60)),
            },
        );
        Ok(parsed.access_token)
    }

    pub async fn user_token(&self, kind: &str) -> anyhow::Result<String> {
        {
            let guard = self.user.lock().await;
            if let Some(tok) = guard.get(kind) {
                if tok.expires_at > Instant::now() + Duration::from_secs(60) {
                    return Ok(tok.access.clone());
                }
            }
        }
        let (client_id, client_secret, refresh) = if kind == "gmail" {
            (
                self.cfg.gmail_client_id.clone(),
                self.cfg.gmail_client_secret.clone(),
                self.cfg.gmail_refresh_token.clone(),
            )
        } else {
            (
                self.cfg.calendar_client_id.clone(),
                self.cfg.calendar_client_secret.clone(),
                self.cfg.calendar_refresh_token.clone(),
            )
        };
        if client_id.is_empty() || client_secret.is_empty() || refresh.is_empty() {
            anyhow::bail!("{kind} OAuth not configured (need client + refresh token).");
        }
        let res = self
            .http
            .post("https://oauth2.googleapis.com/token")
            .form(&[
                ("client_id", client_id.as_str()),
                ("client_secret", client_secret.as_str()),
                ("refresh_token", refresh.as_str()),
                ("grant_type", "refresh_token"),
            ])
            .send()
            .await?;
        let status = res.status();
        let body = res.text().await?;
        if !status.is_success() {
            anyhow::bail!("{kind} token refresh failed: {body}");
        }
        let parsed: TokenResponse = serde_json::from_str(&body)?;
        let mut guard = self.user.lock().await;
        guard.insert(
            kind.to_string(),
            CachedToken {
                access: parsed.access_token.clone(),
                expires_at: Instant::now()
                    + Duration::from_secs(parsed.expires_in.unwrap_or(3500).saturating_sub(60)),
            },
        );
        Ok(parsed.access_token)
    }
}
