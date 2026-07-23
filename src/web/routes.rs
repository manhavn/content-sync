use crate::config;
use crate::models::*;
use crate::remote;
use crate::sync::{self, AppState};
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use std::sync::Arc;
use tower_http::cors::CorsLayer;

const SESSION_COOKIE: &str = "sa_session";

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        // Auth
        .route("/api/login", post(login))
        .route("/api/logout", post(logout))
        .route("/api/me", get(me))
        // Public bootstrap (login screen): whether any auth tokens exist
        .route("/api/bootstrap", get(bootstrap))
        // Status / sync
        .route("/api/status", get(status))
        .route("/api/sync", post(trigger_sync))
        .route("/api/sync/log", get(sync_log))
        // Settings
        .route("/api/settings", get(get_settings).put(put_settings))
        // Graceful self-restart (applies web_bind, reloads process)
        .route("/api/system/restart", post(restart_app))
        // Config export / import (settings + connections + auth tokens; no sync logs)
        // Import: unauthenticated only when no auth tokens yet; otherwise session/token required.
        .route("/api/config/export", get(export_config))
        .route("/api/config/import", post(import_config))
        // Connections
        .route(
            "/api/connections",
            get(list_connections).post(create_connection),
        )
        .route(
            "/api/connections/{id}",
            get(get_connection)
                .put(update_connection)
                .delete(delete_connection),
        )
        .route("/api/connections/{id}/test", post(test_connection))
        .route("/api/connections/{id}/toggle", post(toggle_connection))
        .route("/api/connections/{id}/clone", post(clone_connection))
        // Auth tokens (web UI login keys)
        .route(
            "/api/auth-tokens",
            get(list_auth_tokens).post(create_auth_token),
        )
        .route(
            "/api/auth-tokens/{id}",
            put(update_auth_token).delete(delete_auth_token),
        )
        // Watched files (raw content) — scoped to a connection's watch_dir
        .route("/api/files", get(list_files).post(create_file))
        .route(
            "/api/files/{conn_id}/{name}",
            get(get_file).put(update_file).delete(delete_file),
        )
        // Static
        .route("/", get(index))
        .route("/{*path}", get(static_files))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

// ── Auth helpers ──────────────────────────────────────────────

fn session_id_from(headers: &HeaderMap) -> Option<String> {
    // Prefer Authorization: Bearer
    if let Some(auth) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    {
        if let Some(raw) = auth.strip_prefix("Bearer ") {
            // Could be session id OR auth token — handled in require_auth
            return Some(raw.trim().to_string());
        }
    }
    // Cookie
    if let Some(cookie) = headers.get(header::COOKIE).and_then(|v| v.to_str().ok()) {
        for part in cookie.split(';') {
            let part = part.trim();
            if let Some(v) = part.strip_prefix(&format!("{SESSION_COOKIE}=")) {
                return Some(v.to_string());
            }
        }
    }
    None
}

async fn require_auth(state: &AppState, headers: &HeaderMap) -> Result<AuthToken, ApiError> {
    let Some(cred) = session_id_from(headers) else {
        return Err(ApiError::unauthorized("missing credentials"));
    };

    // Session id?
    if let Ok(Some(session)) = state.db.get_session(&cred) {
        if let Ok(Some(token)) = state.db.get_auth_token(&session.auth_token_id) {
            if token.enabled {
                return Ok(token);
            }
        }
        return Err(ApiError::unauthorized("session invalid"));
    }

    // Raw auth token (API style)
    if let Ok(Some(token)) = state.db.verify_auth_token(&cred) {
        return Ok(token);
    }

    Err(ApiError::unauthorized("invalid credentials"))
}

/// Auth for config import from Web UI:
/// - No auth tokens in DB (first boot) → allow without credentials
/// - Otherwise → require valid session or raw access token (Bearer / cookie)
async fn require_import_auth(state: &AppState, headers: &HeaderMap) -> Result<(), ApiError> {
    let tokens = state.db.list_auth_tokens().map_err(ApiError::internal)?;
    if tokens.is_empty() {
        return Ok(());
    }
    require_auth(state, headers).await?;
    Ok(())
}

// ── Handlers ──────────────────────────────────────────────────

/// Public: login-screen needs to know if a token is required for import.
async fn bootstrap(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let tokens = state.db.list_auth_tokens().map_err(ApiError::internal)?;
    Ok(Json(serde_json::json!({
        "has_auth_tokens": !tokens.is_empty(),
        "auth_token_count": tokens.len(),
    })))
}

