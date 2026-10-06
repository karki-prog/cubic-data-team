//! Resume templates — the .docx files a superadmin picks from before editing.
//!
//! Stored as plain files under `<CUBIC_DATA_DIR>/resume-templates/` with a JSON
//! index beside them, matching how the other stores in `data/` work.
//!
//! ## Why downloads use a token, not the session
//!
//! The Document Server fetches the file **server to server**. It carries no
//! browser cookies, so a session-authenticated download can never work — it
//! would 401 and the editor would open blank. Instead `file_url` mints a short
//! lived signed token and the download route verifies that. Same shape the
//! scrapper uses for its editor callbacks.

use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::extract::{Multipart, Path as AxumPath, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::admin::is_superadmin;
use crate::auth::session_from_headers;
use crate::AppState;

/// Templates are hand-made resumes, not archives — anything larger is a mistake.
const MAX_TEMPLATE_BYTES: usize = 25 * 1024 * 1024;
/// Long enough for the Document Server to fetch, short enough that a leaked URL
/// is worthless within the hour.
const DOWNLOAD_TTL_SECS: i64 = 600;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Template {
    pub id: String,
    pub name: String,
    pub filename: String,
    pub size: u64,
    pub uploaded_by: String,
    pub uploaded_at: String,
}

#[derive(Serialize, Deserialize)]
struct DownloadClaims {
    /// Template id.
    tid: String,
    exp: i64,
}

fn dir(state: &AppState) -> PathBuf {
    state.cfg.data_dir.join("resume-templates")
}

fn index_path(state: &AppState) -> PathBuf {
    dir(state).join("index.json")
}

fn read_index(state: &AppState) -> Vec<Template> {
    std::fs::read_to_string(index_path(state))
        .ok()
        .and_then(|raw| serde_json::from_str::<Vec<Template>>(&raw).ok())
        .unwrap_or_default()
}

fn write_index(state: &AppState, items: &[Template]) -> std::io::Result<()> {
    std::fs::create_dir_all(dir(state))?;
    let body = serde_json::to_string_pretty(items).unwrap_or_else(|_| "[]".into());
    std::fs::write(index_path(state), body)
}

/// Ids land in a filesystem path, so keep them to an alphabet that cannot walk
/// out of the templates directory.
fn safe_id(id: &str) -> Option<String> {
    let clean: String = id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if clean.is_empty() || clean.len() > 64 || clean != id {
        return None;
    }
    Some(clean)
}

fn file_path(state: &AppState, id: &str) -> Option<PathBuf> {
    let id = safe_id(id)?;
    let p = dir(state).join(format!("{id}.docx"));
    // Belt and braces: the joined path must still sit inside the store.
    let base = dir(state);
    if !p.starts_with(&base) {
        return None;
    }
    Some(p)
}

fn require_superadmin(state: &AppState, headers: &HeaderMap) -> Result<String, Response> {
    let Some(user) = session_from_headers(&state.cfg, headers) else {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({ "ok": false, "error": "Sign in first." })),
        )
            .into_response());
    };
    if !is_superadmin(&state.cfg, &user.email) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({ "ok": false, "error": "Superadmin only." })),
        )
            .into_response());
    }
    Ok(user.email)
}

/// `GET /api/admin/templates`
pub async fn list(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(res) = require_superadmin(&state, &headers) {
        return res;
    }
    let items = read_index(&state);
    (StatusCode::OK, Json(json!({ "ok": true, "templates": items }))).into_response()
}

