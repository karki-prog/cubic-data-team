use std::path::PathBuf;

use axum::body::Body;
use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderName, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use hyper_util::rt::TokioIo;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tower::ServiceExt;
use tower_http::services::{ServeDir, ServeFile};

use axum::Json;
use serde_json::json;

use crate::AppState;

const HOP_BY_HOP: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailers",
    "transfer-encoding",
    "upgrade",
    "host",
    "content-length",
];

fn hop(name: &HeaderName) -> bool {
    HOP_BY_HOP.iter().any(|h| name.as_str().eq_ignore_ascii_case(h))
}

pub fn load_app_version(cfg: &crate::config::Config) -> String {
    if let Ok(v) = std::env::var("CUBIC_APP_VERSION") {
        let v = v.trim().to_string();
        if !v.is_empty() {
            return v;
        }
    }
    if !cfg.ui_origin.is_empty() {
        return "dev".into();
    }
    if let Some(dir) = &cfg.static_dir {
        let path = dir.join("version.json");
        if let Ok(raw) = std::fs::read_to_string(path) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) {
                if let Some(v) = val.get("version").and_then(|x| x.as_str()) {
                    let v = v.trim();
                    if !v.is_empty() {
                        return v.to_string();
                    }
                }
            }
        }
    }
    "dev".into()
}

pub async fn version(State(state): State<AppState>, uri: axum::http::Uri) -> impl IntoResponse {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CACHE_CONTROL,
        "no-store, no-cache, must-revalidate".parse().unwrap(),
    );
    // A tab on an older build asks for ?clear=1 before reloading: drop this site's
    // browser cache (never cookies, so nobody is signed out).
    if uri.query().is_some_and(|q| q.split('&').any(|p| p == "clear=1")) {
        headers.insert("clear-site-data", "\"cache\"".parse().unwrap());
    }
    (
        headers,
        Json(json!({
            "ok": true,
            "version": state.app_version,
        })),
    )
}

fn apply_static_cache(path: &str, html_document: bool, mut res: Response) -> Response {
    let immutable = !html_document && path.starts_with("/_next/static/");
    let no_store = html_document
        || path.ends_with(".html")
        || path.ends_with("/version.json")
        // Next page data for in-app navigation: per build, so never reuse an old copy.
        || path.ends_with(".txt")
        || path.ends_with("site.webmanifest")
        || path.ends_with('/')
        || !path
            .rsplit('/')
            .next()
            .unwrap_or("")
            .contains('.');
    let value = if immutable {
        "public, max-age=31536000, immutable"
    } else if no_store {
        "no-store, no-cache, must-revalidate"
    } else {
        "public, max-age=300, must-revalidate"
    };
    res.headers_mut()
        .insert(header::CACHE_CONTROL, value.parse().unwrap());
    if no_store {
        res.headers_mut()
            .insert(header::PRAGMA, "no-cache".parse().unwrap());
        res.headers_mut()
            .insert(header::EXPIRES, "0".parse().unwrap());
    }
    res
}

/// Serve the Next.js UI from a static export, or reverse-proxy to `next dev`.
pub async fn fallback(State(state): State<AppState>, req: Request<Body>) -> Response {
    let path = req.uri().path();
    // Next `trailingSlash` turns /api/auth/google into /api/auth/google/, which
    // does not match Axum routes. Send the browser back to the unslashed path.
    if (path.starts_with("/api/") || path == "/healthz/") && path.len() > 1 && path.ends_with('/') {
        let trimmed = path.trim_end_matches('/');
        let loc = match req.uri().query() {
            Some(q) => format!("{trimmed}?{q}"),
            None => trimmed.to_string(),
        };
        return axum::response::Redirect::permanent(&loc).into_response();
    }
    if path.starts_with("/api/") || path == "/healthz" {
        return StatusCode::NOT_FOUND.into_response();
    }
    if !state.cfg.ui_origin.is_empty() {
        return proxy_ui(&state.cfg.ui_origin, req).await;
    }
    if let Some(dir) = state.cfg.static_dir.clone() {
        return serve_static(dir, req).await;
    }
    (
        StatusCode::NOT_FOUND,
        "UI is not configured. Set CUBIC_UI_ORIGIN (dev) or CUBIC_STATIC_DIR / next export out/.",
    )
        .into_response()
}

async fn serve_static(dir: PathBuf, req: Request<Body>) -> Response {
    let path = req.uri().path().to_string();
    let svc = ServeDir::new(&dir).append_index_html_on_directories(true);
    match svc.oneshot(req).await {
        Ok(res) if res.status() != StatusCode::NOT_FOUND => {
            let html = path.ends_with(".html") || path.ends_with('/');
            return apply_static_cache(&path, html, res.into_response());
        }
        _ => {
            let nested = dir.join(path.trim_start_matches('/')).join("index.html");
            if nested.is_file() {
                match ServeFile::new(nested).oneshot(Request::new(Body::empty())).await {
                    Ok(res) => return apply_static_cache(&path, true, res.into_response()),
                    Err(_) => {}
                }
            }
            let index = dir.join("index.html");
            match ServeFile::new(index).oneshot(Request::new(Body::empty())).await {
                Ok(res) => apply_static_cache(&path, true, res.into_response()),
                Err(_) => StatusCode::NOT_FOUND.into_response(),
            }
        }
    }
}