async fn index() -> Response {
    crate::web::serve_asset("index.html")
}

async fn static_files(Path(path): Path<String>) -> Response {
    crate::web::serve_asset(&path)
}

#[derive(Deserialize)]
struct LoginBody {
    token: String,
}

async fn login(
    State(state): State<Arc<AppState>>,
    Json(body): Json<LoginBody>,
) -> Result<Response, ApiError> {
    let token = state
        .db
        .verify_auth_token(&body.token)
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::unauthorized("invalid token"))?;

    let session = state
        .db
        .create_session(&token.id, 24 * 7)
        .map_err(ApiError::internal)?;

    let body = serde_json::json!({
        "ok": true,
        "session_id": session.id,
        "name": token.name,
        "expires_at": session.expires_at,
    });

    let cookie = format!(
        "{SESSION_COOKIE}={}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}",
        session.id,
        60 * 60 * 24 * 7
    );

    Ok((
        StatusCode::OK,
        [
            (header::SET_COOKIE, cookie),
            (header::CONTENT_TYPE, "application/json".into()),
        ],
        body.to_string(),
    )
        .into_response())
}

async fn logout(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    if let Some(sid) = session_id_from(&headers) {
        let _ = state.db.delete_session(&sid);
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn me(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let token = require_auth(&state, &headers).await?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "name": token.name,
        "id": token.id,
    })))
}

async fn status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<SyncStatus>, ApiError> {
    require_auth(&state, &headers).await?;
    Ok(Json(state.status_snapshot()))
}

async fn trigger_sync(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_auth(&state, &headers).await?;
    // Manual sync clears error backoff so user can force-retry after fixing credentials
    sync::clear_all_backoffs(&state);
    match sync::sync_once(&state).await {
        Ok(()) => Ok(Json(
            serde_json::json!({ "ok": true, "status": state.status_snapshot() }),
        )),
        Err(e) => Err(ApiError::internal(e)),
    }
}

async fn sync_log(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_auth(&state, &headers).await?;
    let logs = state.db.list_sync_log(100).map_err(ApiError::internal)?;
    Ok(Json(serde_json::json!({ "logs": logs })))
}

async fn get_settings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Settings>, ApiError> {
    require_auth(&state, &headers).await?;
    let s = state.db.get_settings().map_err(ApiError::internal)?;
    Ok(Json(s))
}

/// Partial settings update. Watch dirs live on connections; `watch_dir` /
/// `default_files_root` are optional legacy fields and only overwrite when sent.
#[derive(Deserialize)]
struct SettingsUpdate {
    #[serde(default)]
    default_files_root: Option<String>,
    #[serde(default)]
    watch_dir: Option<String>,
    #[serde(default)]
    auto_poll: Option<bool>,
    #[serde(default)]
    poll_interval_secs: Option<u64>,
    #[serde(default)]
    error_backoff_secs: Option<u64>,
    #[serde(default)]
    error_backoff_max_secs: Option<u64>,
    #[serde(default)]
    log_retention_hours: Option<u64>,
    #[serde(default)]
    web_bind: Option<String>,
}

async fn put_settings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<SettingsUpdate>,
) -> Result<Json<Settings>, ApiError> {
    require_auth(&state, &headers).await?;
    let mut s = state.db.get_settings().map_err(ApiError::internal)?;
    if let Some(v) = body.default_files_root {
        s.default_files_root = v;
    }
    if let Some(v) = body.watch_dir {
        s.watch_dir = v;
    }
    if let Some(v) = body.auto_poll {
        s.auto_poll = v;
    }
    if let Some(v) = body.poll_interval_secs {
        s.poll_interval_secs = v;
    }
    if let Some(v) = body.error_backoff_secs {
        s.error_backoff_secs = v;
    }
    if let Some(v) = body.error_backoff_max_secs {
        s.error_backoff_max_secs = v;
    }
    if let Some(v) = body.log_retention_hours {
        s.log_retention_hours = v;
    }
    if let Some(v) = body.web_bind {
        s.web_bind = v;
    }
    state.db.save_settings(&s).map_err(ApiError::internal)?;
    state.request_reload();
    Ok(Json(s))
}