/// `POST /api/admin/templates` — multipart with a `file` part.
pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut form: Multipart,
) -> Response {
    let email = match require_superadmin(&state, &headers) {
        Ok(e) => e,
        Err(res) => return res,
    };

    let mut bytes: Vec<u8> = Vec::new();
    let mut filename = String::new();
    let mut name = String::new();

    while let Ok(Some(field)) = form.next_field().await {
        match field.name().unwrap_or("") {
            "file" => {
                filename = field.file_name().unwrap_or("template.docx").to_string();
                match field.bytes().await {
                    Ok(b) => bytes = b.to_vec(),
                    Err(_) => {
                        return bad("Could not read the uploaded file.");
                    }
                }
            }
            "name" => name = field.text().await.unwrap_or_default(),
            _ => {}
        }
    }

    if bytes.is_empty() {
        return bad("Choose a .docx file to upload.");
    }
    if bytes.len() > MAX_TEMPLATE_BYTES {
        return bad("That file is larger than 25 MB.");
    }
    if !filename.to_lowercase().ends_with(".docx") {
        return bad("Templates must be .docx files.");
    }
    // .docx is a zip; anything else with the extension is not a Word file.
    if bytes.len() < 4 || &bytes[0..2] != b"PK" {
        return bad("That file is not a valid .docx (Word) document.");
    }

    let display = {
        let n = name.trim();
        if n.is_empty() {
            filename.trim_end_matches(".docx").trim_end_matches(".DOCX").to_string()
        } else {
            n.to_string()
        }
    };

    let id = format!(
        "tpl-{}-{}",
        chrono::Utc::now().format("%Y%m%d%H%M%S"),
        std::process::id() % 10_000
    );
    let Some(path) = file_path(&state, &id) else {
        return bad("Could not allocate a template id.");
    };
    if std::fs::create_dir_all(dir(&state)).is_err() || std::fs::write(&path, &bytes).is_err() {
        return server_err("Could not save the template file.");
    }

    let mut items = read_index(&state);
    items.insert(
        0,
        Template {
            id: id.clone(),
            name: display,
            filename,
            size: bytes.len() as u64,
            uploaded_by: email,
            uploaded_at: chrono::Utc::now().to_rfc3339(),
        },
    );
    if write_index(&state, &items).is_err() {
        let _ = std::fs::remove_file(&path);
        return server_err("Could not update the template index.");
    }

    (StatusCode::OK, Json(json!({ "ok": true, "id": id, "templates": items }))).into_response()
}

/// `DELETE /api/admin/templates/{id}`
pub async fn remove(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> Response {
    if let Err(res) = require_superadmin(&state, &headers) {
        return res;
    }
    let Some(path) = file_path(&state, &id) else {
        return bad("Unknown template.");
    };

    let mut items = read_index(&state);
    let before = items.len();
    items.retain(|t| t.id != id);
    if items.len() == before {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "ok": false, "error": "Template not found." })),
        )
            .into_response();
    }
    if write_index(&state, &items).is_err() {
        return server_err("Could not update the template index.");
    }
    // Index first: a stray file is harmless, an index pointing at a missing file
    // is a broken row in the UI.
    let _ = std::fs::remove_file(path);

    (StatusCode::OK, Json(json!({ "ok": true, "templates": items }))).into_response()
}

#[derive(Deserialize)]
pub struct DownloadQuery {
    t: String,
}

/// `GET /api/admin/templates/{id}/file?t=…` — what the Document Server fetches.
///
/// Token-authenticated on purpose: see the module note. No session is involved.
pub async fn download(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Query(q): Query<DownloadQuery>,
) -> Response {
    let secret = download_secret(&state);
    if secret.is_empty() {
        return server_err("Downloads are not configured.");
    }
    let mut validation = Validation::new(Algorithm::HS256);
    validation.set_required_spec_claims(&["exp"]);
    let claims = match decode::<DownloadClaims>(
        &q.t,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    ) {
        Ok(data) => data.claims,
        Err(_) => {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({ "ok": false, "error": "Invalid or expired download link." })),
            )
                .into_response()
        }
    };
    if claims.tid != id {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "ok": false, "error": "Token does not match this template." })),
        )
            .into_response();
    }

    let Some(path) = file_path(&state, &id) else {
        return bad("Unknown template.");
    };
    let Ok(bytes) = std::fs::read(&path) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "ok": false, "error": "Template file is missing." })),
        )
            .into_response();
    };

    let name = read_index(&state)
        .into_iter()
        .find(|t| t.id == id)
        .map(|t| t.filename)
        .unwrap_or_else(|| format!("{id}.docx"));

    Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        )
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", name.replace('"', "")),
        )
        .body(Body::from(bytes))
        .unwrap_or_else(|_| server_err("Could not send the file."))
}

/// Secret used for download tokens. Reuses the ONLYOFFICE secret when set so
/// there is one thing to rotate, falling back to the session secret.
pub fn download_secret(state: &AppState) -> String {
    let oo = state.cfg.onlyoffice_jwt_secret.trim();
    if !oo.is_empty() {
        return oo.to_string();
    }
    state.cfg.jwt_secret.trim().to_string()
}

