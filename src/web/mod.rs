mod routes;

pub use routes::router;

use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "static/"]
pub struct Assets;

/// Fingerprinted static assets referenced from `index.html` (long-cache + `?v=`).
const VERSIONED_ASSETS: &[&str] = &["style.css", "app.js", "i18n.js", "favicon.svg"];

/// Serve an embedded static file with **ETag** and **Cache-Control**.
///
/// - `index.html` / SPA shell: `Cache-Control: no-cache` so clients revalidate; body
///   is skipped on `If-None-Match` match (304). Script/link URLs get `?v=<hash>` so
///   JS/CSS can be cached aggressively without stale deploys.
/// - Other static files: long-lived `public, max-age=31536000, immutable` (URL is
///   content-addressed via the HTML fingerprint query).
pub fn serve_asset(path: &str, headers: &HeaderMap) -> Response {
    let path = if path.is_empty() || path == "/" {
        "index.html"
    } else {
        path.trim_start_matches('/')
    };

    match Assets::get(path) {
        Some(file) => asset_response(path, file, headers, false),
        None => {
            // SPA fallback
            match Assets::get("index.html") {
                Some(file) => asset_response("index.html", file, headers, true),
                None => (StatusCode::NOT_FOUND, "not found").into_response(),
            }
        }
    }
}

fn asset_response(
    path: &str,
    file: rust_embed::EmbeddedFile,
    headers: &HeaderMap,
    spa_fallback: bool,
) -> Response {
    let is_html = path == "index.html" || spa_fallback || path.ends_with(".html");
    let cache_control = if is_html {
        // Always revalidate HTML so new asset fingerprints are picked up quickly.
        "no-cache"
    } else {
        // Fingerprinted via `?v=` from HTML; safe to cache for a long time.
        "public, max-age=31536000, immutable"
    };

    let content_type = if is_html {
        "text/html; charset=utf-8".to_string()
    } else if path.ends_with(".js") {
        "text/javascript; charset=utf-8".to_string()
    } else if path.ends_with(".css") {
        "text/css; charset=utf-8".to_string()
    } else if path.ends_with(".svg") {
        "image/svg+xml".to_string()
    } else {
        mime_guess::from_path(path)
            .first_or_octet_stream()
            .to_string()
    };

    let (body, etag) = if is_html {
        let html = inject_asset_versions(&String::from_utf8_lossy(&file.data));
        let body = html.into_bytes();
        // ETag must cover rewritten markup (linked asset hashes), not only the
        // on-disk index.html bytes.
        let etag = etag_from_bytes(&body);
        (body, etag)
    } else {
        let etag = etag_from_hash(&file.metadata.sha256_hash());
        (file.data.into_owned(), etag)
    };

    if if_none_match_hits(headers, &etag) {
        return (
            StatusCode::NOT_MODIFIED,
            [
                (header::ETAG, etag),
                (header::CACHE_CONTROL, cache_control.to_string()),
            ],
        )
            .into_response();
    }

    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type),
            (header::ETAG, etag),
            (header::CACHE_CONTROL, cache_control.to_string()),
        ],
        body,
    )
        .into_response()
}

/// Rewrite `/style.css`, `/app.js`, … to `/style.css?v=<16-hex>` using each file's SHA-256.
fn inject_asset_versions(html: &str) -> String {
    let mut out = html.to_string();
    for name in VERSIONED_ASSETS {
        let Some(file) = Assets::get(name) else {
            continue;
        };
        let v = short_hash(&file.metadata.sha256_hash());
        let quoted = format!("\"/{name}\"");
        let quoted_v = format!("\"/{name}?v={v}\"");
        out = out.replace(&quoted, &quoted_v);
        let single = format!("'/{name}'");
        let single_v = format!("'/{name}?v={v}'");
        out = out.replace(&single, &single_v);
    }
    out
}

fn short_hash(hash: &[u8; 32]) -> String {
    hex::encode(&hash[..8])
}

fn etag_from_hash(hash: &[u8; 32]) -> String {
    format!("\"{}\"", hex::encode(hash))
}

fn etag_from_bytes(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(data);
    format!("\"{}\"", hex::encode(digest))
}

/// RFC 9110 If-None-Match: true if any tag matches (or `*`).
fn if_none_match_hits(headers: &HeaderMap, etag: &str) -> bool {
    let Some(raw) = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
    else {
        return false;
    };
    let want = strip_etag(etag);
    for part in raw.split(',') {
        let part = part.trim();
        if part == "*" {
            return true;
        }
        if strip_etag(part) == want {
            return true;
        }
    }
    false
}

fn strip_etag(tag: &str) -> &str {
    let t = tag.trim();
    let t = t.strip_prefix("W/").unwrap_or(t).trim();
    t.trim_matches('"')
}
