//! ONLYOFFICE editor configs for the /admin surface.
//!
//! The Document Server is the self-hosted instance the scrapper stack runs (see
//! `deploy/onlyoffice/README.md`). JWT is enabled on it in both directions, so:
//!
//! * every editor config we hand the browser must be signed, or the Document
//!   Server refuses to open it;
//! * anything it posts back to us arrives signed, and an unsigned callback while
//!   a secret is configured must be rejected — otherwise saves can be forged.
//!
//! Superadmin-gated: the whole /admin surface is, and this endpoint re-checks
//! rather than trusting the page gate, because the page gate is client-side.

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use jsonwebtoken::{encode, EncodingKey, Header};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::admin::is_superadmin;
use crate::auth::session_from_headers;
use crate::AppState;

#[derive(Deserialize)]
pub struct EditorQuery {
    /// Absolute URL the Document Server will fetch the file from. It must be
    /// reachable *by the Document Server*, which is not the same as reachable
    /// by the browser. Ignored when `template` is given.
    #[serde(default)]
    url: Option<String>,
    /// Template id to open. Preferred over `url`: the server mints a signed,
    /// short-lived download URL the Document Server can actually fetch, so the
    /// browser never has to know how to reach us from inside Docker.
    #[serde(default)]
    template: Option<String>,
    /// File name shown in the editor; its extension picks the editor type.
    #[serde(default)]
    title: Option<String>,
    /// "edit" (default) or "view".
    #[serde(default)]
    mode: Option<String>,
}

/// Extension from a filename or URL, or None when there isn't a plausible one.
///
/// `rsplit('.').next()` is a trap: on a dotless string it yields the whole
/// string, which is how `title="Template"` became `fileType: "template"` and the
/// Document Server rejected the config. Require a real dot and a sane suffix.
fn file_ext(name: &str) -> Option<String> {
    let stem = name.split('?').next().unwrap_or(name);
    let stem = stem.rsplit('/').next().unwrap_or(stem);
    let (base, ext) = stem.rsplit_once('.')?;
    if base.is_empty() {
        return None;
    }
    let ext = ext.trim().to_lowercase();
    if ext.len() < 2 || ext.len() > 5 || !ext.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    Some(ext)
}

fn doc_type_for(ext: &str) -> &'static str {
    match ext {
        "xlsx" | "xls" | "csv" | "ods" => "cell",
        "pptx" | "ppt" | "odp" => "slide",
        _ => "word",
    }
}