/// Absolute URL the Document Server can fetch this template from, with a signed
/// short-lived token embedded.
pub fn file_url(state: &AppState, id: &str) -> Option<String> {
    let base = {
        let cb = state.cfg.onlyoffice_callback_origin.trim();
        if cb.is_empty() {
            state.cfg.site_url.trim().trim_end_matches('/').to_string()
        } else {
            cb.to_string()
        }
    };
    if base.is_empty() {
        return None;
    }
    let secret = download_secret(state);
    if secret.is_empty() {
        return None;
    }
    let claims = DownloadClaims {
        tid: id.to_string(),
        exp: (chrono::Utc::now().timestamp()) + DOWNLOAD_TTL_SECS,
    };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .ok()?;
    Some(format!("{base}/api/admin/templates/{id}/file?t={token}"))
}

pub fn lookup(state: &AppState, id: &str) -> Option<Template> {
    read_index(state).into_iter().find(|t| t.id == id)
}

fn bad(msg: &str) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({ "ok": false, "error": msg }))).into_response()
}

fn server_err(msg: &str) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "ok": false, "error": msg })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::safe_id;

    #[test]
    fn ids_cannot_escape_the_template_directory() {
        assert!(safe_id("tpl-20260909-1234").is_some());
        // Anything that could climb out of the store, or smuggle a separator.
        assert!(safe_id("../../etc/passwd").is_none());
        assert!(safe_id("tpl/../secret").is_none());
        assert!(safe_id("tpl.docx").is_none());
        assert!(safe_id("").is_none());
        assert!(safe_id(&"a".repeat(65)).is_none());
    }
}

// Keep `Path` imported for the starts_with guard above without tripping unused
// warnings in builds where the guard is optimised out.
#[allow(dead_code)]
fn _assert_path_type(p: &Path) -> bool {
    p.is_absolute()
}

// ————— Save-back callback ——————————————————————————————————————————————

/// Statuses the Document Server sends when edited content is ready to store.
/// 2 = everyone closed the document, 6 = forcesave while still open.
const STATUS_SAVE: i64 = 2;
const STATUS_FORCESAVE: i64 = 6;

#[derive(Deserialize)]
pub struct CallbackClaims {
    #[allow(dead_code)]
    tid: String,
    exp: i64,
}

/// Hosts we will fetch an edited document from — only the Document Server we
/// configured. Without this the callback is an SSRF primitive: the body tells us
/// a URL and we fetch it.
fn download_host_allowed(state: &AppState, url: &str) -> bool {
    let Ok(target) = reqwest::Url::parse(url) else {
        return false;
    };
    let Some(target_host) = target.host_str() else {
        return false;
    };
    let mut allowed: Vec<String> = Vec::new();
    for base in [
        state.cfg.onlyoffice_url.as_str(),
        state.cfg.onlyoffice_callback_origin.as_str(),
    ] {
        if let Ok(u) = reqwest::Url::parse(base.trim()) {
            if let Some(h) = u.host_str() {
                allowed.push(h.to_lowercase());
            }
        }
    }
    allowed.iter().any(|h| h == &target_host.to_lowercase())
}

