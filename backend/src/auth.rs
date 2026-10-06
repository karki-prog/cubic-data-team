use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::Json;
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::config::Config;
use crate::error::AppError;
use crate::AppState;

pub const ACCESS_COOKIE: &str = "cubic_access";
pub const REFRESH_COOKIE: &str = "cubic_refresh";
pub const OAUTH_COOKIE: &str = "cubic_oauth_state";
pub const ACCESS_TTL_SEC: i64 = 15 * 60;
pub const REFRESH_TTL_SEC: i64 = 7 * 24 * 60 * 60;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionUser {
    pub email: String,
    pub name: String,
    pub role: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    #[serde(default)]
    email: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    role: String,
    typ: String,
    #[serde(default)]
    sub: String,
    iat: usize,
    exp: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    next: Option<String>,
}

pub fn cookie_value(headers: &HeaderMap, name: &str) -> String {
    let Some(raw) = headers.get(header::COOKIE).and_then(|v| v.to_str().ok()) else {
        return String::new();
    };
    for part in raw.split(';') {
        let part = part.trim();
        if let Some(rest) = part.strip_prefix(&format!("{name}=")) {
            return urlencoding::decode(rest).map(|s| s.into_owned()).unwrap_or_else(|_| rest.to_string());
        }
    }
    String::new()
}

fn cookie_base(cfg: &Config) -> String {
    let mut s = "Path=/; HttpOnly; SameSite=Lax".to_string();
    if cfg.cookie_secure() {
        s.push_str("; Secure");
    }
    if let Some(domain) = cfg.cookie_domain() {
        s.push_str(&format!("; Domain={domain}"));
    }
    s
}

fn set_cookie(headers: &mut HeaderMap, name: &str, value: &str, max_age: i64, cfg: &Config) {
    let encoded = urlencoding::encode(value);
    let line = format!("{name}={encoded}; Max-Age={max_age}; {}", cookie_base(cfg));
    if let Ok(val) = line.parse() {
        headers.append(header::SET_COOKIE, val);
    }
}

pub fn sign_access(cfg: &Config, user: &SessionUser) -> anyhow::Result<String> {
    sign(cfg, user, "access", ACCESS_TTL_SEC, None)
}

pub fn sign_refresh(cfg: &Config, user: &SessionUser) -> anyhow::Result<String> {
    sign(cfg, user, "refresh", REFRESH_TTL_SEC, None)
}

pub fn sign_oauth_state(cfg: &Config, next: &str) -> anyhow::Result<String> {
    sign(
        cfg,
        &SessionUser {
            email: "oauth".into(),
            name: String::new(),
            role: "staff".into(),
        },
        "oauth",
        10 * 60,
        Some(next.to_string()),
    )
}

fn sign(cfg: &Config, user: &SessionUser, typ: &str, ttl: i64, next: Option<String>) -> anyhow::Result<String> {
    let now = chrono::Utc::now().timestamp() as usize;
    let claims = Claims {
        email: user.email.clone(),
        name: user.name.clone(),
        role: user.role.clone(),
        typ: typ.into(),
        sub: user.email.clone(),
        iat: now,
        exp: now + ttl as usize,
        next,
    };
    let mut header = Header::new(Algorithm::HS256);
    header.typ = Some("JWT".into());
    Ok(encode(
        &header,
        &claims,
        &EncodingKey::from_secret(cfg.jwt_secret.as_bytes()),
    )?)
}

pub fn verify_token(cfg: &Config, token: &str, typ: &str) -> anyhow::Result<SessionUser> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_exp = true;
    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(cfg.jwt_secret.as_bytes()),
        &validation,
    )?;
    if data.claims.typ != typ {
        anyhow::bail!("Wrong token type.");
    }
    let email = if !data.claims.email.is_empty() {
        data.claims.email.to_lowercase()
    } else {
        data.claims.sub.to_lowercase()
    };
    if email.is_empty() {
        anyhow::bail!("Token missing email.");
    }
    Ok(SessionUser {
        email,
        name: data.claims.name,
        role: "staff".into(),
    })
}

pub fn verify_oauth_state(cfg: &Config, token: &str) -> anyhow::Result<String> {
    let mut validation = Validation::new(Algorithm::HS256);
    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(cfg.jwt_secret.as_bytes()),
        &validation,
    )?;
    if data.claims.typ != "oauth" {
        anyhow::bail!("Wrong token type.");
    }
    Ok(data.claims.next.unwrap_or_else(|| "/".into()))
}

