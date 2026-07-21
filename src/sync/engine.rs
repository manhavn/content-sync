use crate::config::ConfigDb;
use crate::models::*;
use crate::remote;
use crate::sync::watcher::{
    abs_path, connection_ids_for_path, scan_files, MultiWatcher, TokenFileEvent,
};
use anyhow::{Context, Result};
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{error, info, warn};

/// Per-connection failure backoff to avoid hammering rate-limited APIs
#[derive(Debug, Clone)]
struct ConnBackoff {
    failures: u32,
    /// Do not call remote until this instant
    next_ok_at: Instant,
}

/// Shared state for sync + web UI
pub struct AppState {
    pub db: ConfigDb,
    pub running: AtomicBool,
    pub last_sync_message: RwLock<Option<String>>,
    pub last_sync_at: RwLock<Option<String>>,
    /// Prevent echo: when we write a file from remote, skip next push
    suppress_paths: RwLock<HashMap<PathBuf, String>>,
    /// Last content hash we successfully processed locally (skip no-op watcher noise)
    last_seen_hash: RwLock<HashMap<PathBuf, String>>,
    /// Last content hash confirmed on remote: (connection_id, file_name) → hash
    /// Used so we still push when local cache is warm but remote table is empty.
    last_pushed_hash: RwLock<HashMap<(String, String), String>>,
    reload_flag: AtomicBool,
    /// connection_id → backoff state after failures
    backoff: RwLock<HashMap<String, ConnBackoff>>,
    /// connection_ids that already ran ensure_schema this process
    schema_ready: RwLock<HashSet<String>>,
}

impl AppState {
    pub fn new(db: ConfigDb) -> Arc<Self> {
        Arc::new(Self {
            db,
            running: AtomicBool::new(false),
            last_sync_message: RwLock::new(None),
            last_sync_at: RwLock::new(None),
            suppress_paths: RwLock::new(HashMap::new()),
            last_seen_hash: RwLock::new(HashMap::new()),
            last_pushed_hash: RwLock::new(HashMap::new()),
            reload_flag: AtomicBool::new(false),
            backoff: RwLock::new(HashMap::new()),
            schema_ready: RwLock::new(HashSet::new()),
        })
    }

    pub fn request_reload(&self) {
        self.reload_flag.store(true, Ordering::SeqCst);
    }

    pub fn take_reload(&self) -> bool {
        self.reload_flag.swap(false, Ordering::SeqCst)
    }

    pub fn set_status(&self, msg: impl Into<String>) {
        let m = msg.into();
        info!("{m}");
        let _ = self.db.log_sync("info", &m);
        *self.last_sync_message.write() = Some(m);
        *self.last_sync_at.write() = Some(now_rfc3339());
    }

    pub fn set_error(&self, msg: impl Into<String>) {
        let m = msg.into();
        error!("{m}");
        let _ = self.db.log_sync("error", &m);
        *self.last_sync_message.write() = Some(m);
        *self.last_sync_at.write() = Some(now_rfc3339());
    }

    pub fn status_snapshot(&self) -> SyncStatus {
        let conns = self.db.list_enabled_connections().unwrap_or_default();
        let mut watch_dirs: Vec<String> = conns.iter().map(|c| c.watch_dir.clone()).collect();
        watch_dirs.sort();
        watch_dirs.dedup();
        let local_count = conns
            .iter()
            .map(|c| {
                scan_files(Path::new(&c.watch_dir))
                    .map(|f| f.len())
                    .unwrap_or(0)
            })
            .sum();
        SyncStatus {
            watch_dirs,
            running: self.running.load(Ordering::SeqCst),
            local_file_count: local_count,
            connections_enabled: conns.len(),
            last_sync_message: self.last_sync_message.read().clone(),
            last_sync_at: self.last_sync_at.read().clone(),
        }
    }

    fn is_in_backoff(&self, conn_id: &str) -> bool {
        self.backoff
            .read()
            .get(conn_id)
            .map(|b| Instant::now() < b.next_ok_at)
            .unwrap_or(false)
    }

    fn backoff_remaining_secs(&self, conn_id: &str) -> u64 {
        self.backoff
            .read()
            .get(conn_id)
            .map(|b| {
                b.next_ok_at
                    .saturating_duration_since(Instant::now())
                    .as_secs()
            })
            .unwrap_or(0)
    }