/// `GET /api/admin/office/editor-config` — a signed config the browser hands to
/// the Document Server's `DocsAPI.DocEditor`.
pub async fn editor_config(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<EditorQuery>,
) -> impl IntoResponse {
    let user = session_from_headers(&state.cfg, &headers);
    let Some(user) = user else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "ok": false, "error": "Sign in first." })),
        );
    };
    if !is_superadmin(&state.cfg, &user.email) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "ok": false, "error": "Superadmin only." })),
        );
    }

    let base = state.cfg.onlyoffice_url.trim();
    let secret = state.cfg.onlyoffice_jwt_secret.trim();
    if base.is_empty() || secret.is_empty() {
        // Say which half is missing — a blank editor with no explanation is the
        // single most confusing failure mode here.
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "ok": false,
                "error": "The document editor is not configured on the server.",
                "missing": {
                    "ONLYOFFICE_URL": base.is_empty(),
                    "ONLYOFFICE_JWT_SECRET": secret.is_empty(),
                }
            })),
        );
    }

    // A template id wins: we know how to build a URL the Document Server can
    // reach, and the browser does not.
    let (doc_url, default_title) = if let Some(tid) = q.template.as_deref().filter(|t| !t.is_empty())
    {
        let Some(tpl) = crate::templates::lookup(&state, tid) else {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "ok": false, "error": "That template no longer exists." })),
            );
        };
        let Some(url) = crate::templates::file_url(&state, tid) else {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "ok": false,
                    "error": "Set ONLYOFFICE_CALLBACK_ORIGIN so the editor can fetch templates."
                })),
            );
        };
        (url, tpl.filename)
    } else {
        match q.url.as_deref().map(str::trim).filter(|u| !u.is_empty()) {
            Some(u) => (u.to_string(), "document.docx".to_string()),
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "ok": false, "error": "Pass a template id or a document url." })),
                )
            }
        }
    };

    // fileType comes from the name we control, never from the caller's title.
    // A template's real filename is server-side truth; a client sending
    // title="Template" must not be able to set fileType to "template".
    let ext = file_ext(&default_title).or_else(|| file_ext(&doc_url)).unwrap_or_else(|| "docx".into());
    let title = q.title.filter(|t| !t.trim().is_empty()).unwrap_or(default_title);
    let plugin_token = crate::admin::mint_plugin_token(&state.cfg, &user.email);
    let mode = match q.mode.as_deref() {
        Some("view") => "view",
        _ => "edit",
    };

    // The key identifies a co-editing session. Same key = same session, so it
    // must change whenever the underlying file changes or the server serves
    // stale cached content.
    let key = format!(
        "{:x}",
        md5_like(&format!("{}|{}|{}", doc_url, title, chrono::Utc::now().timestamp()))
    );

    // Only template-backed sessions can be saved back — we own those files. An
    // arbitrary `url` document has nowhere for us to write to, so it stays
    // read-through and the editor simply offers Download instead.
    let callback = q
        .template
        .as_deref()
        .filter(|t| !t.is_empty())
        .and_then(|tid| crate::templates::callback_url(&state, tid));

    let mut config = build_config(BuildConfig {
        ext: &ext,
        key: &key,
        title: &title,
        doc_url: &doc_url,
        mode,
        user_id: &user.email,
        user_name: if user.name.trim().is_empty() { &user.email } else { &user.name },
        callback: callback.as_deref(),
        plugin_origin: Some(state.cfg.site_url.trim().trim_end_matches('/')).filter(|s| !s.is_empty()),
        plugin_token: plugin_token.as_deref(),
    });

    // The Document Server rejects an unsigned config while JWT is enabled.
    let token = match encode(
        &Header::default(),
        &config,
        &EncodingKey::from_secret(secret.as_bytes()),
    ) {
        Ok(t) => t,
        Err(err) => {
            tracing::error!("[office] failed to sign editor config: {err}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "ok": false, "error": "Could not sign the editor config." })),
            );
        }
    };
    if let Some(obj) = config.as_object_mut() {
        obj.insert("token".into(), Value::String(token));
    }

    (
        StatusCode::OK,
        Json(json!({
            "ok": true,
            // Where the browser loads api.js from.
            "docserverUrl": base,
            "config": config,
        })),
    )
}

/// Small non-cryptographic hash for the co-editing key. Not security relevant —
/// the key only has to be stable per session and change per document version.
fn md5_like(input: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in input.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash
}

pub(crate) struct BuildConfig<'a> {
    pub ext: &'a str,
    pub key: &'a str,
    pub title: &'a str,
    pub doc_url: &'a str,
    pub mode: &'a str,
    pub user_id: &'a str,
    pub user_name: &'a str,
    /// None for documents we cannot save back to.
    pub callback: Option<&'a str>,
    /// Origin the *browser* loads the plugin from. None disables the panel.
    pub plugin_origin: Option<&'a str>,
    /// Bearer token the plugin uses instead of the cookie it cannot send.
    pub plugin_token: Option<&'a str>,
}