pub fn session_from_headers(cfg: &Config, headers: &HeaderMap) -> Option<SessionUser> {
    let access = cookie_value(headers, ACCESS_COOKIE);
    if !access.is_empty() {
        if let Ok(user) = verify_token(cfg, &access, "access") {
            return Some(user);
        }
    }
    let refresh = cookie_value(headers, REFRESH_COOKIE);
    if refresh.is_empty() {
        return None;
    }
    verify_token(cfg, &refresh, "refresh").ok()
}

pub fn require_session(cfg: &Config, headers: &HeaderMap) -> Result<SessionUser, AppError> {
    session_from_headers(cfg, headers).ok_or_else(AppError::unauthorized)
}

pub fn apply_session_cookies(cfg: &Config, headers: &mut HeaderMap, access: &str, refresh: &str) {
    set_cookie(headers, ACCESS_COOKIE, access, ACCESS_TTL_SEC, cfg);
    set_cookie(headers, REFRESH_COOKIE, refresh, REFRESH_TTL_SEC, cfg);
}

pub fn clear_session_cookies(cfg: &Config, headers: &mut HeaderMap) {
    set_cookie(headers, ACCESS_COOKIE, "", 0, cfg);
    set_cookie(headers, REFRESH_COOKIE, "", 0, cfg);
    set_cookie(headers, OAUTH_COOKIE, "", 0, cfg);
}

pub fn safe_next_path(raw: Option<&str>) -> String {
    let value = raw.unwrap_or("").trim();
    if !value.starts_with('/')
        || value.starts_with("//")
        || value.starts_with("/api/")
        || value == "/api"
    {
        "/".into()
    } else {
        value.to_string()
    }
}

fn site_origin(state: &AppState, headers: &HeaderMap) -> String {
    let host = headers
        .get("x-forwarded-host")
        .or_else(|| headers.get(header::HOST))
        .and_then(|v| v.to_str().ok());
    let proto = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok());
    state.cfg.site_origin_from_request(host, proto)
}

/// Always AUTH_SITE_URL so Google Console's authorized redirect URI matches
/// whether the user opened localhost or 127.0.0.1.
fn oauth_origin(state: &AppState) -> String {
    let url = state.cfg.site_url.trim().trim_end_matches('/');
    if url.is_empty() {
        "http://localhost:3000".into()
    } else {
        url.to_string()
    }
}

fn google_redirect_uri(origin: &str) -> String {
    format!("{origin}/api/auth/callback")
}

pub async fn google_start(
    State(state): State<AppState>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let origin = oauth_origin(&state);
    if state.cfg.google_oauth_client_id.is_empty() || state.cfg.google_oauth_client_secret.is_empty() {
        return Redirect::to(&format!("{origin}/login?error=google-not-configured")).into_response();
    }
    let next = safe_next_path(q.get("next").map(|s| s.as_str()));
    let state_jwt = match sign_oauth_state(&state.cfg, &next) {
        Ok(s) => s,
        Err(err) => {
            return Redirect::to(&format!(
                "{origin}/login?error={}",
                urlencoding::encode(&err.to_string())
            ))
            .into_response();
        }
    };
    let params = [
        ("client_id", state.cfg.google_oauth_client_id.as_str()),
        ("redirect_uri", &google_redirect_uri(&origin)),
        ("response_type", "code"),
        ("scope", "openid email profile"),
        ("state", &state_jwt),
        ("prompt", "select_account"),
        ("access_type", "online"),
    ];
    let url = format!(
        "https://accounts.google.com/o/oauth2/v2/auth?{}",
        serde_urlencoded(&params)
    );
    tracing::info!("google oauth start redirect_uri={}", google_redirect_uri(&origin));
    Redirect::to(&url).into_response()
}