    fn clear_backoff(&self, conn_id: &str) {
        self.backoff.write().remove(conn_id);
    }

    fn register_failure(&self, conn_id: &str, settings: &Settings) -> Duration {
        let base = settings.error_backoff_secs.max(30);
        let max = settings.error_backoff_max_secs.max(base);
        let mut map = self.backoff.write();
        let entry = map.entry(conn_id.to_string()).or_insert(ConnBackoff {
            failures: 0,
            next_ok_at: Instant::now(),
        });
        entry.failures = entry.failures.saturating_add(1);
        let exp = (entry.failures - 1).min(8);
        let delay_secs = base.saturating_mul(1u64 << exp).min(max);
        let delay = Duration::from_secs(delay_secs);
        entry.next_ok_at = Instant::now() + delay;
        delay
    }

    fn max_backoff_remaining(&self) -> Duration {
        let now = Instant::now();
        self.backoff
            .read()
            .values()
            .filter_map(|b| {
                if b.next_ok_at > now {
                    Some(b.next_ok_at - now)
                } else {
                    None
                }
            })
            .max()
            .unwrap_or(Duration::ZERO)
    }

    fn any_in_backoff(&self) -> bool {
        let now = Instant::now();
        self.backoff.read().values().any(|b| b.next_ok_at > now)
    }
}

/// Full bidirectional sync once — each connection uses its own watch_dir + table.
pub async fn sync_once(state: &AppState) -> Result<()> {
    let settings = state.db.get_settings()?;
    if let Ok(n) = state.db.purge_old_sync_logs(settings.log_retention_hours) {
        if n > 0 {
            info!(
                "purged {n} sync log(s) older than {}h",
                settings.log_retention_hours
            );
        }
    }

    let connections = state.db.list_enabled_connections()?;
    if connections.is_empty() {
        state.set_status("No enabled connections — nothing to sync");
        return Ok(());
    }

    let mut active = Vec::new();
    let mut skipped = 0usize;
    for conn in connections {
        if state.is_in_backoff(&conn.id) {
            skipped += 1;
            let wait = state.backoff_remaining_secs(&conn.id);
            info!(
                "[{}] in error backoff — skip remote for ~{wait}s",
                conn.name
            );
            continue;
        }
        if conn.watch_dir.trim().is_empty() {
            warn!("[{}] watch_dir empty — skip", conn.name);
            continue;
        }
        std::fs::create_dir_all(&conn.watch_dir)
            .with_context(|| format!("create watch_dir {}", conn.watch_dir))?;
        active.push(conn);
    }

    if active.is_empty() {
        state.set_status(format!(
            "No active connections to sync ({skipped} in backoff or misconfigured)"
        ));
        return Ok(());
    }

    let mut remote_state: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut total_local = 0usize;
    let mut pushed = 0usize;

    for conn in &active {
        let watch_dir = PathBuf::from(&conn.watch_dir);
        // Pull remote → this connection's directory only
        match pull_connection(state, conn, &watch_dir).await {
            Ok((n, hashes)) => {
                state.clear_backoff(&conn.id);
                state.set_status(format!(
                    "[{}] dir={} table={} pull: wrote {n} local, {} remote row(s)",
                    conn.name,
                    conn.watch_dir,
                    conn.table_name,
                    hashes.len()
                ));
                remote_state.insert(conn.id.clone(), hashes);
                let _ = state
                    .db
                    .set_connection_status(&conn.id, None, Some(&now_rfc3339()));
            }
            Err(e) => {
                let delay = state.register_failure(&conn.id, &settings);
                state.set_error(format!(
                    "[{}] pull failed: {e} — backoff {}s",
                    conn.name,
                    delay.as_secs()
                ));
                let _ = state
                    .db
                    .set_connection_status(&conn.id, Some(&e.to_string()), None);
                continue;
            }
        }

        if state.is_in_backoff(&conn.id) {
            continue;
        }

        let local_files = scan_files(&watch_dir)?;
        total_local += local_files.len();
        for path in &local_files {
            match push_file_to_all(
                state,
                path,
                std::slice::from_ref(conn),
                &settings,
                Some(&remote_state),
            )
            .await
            {
                Ok(n) => pushed += n,
                Err(e) => state.set_error(format!("[{}] push {}: {e}", conn.name, path.display())),
            }
        }
    }

    let skipped_note = if skipped > 0 {
        format!(", {skipped} connection(s) skipped (backoff)")
    } else {
        String::new()
    };
    state.set_status(format!(
        "Sync complete — {} connection(s), {} local file(s) scanned, {} push(es){}",
        active.len(),
        total_local,
        pushed,
        skipped_note
    ));
    Ok(())
}