/// Gracefully stop the HTTP server, then re-exec this process as `serve`.
///
/// Uses in-process re-exec (same PID) so restart works as Docker/Cloud Run PID 1
/// and still applies process-level settings such as `web_bind`.
async fn restart_app(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_auth(&state, &headers).await?;

    if state.shutdown_tx.read().is_none() {
        return Err(ApiError::bad_request(
            "restart is only available while the Web UI server (serve/background) is running",
        ));
    }

    let web_bind = state
        .db
        .get_settings()
        .map_err(ApiError::internal)?
        .web_bind;

    // Flag first so the serve loop re-execs after drain (do not spawn a sibling process —
    // that breaks containers where this binary is PID 1: exit of PID 1 kills the child).
    state.request_restart_after_shutdown();

    let _ = state.db.log_sync(
        "info",
        &format!("Web UI requested process restart (will re-bind {web_bind})"),
    );

    // Delay shutdown slightly so this HTTP response can flush.
    let st = Arc::clone(&state);
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(350)).await;
        if !st.request_shutdown() {
            // Fallback: signal self on Unix
            #[cfg(unix)]
            {
                let _ = config::terminate_process(std::process::id());
            }
        }
    });

    Ok(Json(serde_json::json!({
        "ok": true,
        "web_bind": web_bind,
        "reconnect_in_ms": 2000,
        "message": "Restarting… the page will reconnect automatically."
    })))
}

// ── Config export / import ────────────────────────────────────

async fn export_config(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let data = state.db.export_config().map_err(ApiError::internal)?;
    let filename = config_export_filename(chrono::Utc::now());
    let body = serde_json::to_vec_pretty(&data).map_err(ApiError::internal)?;
    let disposition = format!("attachment; filename=\"{filename}\"");
    Ok((
        StatusCode::OK,
        [
            (
                header::CONTENT_TYPE,
                "application/json; charset=utf-8".to_string(),
            ),
            (header::CONTENT_DISPOSITION, disposition),
        ],
        body,
    )
        .into_response())
}

async fn import_config(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<ConfigExport>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_import_auth(&state, &headers).await?;
    state
        .db
        .import_config(&body)
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    state.request_reload();
    Ok(Json(serde_json::json!({
        "ok": true,
        "settings": true,
        "connections": body.connections.len(),
        "auth_tokens": body.auth_tokens.len(),
        "sync_logs": "not imported",
        "file_data": "not imported",
        "note": "Sessions cleared — re-login may be required. Sync logs and file contents were left unchanged."
    })))
}

// ── Connections ───────────────────────────────────────────────

