//! The "Cubic AI" ONLYOFFICE plugin — served to the editor, not installed into
//! the Document Server.
//!
//! Registering it through `editorConfig.plugins.pluginsData` means the shared
//! Document Server container (the scrapper's) is never modified. Installing
//! into `sdkjs-plugins/` would change the editor for that project too.
//!
//! Assets are compiled in rather than read from disk so dev and prod behave the
//! same — in dev the UI is proxied to Next and there is no static dir to read.
//!
//! ## CORS
//!
//! The editor page runs on the Document Server's origin and fetches
//! `config.json` from ours, so these responses must be CORS-open. They are
//! static UI code with no secrets. The plugin's own iframe is served from our
//! origin, so its `/api/*` calls stay same-origin and keep the session cookie.

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};

use crate::AppState;

const CONFIG_JSON: &str = include_str!("../plugin/config.json");
const INDEX_HTML: &str = include_str!("../plugin/index.html");
const ICON_PNG: &[u8] = include_bytes!("../plugin/resources/icon.png");
const ICON_2X_PNG: &[u8] = include_bytes!("../plugin/resources/icon@2x.png");

/// GUID from config.json — the editor config autostarts this.
pub const PLUGIN_GUID: &str = "asc.{C0B1C4A1-7E42-4D6B-9F3A-2E5D8B7A1C90}";

fn cors(mut res: Response) -> Response {
    let h = res.headers_mut();
    h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*".parse().unwrap());
    // No credentials here on purpose: with "*" the browser would refuse them
    // anyway, and none of these assets need a session.
    h.insert(header::CACHE_CONTROL, "no-cache".parse().unwrap());
    res
}

/// `GET /office-plugin/{*path}`
///
/// The token rides in the **path**, not a query string: ONLYOFFICE derives the
/// plugin's base directory from the config.json URL, and a `?t=` query broke
/// that derivation — it requested a bare `/office-plugin` and got a 404. With
/// `/office-plugin/<token>/config.json`, the base is
/// `/office-plugin/<token>/` and the manifest's relative `index.html` resolves
/// correctly, carrying the token with it.
pub async fn asset(State(state): State<AppState>, Path(path): Path<String>) -> Response {
    let raw = path.trim_matches('/');
    let (token, asset) = split_token(raw);

    let body_mime: Option<(Vec<u8>, &str)> = match asset {
        "config.json" => Some((CONFIG_JSON.as_bytes().to_vec(), "application/json")),
        // The base URL itself, and the frame URL, both serve the panel.
        "" | "index.html" => {
            let ds = state.cfg.onlyoffice_url.trim().trim_end_matches('/');
            Some((
                INDEX_HTML
                    .replace("__DOCSERVER__", ds)
                    .replace("__TOKEN__", token)
                    .into_bytes(),
                "text/html; charset=utf-8",
            ))
        }
        "resources/icon.png" => Some((ICON_PNG.to_vec(), "image/png")),
        "resources/icon@2x.png" => Some((ICON_2X_PNG.to_vec(), "image/png")),
        _ => None,
    };

    match body_mime {
        Some((body, mime)) => cors(
            (StatusCode::OK, [(header::CONTENT_TYPE, mime)], body).into_response(),
        ),
        None => cors((StatusCode::NOT_FOUND, "plugin asset not found").into_response()),
    }
}

/// Base URL with no path at all (`/office-plugin`) — ONLYOFFICE asks for this.
pub async fn base(State(state): State<AppState>) -> Response {
    asset(State(state), Path(String::new())).await
}

/// Splits `<token>/rest` from a plain `rest`. A JWT is the only thing here with
/// two dots in a single segment, which is what distinguishes the two shapes.
fn split_token(raw: &str) -> (&str, &str) {
    match raw.split_once('/') {
        Some((first, rest)) if first.matches('.').count() == 2 && !first.is_empty() => {
            (first, rest)
        }
        _ => {
            if raw.matches('.').count() == 2 && !raw.contains('/') {
                (raw, "")
            } else {
                ("", raw)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{split_token, CONFIG_JSON, PLUGIN_GUID};

    /// The token lives in the path so ONLYOFFICE's base-URL derivation keeps it.
    #[test]
    fn token_is_split_off_the_front_of_the_path() {
        let jwt = "aaa.bbb.ccc";
        assert_eq!(split_token(&format!("{jwt}/config.json")), (jwt, "config.json"));
        assert_eq!(split_token(&format!("{jwt}/index.html")), (jwt, "index.html"));
        assert_eq!(
            split_token(&format!("{jwt}/resources/icon.png")),
            (jwt, "resources/icon.png")
        );
        // Bare base URL under a token.
        assert_eq!(split_token(jwt), (jwt, ""));
        // Unauthenticated shapes still resolve to the right asset.
        assert_eq!(split_token("config.json"), ("", "config.json"));
        assert_eq!(split_token(""), ("", ""));
        assert_eq!(split_token("resources/icon.png"), ("", "resources/icon.png"));
    }

    /// A left-panel plugin needs `type: "panel"` *inside the variation* — at the
    /// top level it is silently ignored and the plugin never appears.
    #[test]
    fn manifest_declares_a_left_panel_plugin() {
        let v: serde_json::Value = serde_json::from_str(CONFIG_JSON).expect("config.json parses");
        assert_eq!(v["guid"], PLUGIN_GUID, "guid must match what we autostart");
        let var = &v["variations"][0];
        assert_eq!(var["type"], "panel");
        assert_eq!(var["isVisual"], true);
        assert_eq!(var["isModal"], false);
        assert_eq!(var["url"], "index.html");
        assert!(var["EditorsSupport"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e == "word"));
    }
}