fn serde_urlencoded(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

pub async fn google_callback(
    State(state): State<AppState>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let origin = oauth_origin(&state);
    if let Some(err) = q.get("error") {
        let mapped = if err == "access_denied" {
            "google-cancelled"
        } else {
            err
        };
        return Redirect::to(&format!("{origin}/login?error={}", urlencoding::encode(mapped))).into_response();
    }
    let code = q.get("code").cloned().unwrap_or_default();
    let oauth_state = q.get("state").cloned().unwrap_or_default();
    if code.is_empty() || oauth_state.is_empty() {
        return Redirect::to(&format!("{origin}/login?error=invalid-oauth-state")).into_response();
    }
    let next = match verify_oauth_state(&state.cfg, &oauth_state) {
        Ok(n) => safe_next_path(Some(&n)),
        Err(_) => {
            return Redirect::to(&format!("{origin}/login?error=invalid-oauth-state")).into_response();
        }
    };
    match exchange_google_code(&state, &origin, &code).await {
        Ok(user) => {
            let mut out_headers = HeaderMap::new();
            if let (Ok(access), Ok(refresh)) = (sign_access(&state.cfg, &user), sign_refresh(&state.cfg, &user)) {
                apply_session_cookies(&state.cfg, &mut out_headers, &access, &refresh);
            }
            set_cookie(&mut out_headers, OAUTH_COOKIE, "", 0, &state.cfg);
            let mut res = Redirect::to(&format!("{origin}{next}")).into_response();
            res.headers_mut().extend(out_headers);
            res
        }
        Err(err) => Redirect::to(&format!(
            "{origin}/login?error={}",
            urlencoding::encode(&err.to_string())
        ))
        .into_response(),
    }
}

async fn exchange_google_code(state: &AppState, origin: &str, code: &str) -> anyhow::Result<SessionUser> {
    let res = state
        .google
        .http()
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("code", code),
            ("client_id", state.cfg.google_oauth_client_id.as_str()),
            ("client_secret", state.cfg.google_oauth_client_secret.as_str()),
            ("redirect_uri", &google_redirect_uri(origin)),
            ("grant_type", "authorization_code"),
        ])
        .send()
        .await?;
    let json: serde_json::Value = res.json().await?;
    let id_token = json
        .get("id_token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "{}",
                json.get("error_description")
                    .or_else(|| json.get("error"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("Google token exchange failed.")
            )
        })?;
    let payload_b64 = id_token.split('.').nth(1).unwrap_or("");
    let payload = base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, payload_b64)
        .or_else(|_| base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE, payload_b64))?;
    let payload: serde_json::Value = serde_json::from_slice(&payload)?;
    if payload.get("email_verified").and_then(|v| v.as_bool()) != Some(true) {
        anyhow::bail!("Google email is not verified.");
    }
    let email = payload
        .get("email")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_lowercase();
    if !crate::access::is_login_allowed(state, &email).await {
        anyhow::bail!("not-allowed");
    }
    let name = payload
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    Ok(SessionUser {
        email: email.clone(),
        name: if name.is_empty() {
            email.split('@').next().unwrap_or("staff").to_string()
        } else {
            name
        },
        role: "staff".into(),
    })
}

pub async fn me(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let user = session_from_headers(&state.cfg, &headers);
    Json(json!({ "ok": true, "user": user }))
}

pub async fn login_disabled() -> impl IntoResponse {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        Json(json!({ "ok": false, "error": "Use Google sign-in." })),
    )
}

pub async fn logout_post(State(state): State<AppState>) -> Response {
    let mut headers = HeaderMap::new();
    clear_session_cookies(&state.cfg, &mut headers);
    let mut res = Json(json!({ "ok": true })).into_response();
    res.headers_mut().extend(headers);
    res
}

pub async fn logout_get(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let origin = site_origin(&state, &headers);
    let mut out = HeaderMap::new();
    clear_session_cookies(&state.cfg, &mut out);
    let mut res = Redirect::to(&format!("{origin}/login")).into_response();
    res.headers_mut().extend(out);
    res
}

pub async fn refresh(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let refresh = cookie_value(&headers, REFRESH_COOKIE);
    if refresh.is_empty() {
        return AppError::unauthorized().into_response();
    }
    match verify_token(&state.cfg, &refresh, "refresh") {
        Ok(user) => {
            let Ok(access) = sign_access(&state.cfg, &user) else {
                return AppError::unauthorized().into_response();
            };
            let Ok(next_refresh) = sign_refresh(&state.cfg, &user) else {
                return AppError::unauthorized().into_response();
            };
            let mut out = HeaderMap::new();
            apply_session_cookies(&state.cfg, &mut out, &access, &next_refresh);
            let mut res = Json(json!({ "ok": true, "email": user.email })).into_response();
            res.headers_mut().extend(out);
            res
        }
        Err(_) => AppError::unauthorized().into_response(),
    }
}
