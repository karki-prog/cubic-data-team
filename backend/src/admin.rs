//! Admin surface (`/admin`) and the two access levels behind it.
//!
//! * `is_admin` — **staff**: the whole Google Workspace domain plus extra
//!   addresses. Grants the Application Tracker's candidate roster view.
//! * `is_superadmin` — **superadmin**: an explicit allow-list. The only thing
//!   that opens /admin.
//!
//! Keeping both here means there is exactly one definition of each, and the
//! tracker delegates to `is_admin` rather than holding a copy that could drift.

use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::Json;
use serde_json::json;

use crate::auth::session_from_headers;
use crate::staff::normalize_email;
use crate::AppState;

/// Staff = anyone on the configured Google Workspace domain, plus explicitly
/// listed extra addresses. Candidates sign in with personal Gmail accounts and
/// therefore never match.
///
/// This is the *tracker* staff check (who may see the candidate roster). It is
/// intentionally broader than `is_superadmin` — never use it to gate /admin.
pub fn is_admin(cfg: &crate::config::Config, email: &str) -> bool {
    let value = normalize_email(email);
    if value.is_empty() {
        return false;
    }
    let domain = cfg.allowed_domain.trim().to_lowercase();
    if !domain.is_empty() && value.ends_with(&format!("@{domain}")) {
        return true;
    }
    cfg.extra_emails.iter().any(|e| normalize_email(e) == value)
}

/// Superadmin — the only role that opens /admin. An explicit allow-list, never
/// a domain match. Defaults to karki@cubicit.net; override with
/// SUPERADMIN_EMAILS.
pub fn is_superadmin(cfg: &crate::config::Config, email: &str) -> bool {
    email_in_list(&cfg.superadmin_emails, email)
}

/// Split out from `is_superadmin` so the matching rule is testable without
/// building a whole Config.
fn email_in_list(list: &[String], email: &str) -> bool {
    let value = normalize_email(email);
    if value.is_empty() {
        return false;
    }
    list.iter().any(|e| normalize_email(e) == value)
}

#[cfg(test)]
mod tests {
    use super::email_in_list;

    fn list() -> Vec<String> {
        vec!["karki@cubicit.net".to_string()]
    }

    #[test]
    fn only_the_listed_address_is_superadmin() {
        assert!(email_in_list(&list(), "karki@cubicit.net"));
        // Case and surrounding whitespace must not matter.
        assert!(email_in_list(&list(), "  Karki@CubicIT.net  "));
        // Everyone else is out — including other staff on the same domain, who
        // remain `is_admin` (tracker roster) but are not superadmin.
        assert!(!email_in_list(&list(), "someoneelse@cubicit.net"));
        assert!(!email_in_list(&list(), "karki@example.com"));
        assert!(!email_in_list(&list(), "karki@cubicit.net.evil.com"));
        assert!(!email_in_list(&list(), ""));
    }
}

/// `GET /api/admin/me` — what the `/admin` pages gate on.
///
/// Always 200 so the client can tell "signed in but not superadmin" (render a
/// clear refusal) apart from "not signed in" (bounce to /login). A 403 would
/// collapse those two into one indistinguishable failure.
pub async fn me(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let user = session_from_headers(&state.cfg, &headers);
    let superadmin = user
        .as_ref()
        .map(|u| is_superadmin(&state.cfg, &u.email))
        .unwrap_or(false);
    Json(json!({
        "ok": true,
        "signedIn": user.is_some(),
        "superadmin": superadmin,
        "user": user,
    }))
}


// ————— Plugin tokens ————————————————————————————————————————————————

/// The ONLYOFFICE plugin runs in an iframe embedded in the Document Server's
/// page, so the browser treats our cookie as third-party and withholds it
/// (session cookies are `SameSite=Lax`, deliberately — that is CSRF protection
/// worth keeping). The plugin therefore carries a short-lived bearer token
/// instead, minted for the signed-in superadmin when the editor config is built.
#[derive(serde::Serialize, serde::Deserialize)]
struct PluginClaims {
    sub: String,
    exp: i64,
}

/// Editing sessions run long; a working day is plenty and still bounded.
const PLUGIN_TOKEN_TTL_SECS: i64 = 12 * 60 * 60;

pub fn mint_plugin_token(cfg: &crate::config::Config, email: &str) -> Option<String> {
    let secret = cfg.jwt_secret.trim();
    if secret.is_empty() || email.trim().is_empty() {
        return None;
    }
    jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &PluginClaims {
            sub: normalize_email(email),
            exp: chrono::Utc::now().timestamp() + PLUGIN_TOKEN_TTL_SECS,
        },
        &jsonwebtoken::EncodingKey::from_secret(secret.as_bytes()),
    )
    .ok()
}

/// Returns the email the token was minted for, and only if that address is
/// *still* a superadmin — revoking access must not wait for a token to expire.
pub fn verify_plugin_token(cfg: &crate::config::Config, token: &str) -> Option<String> {
    let secret = cfg.jwt_secret.trim();
    if secret.is_empty() {
        return None;
    }
    let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::HS256);
    validation.set_required_spec_claims(&["exp"]);
    let data = jsonwebtoken::decode::<PluginClaims>(
        token,
        &jsonwebtoken::DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .ok()?;
    let email = data.claims.sub;
    if is_superadmin(cfg, &email) {
        Some(email)
    } else {
        None
    }
}