async fn proxy_ui(origin: &str, req: Request<Body>) -> Response {
    if req.headers().get(header::UPGRADE).is_some() {
        return proxy_upgrade(origin, req).await;
    }
    let path = req
        .uri()
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or("/");
    let target = format!("{origin}{path}");
    let method = req.method().clone();
    let mut headers = HeaderMap::new();
    for (k, v) in req.headers() {
        if hop(k) {
            continue;
        }
        headers.append(k.clone(), v.clone());
    }
    let body = match axum::body::to_bytes(req.into_body(), 32 * 1024 * 1024).await {
        Ok(b) => b,
        Err(err) => {
            return (StatusCode::BAD_GATEWAY, err.to_string()).into_response();
        }
    };
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build();
    let Ok(client) = client else {
        return StatusCode::BAD_GATEWAY.into_response();
    };
    let mut rb = client.request(
        reqwest::Method::from_bytes(method.as_str().as_bytes()).unwrap_or(reqwest::Method::GET),
        &target,
    );
    for (k, v) in &headers {
        if let Ok(val) = v.to_str() {
            rb = rb.header(k.as_str(), val);
        }
    }
    match rb.body(body.to_vec()).send().await {
        Ok(upstream) => {
            let status = StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let mut out = HeaderMap::new();
            for (k, v) in upstream.headers() {
                if hop(&HeaderName::from_bytes(k.as_str().as_bytes()).unwrap_or(header::CONTENT_TYPE)) {
                    continue;
                }
                if k.as_str().eq_ignore_ascii_case("set-cookie") {
                    continue;
                }
                if let Ok(name) = HeaderName::from_bytes(k.as_str().as_bytes()) {
                    if let Ok(val) = v.to_str() {
                        if let Ok(hv) = val.parse() {
                            out.append(name, hv);
                        }
                    }
                }
            }
            let cookies: Vec<String> = upstream
                .headers()
                .get_all("set-cookie")
                .iter()
                .filter_map(|v| v.to_str().ok().map(|s| s.to_string()))
                .collect();
            let bytes = upstream.bytes().await.unwrap_or_default();
            let mut res = Response::new(Body::from(bytes));
            *res.status_mut() = status;
            *res.headers_mut() = out;
            for cookie in cookies {
                if let Ok(val) = cookie.parse() {
                    res.headers_mut().append(header::SET_COOKIE, val);
                }
            }
            res
        }
        Err(err) => (
            StatusCode::BAD_GATEWAY,
            format!("UI proxy failed ({err}). Is next dev running?"),
        )
            .into_response(),
    }
}

async fn proxy_upgrade(origin: &str, req: Request<Body>) -> Response {
    let Ok(parsed) = url::Url::parse(origin) else {
        return StatusCode::BAD_GATEWAY.into_response();
    };
    let host = parsed.host_str().unwrap_or("127.0.0.1").to_string();
    let port = parsed.port_or_known_default().unwrap_or(80);
    let mut upstream = match TcpStream::connect((host.as_str(), port)).await {
        Ok(s) => s,
        Err(err) => return (StatusCode::BAD_GATEWAY, err.to_string()).into_response(),
    };
    let path = req
        .uri()
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or("/")
        .to_string();
    let mut head = format!("{} {} HTTP/1.1\r\nHost: {host}:{port}\r\n", req.method(), path);
    for (k, v) in req.headers() {
        if k == header::HOST {
            continue;
        }
        if let Ok(val) = v.to_str() {
            head.push_str(&format!("{k}: {val}\r\n"));
        }
    }
    head.push_str("\r\n");
    if upstream.write_all(head.as_bytes()).await.is_err() {
        return StatusCode::BAD_GATEWAY.into_response();
    }

    let mut buf = vec![0u8; 16 * 1024];
    let n = match upstream.read(&mut buf).await {
        Ok(0) | Err(_) => return StatusCode::BAD_GATEWAY.into_response(),
        Ok(n) => n,
    };
    let header_end = buf[..n]
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| i + 4);
    let Some(header_end) = header_end else {
        return StatusCode::BAD_GATEWAY.into_response();
    };
    let header_bytes = &buf[..header_end];
    let rest = buf[header_end..n].to_vec();
    let header_text = String::from_utf8_lossy(header_bytes);
    let mut lines = header_text.split("\r\n");
    let status_line = lines.next().unwrap_or("HTTP/1.1 502 Bad Gateway");
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(502);
    let mut out_headers = HeaderMap::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            if let Ok(name) = HeaderName::from_bytes(k.trim().as_bytes()) {
                if let Ok(val) = v.trim().parse() {
                    out_headers.append(name, val);
                }
            }
        }
    }

    let on_upgrade = hyper::upgrade::on(req);
    tokio::spawn(async move {
        let Ok(upgraded) = on_upgrade.await else {
            return;
        };
        let mut client = TokioIo::new(upgraded);
        if !rest.is_empty() {
            let _ = client.write_all(&rest).await;
        }
        let _ = tokio::io::copy_bidirectional(&mut client, &mut upstream).await;
    });

    let mut res = Response::new(Body::empty());
    *res.status_mut() = StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY);
    *res.headers_mut() = out_headers;
    res
}