fn schema_key(conn: &Connection) -> String {
    format!("{}:{}:{}", conn.id, conn.driver.as_str(), conn.table_name)
}

async fn ensure_schema_once(state: &AppState, conn: &Connection) -> Result<()> {
    let key = schema_key(conn);
    if state.schema_ready.read().contains(&key) {
        return Ok(());
    }
    let report = remote::ensure_schema(conn).await?;
    if !report.added_columns.is_empty() {
        info!(
            "[{}] ({}) migrated table `{}`: added columns {}",
            conn.name,
            conn.driver,
            report.table,
            report.added_columns.join(", ")
        );
    } else {
        info!(
            "[{}] ({}) table `{}` schema OK ({} columns)",
            conn.name,
            conn.driver,
            report.table,
            report.columns.len()
        );
    }
    state.schema_ready.write().insert(key);
    Ok(())
}

/// Pull remote → local. Returns (files_written_locally, remote file_name → content_hash).
async fn pull_connection(
    state: &AppState,
    conn: &Connection,
    watch_dir: &Path,
) -> Result<(usize, HashMap<String, String>)> {
    ensure_schema_once(state, conn).await?;
    let remote_files = remote::list_files(conn).await?;
    let mut updated = 0usize;
    let mut remote_hashes = HashMap::new();

    for mut rec in remote_files {
        rec.connection_id = Some(conn.id.clone());
        if rec.file_name.is_empty() {
            continue;
        }
        if rec.content_hash.is_empty() {
            rec.content_hash = hash_content(&rec.content);
        }
        rec.connection_id = Some(conn.id.clone());
        remote_hashes.insert(rec.file_name.clone(), rec.content_hash.clone());
        state.last_pushed_hash.write().insert(
            (conn.id.clone(), rec.file_name.clone()),
            rec.content_hash.clone(),
        );

        let path = watch_dir.join(&rec.file_name);
        rec.file_path = path.display().to_string();

        let local_hash = if path.exists() {
            read_local_hash(&path).ok()
        } else {
            None
        };

        let need_write = match local_hash {
            None => true,
            Some(h) if h != rec.content_hash && !rec.content_hash.is_empty() => true,
            Some(_) => false,
        };

        if need_write {
            let h = rec.content_hash.clone();
            state.suppress_paths.write().insert(path.clone(), h.clone());
            state.last_seen_hash.write().insert(path.clone(), h);
            std::fs::write(&path, &rec.content)
                .with_context(|| format!("write {}", path.display()))?;
            updated += 1;
            info!(
                "wrote local file from remote [{}]: {}",
                conn.name,
                path.display()
            );
        } else if !rec.content_hash.is_empty() {
            state
                .last_seen_hash
                .write()
                .insert(path.clone(), rec.content_hash.clone());
        }

        state.db.upsert_file_cache(&rec)?;
    }
    Ok((updated, remote_hashes))
}