/// `POST /api/admin/templates/{id}/callback?t=…`
///
/// Called by the Document Server, never by a browser — so it is authenticated by
/// the same signed token as the download, plus the Document Server's own
/// signature over the body. It must always answer `{"error": 0}` on success or
/// the editor reports the save as failed.
pub async fn save_callback(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Query(q): Query<DownloadQuery>,
    headers: HeaderMap,
    body: String,
) -> Response {
    let secret = download_secret(&state);
    if secret.is_empty() {
        return Json(json!({ "error": 1, "message": "Saving is not configured." })).into_response();
    }

    // 1. Our own link token — proves the callback URL came from us.
    let mut validation = Validation::new(Algorithm::HS256);
    validation.set_required_spec_claims(&["exp"]);
    if decode::<CallbackClaims>(&q.t, &DecodingKey::from_secret(secret.as_bytes()), &validation)
        .is_err()
    {
        return Json(json!({ "error": 1, "message": "Invalid or expired callback link." }))
            .into_response();
    }

    let Ok(payload) = serde_json::from_str::<serde_json::Value>(&body) else {
        return Json(json!({ "error": 1, "message": "Malformed callback body." })).into_response();
    };

    // 2. The Document Server's signature over the body. A secret is configured,
    //    so an unsigned callback is forged — reject it rather than trusting it.
    let body_token = payload
        .get("token")
        .and_then(|t| t.as_str())
        .map(str::to_string)
        .or_else(|| {
            headers
                .get(axum::http::header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
                .map(str::to_string)
        });
    let Some(body_token) = body_token else {
        return Json(json!({ "error": 1, "message": "Unsigned callback rejected." }))
            .into_response();
    };
    let mut body_validation = Validation::new(Algorithm::HS256);
    body_validation.required_spec_claims.clear();
    body_validation.validate_exp = false;
    if decode::<serde_json::Value>(
        &body_token,
        &DecodingKey::from_secret(state.cfg.onlyoffice_jwt_secret.trim().as_bytes()),
        &body_validation,
    )
    .is_err()
    {
        return Json(json!({ "error": 1, "message": "Invalid callback signature." }))
            .into_response();
    }

    let status = payload.get("status").and_then(|s| s.as_i64()).unwrap_or(0);
    if status != STATUS_SAVE && status != STATUS_FORCESAVE {
        // Editing, closed-without-changes, or an error we do not store. The
        // Document Server still expects a success acknowledgement.
        return Json(json!({ "error": 0 })).into_response();
    }

    let Some(url) = payload.get("url").and_then(|u| u.as_str()) else {
        return Json(json!({ "error": 1, "message": "Callback carried no document url." }))
            .into_response();
    };
    if !download_host_allowed(&state, url) {
        tracing::warn!("[templates] refused save fetch from unexpected host: {url}");
        return Json(json!({ "error": 1, "message": "Refused to fetch from that host." }))
            .into_response();
    }
    let Some(path) = file_path(&state, &id) else {
        return Json(json!({ "error": 1, "message": "Unknown template." })).into_response();
    };

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
    {
        Ok(c) => c,
        Err(_) => {
            return Json(json!({ "error": 1, "message": "Could not build HTTP client." }))
                .into_response()
        }
    };
    let bytes = match client.get(url).send().await {
        Ok(res) if res.status().is_success() => match res.bytes().await {
            Ok(b) => b.to_vec(),
            Err(err) => {
                tracing::error!("[templates] reading saved document failed: {err}");
                return Json(json!({ "error": 1, "message": "Could not read the saved document." }))
                    .into_response();
            }
        },
        Ok(res) => {
            tracing::error!("[templates] save fetch returned {}", res.status());
            return Json(json!({ "error": 1, "message": "Could not fetch the saved document." }))
                .into_response();
        }
        Err(err) => {
            tracing::error!("[templates] save fetch failed: {err}");
            return Json(json!({ "error": 1, "message": "Could not fetch the saved document." }))
                .into_response();
        }
    };

    if bytes.len() < 4 || &bytes[0..2] != b"PK" {
        return Json(json!({ "error": 1, "message": "Saved document was not a .docx." }))
            .into_response();
    }

    // Write beside the target then rename, so a failed write cannot leave a
    // half-written template behind.
    let tmp = path.with_extension("docx.tmp");
    if std::fs::write(&tmp, &bytes).is_err() || std::fs::rename(&tmp, &path).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return Json(json!({ "error": 1, "message": "Could not store the saved document." }))
            .into_response();
    }

    let mut items = read_index(&state);
    if let Some(t) = items.iter_mut().find(|t| t.id == id) {
        t.size = bytes.len() as u64;
    }
    let _ = write_index(&state, &items);

    tracing::info!("[templates] saved {} ({} bytes)", id, bytes.len());
    Json(json!({ "error": 0 })).into_response()
}

/// Absolute URL the Document Server posts saves back to.
pub fn callback_url(state: &AppState, id: &str) -> Option<String> {
    let base = {
        let cb = state.cfg.onlyoffice_callback_origin.trim();
        if cb.is_empty() {
            state.cfg.site_url.trim().trim_end_matches('/').to_string()
        } else {
            cb.to_string()
        }
    };
    if base.is_empty() {
        return None;
    }
    let secret = download_secret(state);
    if secret.is_empty() {
        return None;
    }
    let claims = DownloadClaims {
        tid: id.to_string(),
        // Editing sessions outlive a download; give the callback a day.
        exp: chrono::Utc::now().timestamp() + 86_400,
    };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .ok()?;
    Some(format!("{base}/api/admin/templates/{id}/callback?t={token}"))
}