async fn list_connections(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<ConnectionView>>, ApiError> {
    require_auth(&state, &headers).await?;
    let list = state.db.list_connections().map_err(ApiError::internal)?;
    Ok(Json(list.iter().map(ConnectionView::from).collect()))
}

#[derive(Deserialize)]
struct ConnectionBody {
    name: String,
    url: String,
    /// Bunny access token, or DB password (optional if in DSN / SQLite file)
    #[serde(default)]
    access_token: String,
    #[serde(default = "default_table")]
    table_name: String,
    /// Local directory this connection syncs
    #[serde(default)]
    watch_dir: Option<String>,
    /// sql_api|libsql|sqlite|postgres|mysql|mariadb|mongodb
    #[serde(default)]
    driver: Option<String>,
    #[serde(default = "default_true")]
    enabled: bool,
}

fn default_true() -> bool {
    true
}
fn default_table() -> String {
    crate::models::DEFAULT_CONTENT_TABLE.to_string()
}

async fn create_connection(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<ConnectionBody>,
) -> Result<Json<ConnectionMutationResult>, ApiError> {
    require_auth(&state, &headers).await?;
    if body.name.trim().is_empty() || body.url.trim().is_empty() {
        return Err(ApiError::bad_request("name and url are required"));
    }
    let driver = body
        .driver
        .as_deref()
        .map(crate::models::ConnectionDriver::parse)
        .transpose()
        .map_err(|e| ApiError::bad_request(e.to_string()))?
        .unwrap_or_default();
    if driver.requires_secret() && body.access_token.trim().is_empty() {
        return Err(ApiError::bad_request(
            "access_token is required for sql_api and libsql drivers",
        ));
    }
    let (c, disabled) = state
        .db
        .create_connection(
            &body.name,
            &body.url,
            &body.access_token,
            &body.table_name,
            body.watch_dir.as_deref(),
            driver,
            body.enabled,
        )
        .map_err(ApiError::internal)?;
    state.request_reload();
    Ok(Json(ConnectionMutationResult::new(&c, disabled)))
}

async fn get_connection(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<ConnectionView>, ApiError> {
    require_auth(&state, &headers).await?;
    let c = state
        .db
        .get_connection(&id)
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::not_found("connection not found"))?;
    Ok(Json(ConnectionView::from(&c)))
}

#[derive(Deserialize)]
struct ConnectionUpdate {
    name: Option<String>,
    url: Option<String>,
    access_token: Option<String>,
    table_name: Option<String>,
    watch_dir: Option<String>,
    driver: Option<String>,
    enabled: Option<bool>,
}

async fn update_connection(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<ConnectionUpdate>,
) -> Result<Json<ConnectionMutationResult>, ApiError> {
    require_auth(&state, &headers).await?;
    let driver = body
        .driver
        .as_deref()
        .map(crate::models::ConnectionDriver::parse)
        .transpose()
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    let (c, disabled) = state
        .db
        .update_connection(
            &id,
            body.name.as_deref(),
            body.url.as_deref(),
            body.access_token.as_deref(),
            body.table_name.as_deref(),
            body.watch_dir.as_deref(),
            driver,
            body.enabled,
        )
        .map_err(ApiError::internal)?;
    state.request_reload();
    Ok(Json(ConnectionMutationResult::new(&c, disabled)))
}

async fn delete_connection(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_auth(&state, &headers).await?;
    state
        .db
        .delete_connection(&id)
        .map_err(ApiError::internal)?;
    // file_cache rows cascade in delete_connection; clear runtime maps too
    state.forget_connection(&id);
    state.request_reload();
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn test_connection(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_auth(&state, &headers).await?;
    let c = state
        .db
        .get_connection(&id)
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::not_found("connection not found"))?;
    match remote::test_connection(&c).await {
        Ok(report) => {
            let _ = state
                .db
                .set_connection_status(&id, None, Some(&now_rfc3339()));
            let msg = if report.added_columns.is_empty() {
                format!(
                    "connection OK ({}) — table `{}` schema matches ({} cols)",
                    c.driver,
                    report.table,
                    report.columns.len()
                )
            } else {
                format!(
                    "connection OK ({}) — migrated `{}`, added columns: {}",
                    c.driver,
                    report.table,
                    report.added_columns.join(", ")
                )
            };
            Ok(Json(serde_json::json!({
                "ok": true,
                "message": msg,
                "driver": c.driver,
                "table": report.table,
                "columns": report.columns,
                "added_columns": report.added_columns,
            })))
        }
        Err(e) => {
            let _ = state
                .db
                .set_connection_status(&id, Some(&e.to_string()), None);
            Err(ApiError::bad_request(format!("test failed: {e}")))
        }
    }
}

async fn toggle_connection(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<ConnectionMutationResult>, ApiError> {
    require_auth(&state, &headers).await?;
    let c = state
        .db
        .get_connection(&id)
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::not_found("connection not found"))?;
    let (c, disabled) = state
        .db
        .update_connection(&id, None, None, None, None, None, None, Some(!c.enabled))
        .map_err(ApiError::internal)?;
    state.request_reload();
    Ok(Json(ConnectionMutationResult::new(&c, disabled)))
}

async fn clone_connection(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<ConnectionMutationResult>, ApiError> {
    require_auth(&state, &headers).await?;
    let c = state.db.clone_connection(&id).map_err(|e| {
        let msg = e.to_string();
        if msg.contains("not found") {
            ApiError::not_found(msg)
        } else {
            ApiError::internal(e)
        }
    })?;
    state.request_reload();
    // Clone is always disabled — no conflict disables
    Ok(Json(ConnectionMutationResult::new(&c, Vec::new())))
}

// ── Auth tokens ───────────────────────────────────────────────

async fn list_auth_tokens(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<AuthTokenView>>, ApiError> {
    require_auth(&state, &headers).await?;
    let list = state.db.list_auth_tokens().map_err(ApiError::internal)?;
    Ok(Json(list.iter().map(AuthTokenView::from).collect()))
}

#[derive(Deserialize)]
struct AuthTokenCreate {
    name: String,
}

async fn create_auth_token(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<AuthTokenCreate>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_auth(&state, &headers).await?;
    if body.name.trim().is_empty() {
        return Err(ApiError::bad_request("name required"));
    }
    let (token, raw) = state
        .db
        .create_auth_token(&body.name)
        .map_err(ApiError::internal)?;
    Ok(Json(serde_json::json!({
        "id": token.id,
        "name": token.name,
        "token_prefix": token.token_prefix,
        "enabled": token.enabled,
        "created_at": token.created_at,
        "raw_token": raw,
        "warning": "Copy the raw_token now — it will not be shown again."
    })))
}

#[derive(Deserialize)]
struct AuthTokenUpdate {
    name: Option<String>,
    enabled: Option<bool>,
}

async fn update_auth_token(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<AuthTokenUpdate>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_auth(&state, &headers).await?;
    state
        .db
        .update_auth_token(&id, body.name.as_deref(), body.enabled)
        .map_err(ApiError::internal)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn delete_auth_token(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_auth(&state, &headers).await?;
    state
        .db
        .delete_auth_token(&id)
        .map_err(ApiError::internal)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ── Watched files (raw content, per-connection) ───────────────

async fn list_files(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_auth(&state, &headers).await?;
    // Heal leftovers if a connection was removed without cache cascade
    let _ = state.db.purge_orphan_file_cache();
    let conns = state.db.list_connections().map_err(ApiError::internal)?;
    let conn_names: std::collections::HashMap<_, _> = conns
        .iter()
        .map(|c| (c.id.clone(), c.name.clone()))
        .collect();
    let list = state.db.list_file_cache().map_err(ApiError::internal)?;
    let views: Vec<_> = list
        .iter()
        .filter(|r| {
            // Never surface cache rows for deleted connections
            match r.connection_id.as_deref() {
                None | Some("local") => true,
                Some(id) => conn_names.contains_key(id),
            }
        })
        .map(|r| {
            let preview: String = r.content.chars().take(120).collect();
            let cid = r.connection_id.clone().unwrap_or_default();
            serde_json::json!({
                "id": r.id,
                "file_name": r.file_name,
                "file_path": r.file_path,
                "content_hash": r.content_hash,
                "content_preview": preview,
                "size": r.content.len(),
                "updated_at": r.updated_at,
                "connection_id": r.connection_id,
                "connection_name": conn_names.get(&cid).cloned().unwrap_or_default(),
            })
        })
        .collect();
    Ok(Json(serde_json::json!({ "files": views })))
}

#[derive(Deserialize)]
struct FileBody {
    connection_id: String,
    file_name: String,
    #[serde(default)]
    content: String,
}

async fn create_file(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<FileBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_auth(&state, &headers).await?;
    if body.connection_id.trim().is_empty() || body.file_name.trim().is_empty() {
        return Err(ApiError::bad_request(
            "connection_id and file_name required",
        ));
    }
    let path = sync::write_and_push(&state, &body.connection_id, &body.file_name, &body.content)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "connection_id": body.connection_id,
        "file_name": body.file_name,
        "path": path.display().to_string(),
    })))
}

#[derive(Deserialize)]
struct FilePathParams {
    conn_id: String,
    name: String,
}

async fn get_file(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(p): Path<FilePathParams>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_auth(&state, &headers).await?;
    let list = state.db.list_file_cache().map_err(ApiError::internal)?;
    let rec = list
        .into_iter()
        .find(|r| r.file_name == p.name && r.connection_id.as_deref() == Some(p.conn_id.as_str()))
        .ok_or_else(|| ApiError::not_found("file not found"))?;
    Ok(Json(serde_json::json!({
        "connection_id": rec.connection_id,
        "file_name": rec.file_name,
        "file_path": rec.file_path,
        "content": rec.content,
        "content_hash": rec.content_hash,
        "updated_at": rec.updated_at,
    })))
}

async fn update_file(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(p): Path<FilePathParams>,
    Json(body): Json<FileBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_auth(&state, &headers).await?;
    let path = sync::write_and_push(&state, &p.conn_id, &p.name, &body.content)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "connection_id": p.conn_id,
        "file_name": p.name,
        "path": path.display().to_string(),
    })))
}

async fn delete_file(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(p): Path<FilePathParams>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_auth(&state, &headers).await?;
    sync::delete_local_and_remote(&state, &p.conn_id, &p.name)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ── Error type ────────────────────────────────────────────────

struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn unauthorized(m: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: m.into(),
        }
    }
    fn bad_request(m: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: m.into(),
        }
    }
    fn not_found(m: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: m.into(),
        }
    }
    fn internal(e: impl std::fmt::Display) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: e.to_string(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = serde_json::json!({ "error": self.message });
        (self.status, Json(body)).into_response()
    }
}