/// Push one local file to connections. Returns number of successful remote upserts.
///
/// `remote_state`: optional map conn_id → (file_name → hash) from the latest pull.
/// If remote already has the same hash, skip that connection. **Local cache alone is never
/// enough to skip** — otherwise an empty remote table never gets filled after a local index.
async fn push_file_to_all(
    state: &AppState,
    path: &Path,
    connections: &[Connection],
    settings: &Settings,
    remote_state: Option<&HashMap<String, HashMap<String, String>>>,
) -> Result<usize> {
    let mut rec = read_file_record(path)?;
    let hash = rec.content_hash.clone();

    // Echo after our own disk write — do not re-push
    if let Some(sup) = state.suppress_paths.write().remove(path) {
        if sup == hash || sup.is_empty() {
            state
                .last_seen_hash
                .write()
                .insert(path.to_path_buf(), hash);
            return Ok(0);
        }
    }

    rec.updated_at = now_rfc3339();
    let mut pushed = 0usize;

    for conn in connections {
        if state.is_in_backoff(&conn.id) {
            continue;
        }

        let already_on_remote = remote_state
            .and_then(|m| m.get(&conn.id))
            .and_then(|files| files.get(&rec.file_name))
            .map(|h| h == &hash)
            .unwrap_or(false)
            || state
                .last_pushed_hash
                .read()
                .get(&(conn.id.clone(), rec.file_name.clone()))
                .map(|h| h == &hash)
                .unwrap_or(false);

        if already_on_remote {
            rec.connection_id = Some(conn.id.clone());
            let _ = state.db.upsert_file_cache(&rec);
            continue;
        }

        match ensure_schema_once(state, conn).await {
            Ok(()) => {}
            Err(e) => {
                let delay = state.register_failure(&conn.id, settings);
                let _ = state
                    .db
                    .set_connection_status(&conn.id, Some(&e.to_string()), None);
                warn!(
                    "schema [{}] failed: {e} — backoff {}s",
                    conn.name,
                    delay.as_secs()
                );
                continue;
            }
        }

        rec.connection_id = Some(conn.id.clone());
        match remote::upsert_file(conn, &rec).await {
            Ok(()) => {
                state.clear_backoff(&conn.id);
                state
                    .last_pushed_hash
                    .write()
                    .insert((conn.id.clone(), rec.file_name.clone()), hash.clone());
                let _ = state
                    .db
                    .set_connection_status(&conn.id, None, Some(&now_rfc3339()));
                let _ = state.db.upsert_file_cache(&rec);
                pushed += 1;
                info!(
                    "pushed {} → [{}] table={} dir={}",
                    rec.file_name, conn.name, conn.table_name, conn.watch_dir
                );
            }
            Err(e) => {
                let delay = state.register_failure(&conn.id, settings);
                let _ = state
                    .db
                    .set_connection_status(&conn.id, Some(&e.to_string()), None);
                state
                    .last_pushed_hash
                    .write()
                    .remove(&(conn.id.clone(), rec.file_name.clone()));
                warn!(
                    "push {} → [{}] failed: {e} — backoff {}s",
                    rec.file_name,
                    conn.name,
                    delay.as_secs()
                );
            }
        }
    }
    state
        .last_seen_hash
        .write()
        .insert(path.to_path_buf(), rec.content_hash.clone());
    Ok(pushed)
}

pub fn read_file_record(path: &Path) -> Result<FileRecord> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow::anyhow!("invalid path"))?;
    let file_name = validate_file_name(name)?;
    let content =
        std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    FileRecord::from_content(&file_name, &content, Some(&path.display().to_string()))
}

fn read_local_hash(path: &Path) -> Result<String> {
    Ok(read_file_record(path)?.content_hash)
}

fn next_poll_delay(state: &AppState, settings: &Settings) -> Duration {
    let healthy = Duration::from_secs(settings.poll_interval_secs.max(5));
    if !state.any_in_backoff() {
        return healthy;
    }
    let remaining = state.max_backoff_remaining();
    let min_error = Duration::from_secs(settings.error_backoff_secs.max(30));
    remaining.max(healthy).max(min_error)
}

/// Background loop: watch files + periodic pull
pub async fn run_sync_loop(state: Arc<AppState>, mut shutdown: tokio::sync::watch::Receiver<bool>) {
    state.running.store(true, Ordering::SeqCst);
    state.set_status("Sync engine started");

    if let Err(e) = sync_once(&state).await {
        state.set_error(format!("initial sync error: {e}"));
    }

    let mut watcher = match create_watcher(&state) {
        Ok(w) => Some(w),
        Err(e) => {
            state.set_error(format!("file watcher failed: {e}"));
            None
        }
    };

    let mut next_poll =
        Instant::now() + next_poll_delay(&state, &state.db.get_settings().unwrap_or_default());

    loop {
        let poll_wait = next_poll.saturating_duration_since(Instant::now());

        tokio::select! {
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    break;
                }
            }
            _ = tokio::time::sleep(poll_wait) => {
                if state.take_reload() {
                    watcher = match create_watcher(&state) {
                        Ok(w) => Some(w),
                        Err(e) => {
                            state.set_error(format!("reload watcher: {e}"));
                            None
                        }
                    };
                }
                if let Err(e) = sync_once(&state).await {
                    state.set_error(format!("periodic sync error: {e}"));
                }
                let settings = state.db.get_settings().unwrap_or_default();
                next_poll = Instant::now() + next_poll_delay(&state, &settings);
            }
            _ = tokio::time::sleep(Duration::from_millis(200)) => {
                if let Some(ref w) = watcher {
                    let events = w.try_recv();
                    if !events.is_empty() {
                        handle_file_events(&state, events).await;
                    }
                }
            }
        }
    }

    state.running.store(false, Ordering::SeqCst);
    state.set_status("Sync engine stopped");
}