/// Builds the editor config.
///
/// `callbackUrl` is **omitted** when there is nothing to save back to. Sending
/// it as JSON `null` makes the Document Server reject the whole config with
/// "parameter invalid" — an absent key and a null key are not the same thing to
/// it. Same rule for any other optional field added here later.
pub(crate) fn build_config(c: BuildConfig<'_>) -> Value {
    let mut editor = json!({
        "mode": c.mode,
        "lang": "en-US",
        "user": { "id": c.user_id, "name": c.user_name },
        "customization": {
            "autosave": false,
            "forcesave": true,
            // White UI to match the admin surface. "theme-white" needs Docs
            // 9.0+; the server here is 9.4. The editor stores a user's own
            // theme choice in localStorage and that overrides this value.
            "uiTheme": "theme-white",
        }
    });
    if let (Some(cb), Some(obj)) = (c.callback, editor.as_object_mut()) {
        obj.insert("callbackUrl".into(), Value::String(cb.to_string()));
    }

    // Register the Cubic AI panel with this editing session only. Loading it by
    // URL keeps the shared Document Server untouched — installing into its
    // sdkjs-plugins directory would change the editor for the scrapper too.
    if let (Some(origin), Some(obj)) = (c.plugin_origin, editor.as_object_mut()) {
        obj.insert(
            "plugins".into(),
            json!({
                "pluginsData": [match c.plugin_token {
                    Some(t) => format!("{origin}/office-plugin/{t}/config.json"),
                    None => format!("{origin}/office-plugin/config.json"),
                }],
                "autostart": [crate::plugin::PLUGIN_GUID],
            }),
        );
    }

    json!({
        "document": {
            "fileType": c.ext,
            "key": c.key,
            "title": c.title,
            "url": c.doc_url,
            "permissions": {
                "edit": c.mode == "edit",
                "download": true,
                "print": true,
            }
        },
        "documentType": doc_type_for(c.ext),
        "editorConfig": editor,
    })
}

#[cfg(test)]
mod tests {
    use super::{build_config, file_ext, BuildConfig};

    #[test]
    fn file_ext_refuses_a_name_with_no_extension() {
        // The exact regression: the client sent title="Template".
        assert_eq!(file_ext("Template"), None);
        assert_eq!(file_ext(""), None);
        assert_eq!(file_ext(".docx"), None);
        assert_eq!(file_ext("resume."), None);
    }

    #[test]
    fn file_ext_reads_real_names_and_urls() {
        assert_eq!(file_ext("Starter_Resume.docx").as_deref(), Some("docx"));
        assert_eq!(file_ext("A Resume.DOCX").as_deref(), Some("docx"));
        assert_eq!(
            file_ext("http://host:3000/api/admin/templates/t1/file?t=abc.def").as_deref(),
            None,
            "query strings must not be mistaken for an extension"
        );
        assert_eq!(file_ext("http://h/x/report.xlsx").as_deref(), Some("xlsx"));
    }

    fn cfg(callback: Option<&str>) -> serde_json::Value {
        build_config(BuildConfig {
            ext: "docx",
            key: "abc123",
            title: "Starter_Resume.docx",
            doc_url: "http://host.docker.internal:3000/file.docx",
            mode: "edit",
            user_id: "karki@cubicit.net",
            user_name: "Karki",
            callback,
            plugin_origin: Some("http://localhost:3000"),
            plugin_token: Some("tok"),
        })
    }

    /// A null callbackUrl is rejected by the Document Server as "parameter
    /// invalid", so the key must be absent rather than null.
    #[test]
    fn callback_url_is_omitted_not_null_when_absent() {
        let c = cfg(None);
        let editor = c["editorConfig"].as_object().unwrap();
        assert!(!editor.contains_key("callbackUrl"));
        assert!(!c.to_string().contains("null"));
    }

    #[test]
    fn callback_url_is_present_when_saving_is_possible() {
        let c = cfg(Some("http://host/cb?t=x"));
        assert_eq!(c["editorConfig"]["callbackUrl"], "http://host/cb?t=x");
    }

    #[test]
    fn document_type_follows_the_extension() {
        assert_eq!(cfg(None)["documentType"], "word");
    }
}