fn create_watcher(state: &AppState) -> Result<MultiWatcher> {
    let conns = state.db.list_enabled_connections()?;
    let dirs: Vec<PathBuf> = conns
        .iter()
        .filter(|c| !c.watch_dir.trim().is_empty())
        .map(|c| PathBuf::from(&c.watch_dir))
        .collect();
    for d in &dirs {
        std::fs::create_dir_all(d)?;
    }
    if dirs.is_empty() {
        // Watch nothing useful — empty multi-watcher
        return MultiWatcher::new(&[]);
    }
    MultiWatcher::new(&dirs)
}

fn enabled_conn_dir_map(state: &AppState) -> HashMap<String, PathBuf> {
    state
        .db
        .list_enabled_connections()
        .unwrap_or_default()
        .into_iter()
        .filter(|c| !c.watch_dir.trim().is_empty())
        .map(|c| (c.id.clone(), abs_path(Path::new(&c.watch_dir))))
        .collect()
}

async fn handle_file_events(state: &AppState, events: Vec<TokenFileEvent>) {
    let settings = state.db.get_settings().unwrap_or_default();
    let all_enabled = match state.db.list_enabled_connections() {
        Ok(c) => c,
        Err(e) => {
            state.set_error(format!("list connections: {e}"));
            return;
        }
    };
    let dir_map = enabled_conn_dir_map(state);

    for ev in events {
        match ev {
            TokenFileEvent::Changed(path) => {
                if !path.is_file() {
                    continue;
                }
                let owner_ids = connection_ids_for_path(&path, &dir_map);
                if owner_ids.is_empty() {
                    continue;
                }
                let owners: Vec<Connection> = all_enabled
                    .iter()
                    .filter(|c| owner_ids.contains(&c.id) && !state.is_in_backoff(&c.id))
                    .cloned()
                    .collect();

                let mut rec = match read_file_record(&path) {
                    Ok(r) => r,
                    Err(e) => {
                        warn!("skip unreadable {}: {e}", path.display());
                        continue;
                    }
                };
                let hash = rec.content_hash.clone();

                if let Some(sup) = state.suppress_paths.write().remove(&path) {
                    if sup == hash || sup.is_empty() {
                        state.last_seen_hash.write().insert(path.clone(), hash);
                        continue;
                    }
                }
                if state
                    .last_seen_hash
                    .read()
                    .get(&path)
                    .map(|h| h == &hash)
                    .unwrap_or(false)
                {
                    continue;
                }

                info!("file content changed: {}", path.display());
                state
                    .last_seen_hash
                    .write()
                    .insert(path.clone(), hash.clone());

                if owners.is_empty() {
                    // Still index under each owner id if all in backoff
                    for id in &owner_ids {
                        rec.connection_id = Some(id.clone());
                        let _ = state.db.upsert_file_cache(&rec);
                    }
                    continue;
                }

                for id in &owner_ids {
                    state
                        .last_pushed_hash
                        .write()
                        .remove(&(id.clone(), rec.file_name.clone()));
                }

                match push_file_to_all(state, &path, &owners, &settings, None).await {
                    Ok(n) => {
                        state.set_status(format!(
                            "synced {} → {} connection(s) ({} push)",
                            rec.file_name,
                            owners.len(),
                            n
                        ));
                    }
                    Err(e) => {
                        state.last_seen_hash.write().remove(&path);
                        state.set_error(format!("push after change: {e}"));
                    }
                }
            }
            TokenFileEvent::Removed(path) => {
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default();
                if validate_file_name(name).is_err() {
                    continue;
                }
                let owner_ids = connection_ids_for_path(&path, &dir_map);
                if owner_ids.is_empty() {
                    // path may already be gone — match by parent
                    if let Some(parent) = path.parent() {
                        let parent = abs_path(parent);
                        for (id, dir) in &dir_map {
                            if *dir == parent {
                                let _ = state.db.delete_file_cache(id, name);
                            }
                        }
                    }
                    continue;
                }
                state.set_status(format!("file removed: {name}"));
                for id in &owner_ids {
                    let _ = state.db.delete_file_cache(id, name);
                    if let Some(conn) = all_enabled.iter().find(|c| c.id == *id) {
                        if state.is_in_backoff(&conn.id) {
                            continue;
                        }
                        if let Err(e) = ensure_schema_once(state, conn).await {
                            warn!("schema [{}] on delete: {e}", conn.name);
                            continue;
                        }
                        if let Err(e) = remote::delete_file(conn, name).await {
                            warn!("remote delete {} on [{}]: {e}", name, conn.name);
                        }
                        state
                            .last_pushed_hash
                            .write()
                            .remove(&(conn.id.clone(), name.to_string()));
                    }
                }
            }
        }
    }
}

/// Write a raw file into a connection's watch_dir and push to that connection only.
pub async fn write_and_push(
    state: &AppState,
    connection_id: &str,
    file_name: &str,
    content: &str,
) -> Result<PathBuf> {
    let conn = state
        .db
        .get_connection(connection_id)?
        .ok_or_else(|| anyhow::anyhow!("connection not found"))?;
    if conn.watch_dir.trim().is_empty() {
        anyhow::bail!("connection has empty watch_dir");
    }
    let settings = state.db.get_settings()?;
    let watch_dir = PathBuf::from(&conn.watch_dir);
    std::fs::create_dir_all(&watch_dir)?;
    let file_name = validate_file_name(file_name)?;
    let path = watch_dir.join(&file_name);

    let _ = state.db.delete_file_cache(&conn.id, &file_name);
    let h = hash_content(content);
    state.suppress_paths.write().insert(path.clone(), h.clone());
    state.last_seen_hash.write().insert(path.clone(), h);
    state
        .last_pushed_hash
        .write()
        .remove(&(conn.id.clone(), file_name.clone()));
    std::fs::write(&path, content)?;

    if conn.enabled && !state.is_in_backoff(&conn.id) {
        let n =
            push_file_to_all(state, &path, std::slice::from_ref(&conn), &settings, None).await?;
        info!(
            "write_and_push {} → [{}]: {n} remote upsert(s)",
            file_name, conn.name
        );
    } else {
        let mut rec =
            FileRecord::from_content(&file_name, content, Some(&path.display().to_string()))?;
        rec.connection_id = Some(conn.id.clone());
        state.db.upsert_file_cache(&rec)?;
    }
    Ok(path)
}

pub async fn delete_local_and_remote(
    state: &AppState,
    connection_id: &str,
    file_name: &str,
) -> Result<()> {
    let conn = state
        .db
        .get_connection(connection_id)?
        .ok_or_else(|| anyhow::anyhow!("connection not found"))?;
    let file_name = validate_file_name(file_name)?;
    let path = PathBuf::from(&conn.watch_dir).join(&file_name);
    if path.exists() {
        state
            .suppress_paths
            .write()
            .insert(path.clone(), String::new());
        std::fs::remove_file(&path)?;
    }
    state.db.delete_file_cache(&conn.id, &file_name)?;
    state
        .last_pushed_hash
        .write()
        .remove(&(conn.id.clone(), file_name.clone()));

    if conn.enabled && !state.is_in_backoff(&conn.id) {
        let settings = state.db.get_settings()?;
        if let Err(e) = ensure_schema_once(state, &conn).await {
            let delay = state.register_failure(&conn.id, &settings);
            warn!(
                "schema [{}] on delete: {e} — backoff {}s",
                conn.name,
                delay.as_secs()
            );
        } else if let Err(e) = remote::delete_file(&conn, &file_name).await {
            let delay = state.register_failure(&conn.id, &settings);
            warn!(
                "delete remote {} on [{}]: {e} — backoff {}s",
                file_name,
                conn.name,
                delay.as_secs()
            );
        }
    }
    Ok(())
}

/// Clear all connection backoffs (e.g. after manual Sync now from UI).
pub fn clear_all_backoffs(state: &AppState) {
    state.backoff.write().clear();
}
