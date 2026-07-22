use crate::models::*;
use anyhow::{anyhow, Context, Result};
use parking_lot::Mutex;
use rusqlite::{params, Connection as SqliteConn, OptionalExtension};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Clone)]
pub struct ConfigDb {
    conn: Arc<Mutex<SqliteConn>>,
}

impl ConfigDb {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn =
            SqliteConn::open(path).with_context(|| format!("open config db {}", path.display()))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS settings (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS auth_tokens (
                id           TEXT PRIMARY KEY,
                name         TEXT NOT NULL,
                token_hash   TEXT NOT NULL UNIQUE,
                token_prefix TEXT NOT NULL,
                enabled      INTEGER NOT NULL DEFAULT 1,
                created_at   TEXT NOT NULL,
                last_used_at TEXT
            );

            CREATE TABLE IF NOT EXISTS connections (
                id           TEXT PRIMARY KEY,
                name         TEXT NOT NULL,
                url          TEXT NOT NULL,
                access_token TEXT NOT NULL,
                enabled      INTEGER NOT NULL DEFAULT 1,
                created_at   TEXT NOT NULL,
                updated_at   TEXT NOT NULL,
                last_error   TEXT,
                last_sync_at TEXT,
                table_name   TEXT NOT NULL DEFAULT 'content_syncs',
                driver       TEXT NOT NULL DEFAULT 'sql_api',
                watch_dir    TEXT NOT NULL DEFAULT ''
            );

            CREATE TABLE IF NOT EXISTS sessions (
                id            TEXT PRIMARY KEY,
                auth_token_id TEXT NOT NULL,
                created_at    TEXT NOT NULL,
                expires_at    TEXT NOT NULL,
                FOREIGN KEY (auth_token_id) REFERENCES auth_tokens(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS file_cache (
                id            TEXT PRIMARY KEY,
                connection_id TEXT NOT NULL,
                file_name     TEXT NOT NULL,
                file_path     TEXT,
                content       TEXT,
                content_hash  TEXT,
                updated_at    TEXT NOT NULL,
                UNIQUE(connection_id, file_name)
            );

            CREATE TABLE IF NOT EXISTS sync_log (
                id        INTEGER PRIMARY KEY AUTOINCREMENT,
                level     TEXT NOT NULL,
                message   TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            "#,
        )?;
        // Migrations for existing installs
        let _ = conn.execute("ALTER TABLE auth_tokens ADD COLUMN raw_token TEXT", []);
        let _ = conn.execute(
            "ALTER TABLE connections ADD COLUMN table_name TEXT NOT NULL DEFAULT 'content_syncs'",
            [],
        );
        let _ = conn.execute(
            "UPDATE connections SET table_name = 'content_syncs' WHERE table_name = 'oauth_tokens' OR table_name IS NULL OR table_name = ''",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE connections ADD COLUMN driver TEXT NOT NULL DEFAULT 'sql_api'",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE connections ADD COLUMN watch_dir TEXT NOT NULL DEFAULT ''",
            [],
        );
        // One-time migrate legacy oauth_cache → file_cache
        let has_legacy: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='oauth_cache'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0)
            > 0;
        if has_legacy {
            let _ = conn.execute(
                r#"
                INSERT OR IGNORE INTO file_cache(id, connection_id, file_name, content, content_hash, updated_at)
                SELECT
                    id,
                    COALESCE(NULLIF(connection_id, ''), 'local'),
                    CASE
                        WHEN file_name IS NOT NULL AND file_name != '' THEN file_name
                        WHEN instr(account_name, '::') > 0 THEN substr(account_name, instr(account_name, '::') + 2)
                        ELSE account_name
                    END,
                    content_json,
                    content_hash,
                    updated_at
                FROM oauth_cache
                "#,
                [],
            );
        }
        // Heal orphans left by older builds that deleted connections without cascade
        let _ = conn.execute(
            r#"
            DELETE FROM file_cache
            WHERE connection_id != 'local'
              AND connection_id NOT IN (SELECT id FROM connections)
            "#,
            [],
        );
        Ok(())
    }

    // ── Settings ──────────────────────────────────────────────

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
        let v = stmt
            .query_row(params![key], |r| r.get::<_, String>(0))
            .optional()?;
        Ok(v)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO settings(key, value) VALUES(?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn get_settings(&self) -> Result<Settings> {
        let defaults = Settings::default();
        let watch = self
            .get_setting("watch_dir")?
            .unwrap_or_else(|| defaults.watch_dir.clone());
        let root = self
            .get_setting("default_files_root")?
            .unwrap_or_else(|| watch.clone());
        Ok(Settings {
            default_files_root: root,
            watch_dir: watch,
            poll_interval_secs: self
                .get_setting("poll_interval_secs")?
                .and_then(|s| s.parse().ok())
                .unwrap_or(defaults.poll_interval_secs),
            error_backoff_secs: self
                .get_setting("error_backoff_secs")?
                .and_then(|s| s.parse().ok())
                .unwrap_or(defaults.error_backoff_secs),
            error_backoff_max_secs: self
                .get_setting("error_backoff_max_secs")?
                .and_then(|s| s.parse().ok())
                .unwrap_or(defaults.error_backoff_max_secs),
            log_retention_hours: self
                .get_setting("log_retention_hours")?
                .and_then(|s| s.parse().ok())
                .unwrap_or(defaults.log_retention_hours),
            web_bind: self.get_setting("web_bind")?.unwrap_or(defaults.web_bind),
        })
    }

    pub fn save_settings(&self, s: &Settings) -> Result<()> {
        self.set_setting("watch_dir", &s.watch_dir)?;
        self.set_setting("default_files_root", &s.default_files_root)?;
        self.set_setting("poll_interval_secs", &s.poll_interval_secs.to_string())?;
        self.set_setting("error_backoff_secs", &s.error_backoff_secs.to_string())?;
        self.set_setting(
            "error_backoff_max_secs",
            &s.error_backoff_max_secs.to_string(),
        )?;
        self.set_setting("log_retention_hours", &s.log_retention_hours.to_string())?;
        self.set_setting("web_bind", &s.web_bind)?;
        Ok(())
    }

    // ── Auth tokens (Web UI login) ────────────────────────────

    fn map_auth_token(r: &rusqlite::Row<'_>) -> rusqlite::Result<AuthToken> {
        Ok(AuthToken {
            id: r.get(0)?,
            name: r.get(1)?,
            token_hash: r.get(2)?,
            token_prefix: r.get(3)?,
            enabled: r.get::<_, i32>(4)? != 0,
            created_at: r.get(5)?,
            last_used_at: r.get(6)?,
            raw_token: r.get(7)?,
        })
    }

    /// Create a new auth token. Raw token is stored locally for `token show`.
    pub fn create_auth_token(&self, name: &str) -> Result<(AuthToken, String)> {
        let raw = generate_auth_token();
        let hash = hash_token(&raw);
        let prefix: String = raw.chars().take(12).collect();
        let token = AuthToken {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            token_hash: hash,
            token_prefix: prefix,
            raw_token: Some(raw.clone()),
            enabled: true,
            created_at: now_rfc3339(),
            last_used_at: None,
        };
        {
            let conn = self.conn.lock();
            conn.execute(
                "INSERT INTO auth_tokens(id, name, token_hash, token_prefix, enabled, created_at, last_used_at, raw_token)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                params![
                    token.id,
                    token.name,
                    token.token_hash,
                    token.token_prefix,
                    1i32,
                    token.created_at,
                    Option::<String>::None,
                    raw
                ],
            )?;
        }
        Ok((token, raw))
    }

    pub fn list_auth_tokens(&self) -> Result<Vec<AuthToken>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, name, token_hash, token_prefix, enabled, created_at, last_used_at, raw_token
             FROM auth_tokens ORDER BY created_at DESC",
        )?;
        let rows = stmt
            .query_map([], Self::map_auth_token)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn get_auth_token(&self, id: &str) -> Result<Option<AuthToken>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, name, token_hash, token_prefix, enabled, created_at, last_used_at, raw_token
             FROM auth_tokens WHERE id = ?1",
        )?;
        let row = stmt
            .query_row(params![id], Self::map_auth_token)
            .optional()?;
        Ok(row)
    }

    /// Find by exact name (case-sensitive) or by id.
    pub fn find_auth_token(&self, name_or_id: &str) -> Result<Option<AuthToken>> {
        {
            let conn = self.conn.lock();
            let mut stmt = conn.prepare(
                "SELECT id, name, token_hash, token_prefix, enabled, created_at, last_used_at, raw_token
                 FROM auth_tokens WHERE name = ?1 LIMIT 1",
            )?;
            let by_name = stmt
                .query_row(params![name_or_id], Self::map_auth_token)
                .optional()?;
            if by_name.is_some() {
                return Ok(by_name);
            }
        }
        self.get_auth_token(name_or_id)
    }

    /// Show raw token for CLI. If raw was never stored (legacy row), regenerate and save.
    /// Returns (token, raw, rotated).
    pub fn show_auth_token(&self, name_or_id: &str) -> Result<(AuthToken, String, bool)> {
        let mut token = self
            .find_auth_token(name_or_id)?
            .ok_or_else(|| anyhow!("auth token not found: {name_or_id}"))?;

        if let Some(raw) = token.raw_token.clone() {
            if !raw.is_empty() {
                return Ok((token, raw, false));
            }
        }

        // Legacy rows: only hash was stored — mint a new raw so CLI can recover access.
        let raw = generate_auth_token();
        let hash = hash_token(&raw);
        let prefix: String = raw.chars().take(12).collect();
        {
            let conn = self.conn.lock();
            conn.execute(
                "UPDATE auth_tokens SET token_hash = ?1, token_prefix = ?2, raw_token = ?3 WHERE id = ?4",
                params![hash, prefix, raw, token.id],
            )?;
        }
        token.token_hash = hash;
        token.token_prefix = prefix;
        token.raw_token = Some(raw.clone());
        Ok((token, raw, true))
    }

    pub fn update_auth_token(
        &self,
        id: &str,
        name: Option<&str>,
        enabled: Option<bool>,
    ) -> Result<()> {
        let mut t = self
            .get_auth_token(id)?
            .ok_or_else(|| anyhow!("auth token not found"))?;
        if let Some(n) = name {
            t.name = n.to_string();
        }
        if let Some(e) = enabled {
            t.enabled = e;
        }
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE auth_tokens SET name = ?1, enabled = ?2 WHERE id = ?3",
            params![t.name, if t.enabled { 1i32 } else { 0 }, id],
        )?;
        Ok(())
    }

    pub fn delete_auth_token(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM sessions WHERE auth_token_id = ?1", params![id])?;
        let n = conn.execute("DELETE FROM auth_tokens WHERE id = ?1", params![id])?;
        if n == 0 {
            return Err(anyhow!("auth token not found"));
        }
        Ok(())
    }

    pub fn verify_auth_token(&self, raw: &str) -> Result<Option<AuthToken>> {
        let hash = hash_token(raw);
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, name, token_hash, token_prefix, enabled, created_at, last_used_at, raw_token
             FROM auth_tokens WHERE token_hash = ?1 AND enabled = 1",
        )?;
        let row = stmt
            .query_row(params![hash], Self::map_auth_token)
            .optional()?;
        if let Some(ref t) = row {
            let now = now_rfc3339();
            conn.execute(
                "UPDATE auth_tokens SET last_used_at = ?1 WHERE id = ?2",
                params![now, t.id],
            )?;
        }
        Ok(row)
    }

    // ── Sessions ──────────────────────────────────────────────

    pub fn create_session(&self, auth_token_id: &str, ttl_hours: i64) -> Result<Session> {
        let now = chrono::Utc::now();
        let session = Session {
            id: uuid::Uuid::new_v4().to_string(),
            auth_token_id: auth_token_id.to_string(),
            created_at: now.to_rfc3339(),
            expires_at: (now + chrono::Duration::hours(ttl_hours)).to_rfc3339(),
        };
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO sessions(id, auth_token_id, created_at, expires_at) VALUES(?1,?2,?3,?4)",
            params![
                session.id,
                session.auth_token_id,
                session.created_at,
                session.expires_at
            ],
        )?;
        Ok(session)
    }

    pub fn get_session(&self, id: &str) -> Result<Option<Session>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, auth_token_id, created_at, expires_at FROM sessions WHERE id = ?1",
        )?;
        let row = stmt
            .query_row(params![id], |r| {
                Ok(Session {
                    id: r.get(0)?,
                    auth_token_id: r.get(1)?,
                    created_at: r.get(2)?,
                    expires_at: r.get(3)?,
                })
            })
            .optional()?;
        if let Some(ref s) = row {
            if let Ok(exp) = chrono::DateTime::parse_from_rfc3339(&s.expires_at) {
                if exp < chrono::Utc::now() {
                    drop(stmt);
                    conn.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
                    return Ok(None);
                }
            }
        }
        Ok(row)
    }

    pub fn delete_session(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
        Ok(())
    }

    // ── Connections ───────────────────────────────────────────

    fn map_connection(r: &rusqlite::Row<'_>) -> rusqlite::Result<Connection> {
        let table: String = r.get::<_, String>(9).unwrap_or_default();
        let driver_s: String = r.get::<_, String>(10).unwrap_or_default();
        let driver = ConnectionDriver::parse(&driver_s).unwrap_or_default();
        let watch_dir: String = r.get::<_, String>(11).unwrap_or_default();
        let name: String = r.get(1)?;
        Ok(Connection {
            id: r.get(0)?,
            name: name.clone(),
            url: r.get(2)?,
            access_token: r.get(3)?,
            enabled: r.get::<_, i32>(4)? != 0,
            created_at: r.get(5)?,
            updated_at: r.get(6)?,
            last_error: r.get(7)?,
            last_sync_at: r.get(8)?,
            table_name: if table.is_empty() {
                DEFAULT_CONTENT_TABLE.to_string()
            } else {
                table
            },
            driver,
            watch_dir: if watch_dir.is_empty() {
                default_watch_dir_for(&name)
            } else {
                watch_dir
            },
        })
    }

    pub fn create_connection(
        &self,
        name: &str,
        url: &str,
        access_token: &str,
        table_name: &str,
        watch_dir: Option<&str>,
        driver: ConnectionDriver,
        enabled: bool,
    ) -> Result<Connection> {
        let table_name = validate_table_name(table_name)?;
        let watch_dir = watch_dir
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(|| default_watch_dir_for(name));
        std::fs::create_dir_all(&watch_dir)
            .with_context(|| format!("create watch_dir {watch_dir}"))?;
        let now = now_rfc3339();
        let c = Connection {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            url: normalize_connection_url(url, driver),
            access_token: access_token.to_string(),
            table_name,
            watch_dir,
            driver,
            enabled,
            created_at: now.clone(),
            updated_at: now,
            last_error: None,
            last_sync_at: None,
        };
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO connections(id, name, url, access_token, enabled, created_at, updated_at, last_error, last_sync_at, table_name, driver, watch_dir)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![
                c.id,
                c.name,
                c.url,
                c.access_token,
                if c.enabled { 1i32 } else { 0 },
                c.created_at,
                c.updated_at,
                Option::<String>::None,
                Option::<String>::None,
                c.table_name,
                c.driver.as_str(),
                c.watch_dir
            ],
        )?;
        Ok(c)
    }

    pub fn list_connections(&self) -> Result<Vec<Connection>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, name, url, access_token, enabled, created_at, updated_at, last_error, last_sync_at, table_name, driver, watch_dir
             FROM connections ORDER BY name",
        )?;
        let rows = stmt
            .query_map([], Self::map_connection)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn list_enabled_connections(&self) -> Result<Vec<Connection>> {
        Ok(self
            .list_connections()?
            .into_iter()
            .filter(|c| c.enabled)
            .collect())
    }

    pub fn get_connection(&self, id: &str) -> Result<Option<Connection>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, name, url, access_token, enabled, created_at, updated_at, last_error, last_sync_at, table_name, driver, watch_dir
             FROM connections WHERE id = ?1",
        )?;
        let row = stmt
            .query_row(params![id], Self::map_connection)
            .optional()?;
        Ok(row)
    }

    pub fn update_connection(
        &self,
        id: &str,
        name: Option<&str>,
        url: Option<&str>,
        access_token: Option<&str>,
        table_name: Option<&str>,
        watch_dir: Option<&str>,
        driver: Option<ConnectionDriver>,
        enabled: Option<bool>,
    ) -> Result<Connection> {
        let mut c = self
            .get_connection(id)?
            .ok_or_else(|| anyhow!("connection not found"))?;
        if let Some(n) = name {
            c.name = n.to_string();
        }
        if let Some(d) = driver {
            c.driver = d;
        }
        if let Some(u) = url {
            c.url = normalize_connection_url(u, c.driver);
        } else if driver.is_some() {
            c.url = normalize_connection_url(&c.url, c.driver);
        }
        if let Some(t) = access_token {
            if !t.is_empty() {
                c.access_token = t.to_string();
            }
        }
        if let Some(t) = table_name {
            c.table_name = validate_table_name(t)?;
        }
        if let Some(w) = watch_dir {
            let w = w.trim();
            if !w.is_empty() {
                c.watch_dir = w.to_string();
                std::fs::create_dir_all(&c.watch_dir)
                    .with_context(|| format!("create watch_dir {}", c.watch_dir))?;
            }
        }
        if let Some(e) = enabled {
            c.enabled = e;
        }
        c.updated_at = now_rfc3339();
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE connections SET name=?1, url=?2, access_token=?3, enabled=?4, updated_at=?5, table_name=?6, driver=?7, watch_dir=?8 WHERE id=?9",
            params![
                c.name,
                c.url,
                c.access_token,
                if c.enabled { 1i32 } else { 0 },
                c.updated_at,
                c.table_name,
                c.driver.as_str(),
                c.watch_dir,
                id
            ],
        )?;
        Ok(c)
    }

    pub fn set_connection_status(
        &self,
        id: &str,
        last_error: Option<&str>,
        last_sync_at: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE connections SET last_error = ?1, last_sync_at = COALESCE(?2, last_sync_at) WHERE id = ?3",
            params![last_error, last_sync_at, id],
        )?;
        Ok(())
    }

    pub fn delete_connection(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock();
        let tx = conn.unchecked_transaction()?;
        // Cascade local DB only — do NOT delete watch_dir files on disk
        tx.execute(
            "DELETE FROM file_cache WHERE connection_id = ?1",
            params![id],
        )?;
        let n = tx.execute("DELETE FROM connections WHERE id = ?1", params![id])?;
        if n == 0 {
            return Err(anyhow!("connection not found"));
        }
        // Safety net: drop any other orphan cache rows
        tx.execute(
            r#"
            DELETE FROM file_cache
            WHERE connection_id != 'local'
              AND connection_id NOT IN (SELECT id FROM connections)
            "#,
            [],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Remove `file_cache` rows whose connection no longer exists (cache only, not disk).
    pub fn purge_orphan_file_cache(&self) -> Result<usize> {
        let conn = self.conn.lock();
        let n = conn.execute(
            r#"
            DELETE FROM file_cache
            WHERE connection_id != 'local'
              AND connection_id NOT IN (SELECT id FROM connections)
            "#,
            [],
        )?;
        Ok(n)
    }

    // ── File cache (lean schema) ──────────────────────────────

    pub fn upsert_file_cache(&self, rec: &FileRecord) -> Result<()> {
        let conn_id = rec
            .connection_id
            .clone()
            .unwrap_or_else(|| "local".to_string());
        // Always derive local PK from (connection_id, file_name). Using remote `rec.id`
        // breaks when two connections share one remote table (same remote ids, different
        // watch dirs) — INSERT would hit PRIMARY KEY on id while (conn, name) is new.
        let local_id = file_cache_row_id(&conn_id, &rec.file_name);
        let conn = self.conn.lock();
        conn.execute(
            r#"
            INSERT INTO file_cache(
                id, connection_id, file_name, file_path, content, content_hash, updated_at
            ) VALUES(?1,?2,?3,?4,?5,?6,?7)
            ON CONFLICT(connection_id, file_name) DO UPDATE SET
                id           = excluded.id,
                file_path    = excluded.file_path,
                content      = excluded.content,
                content_hash = excluded.content_hash,
                updated_at   = excluded.updated_at
            "#,
            params![
                local_id,
                conn_id,
                rec.file_name,
                rec.file_path,
                rec.content,
                rec.content_hash,
                rec.updated_at
            ],
        )?;
        Ok(())
    }

    pub fn list_file_cache(&self) -> Result<Vec<FileRecord>> {
        let conns = self.list_connections().unwrap_or_default();
        let conn_map: std::collections::HashMap<String, Connection> =
            conns.into_iter().map(|c| (c.id.clone(), c)).collect();

        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, connection_id, file_name, file_path, content, content_hash, updated_at
             FROM file_cache ORDER BY connection_id, file_name",
        )?;
        let rows = stmt
            .query_map([], |r| {
                let connection_id: String = r.get(1)?;
                let file_name: String = r.get(2)?;
                // Nullable TEXT columns (legacy rows / remote-only cache)
                let mut file_path: String = r.get::<_, Option<String>>(3)?.unwrap_or_default();
                let content: String = r.get::<_, Option<String>>(4)?.unwrap_or_default();
                let content_hash: String = r.get::<_, Option<String>>(5)?.unwrap_or_default();
                let cid = if connection_id == "local" {
                    None
                } else {
                    Some(connection_id.clone())
                };
                if file_path.is_empty() {
                    if let Some(ref id) = cid {
                        if let Some(c) = conn_map.get(id) {
                            file_path = PathBuf::from(&c.watch_dir)
                                .join(&file_name)
                                .display()
                                .to_string();
                        }
                    }
                }
                Ok(FileRecord {
                    id: r.get(0)?,
                    file_name,
                    file_path,
                    content,
                    content_hash,
                    updated_at: r.get(6)?,
                    connection_id: cid,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn delete_file_cache(&self, connection_id: &str, file_name: &str) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "DELETE FROM file_cache WHERE connection_id = ?1 AND file_name = ?2",
            params![connection_id, file_name],
        )?;
        Ok(())
    }

    // ── Sync log ──────────────────────────────────────────────

    pub fn log_sync(&self, level: &str, message: &str) -> Result<()> {
        {
            let conn = self.conn.lock();
            conn.execute(
                "INSERT INTO sync_log(level, message, created_at) VALUES(?1,?2,?3)",
                params![level, message, now_rfc3339()],
            )?;
            // Soft cap by count
            conn.execute(
                "DELETE FROM sync_log WHERE id NOT IN (SELECT id FROM sync_log ORDER BY id DESC LIMIT 2000)",
                [],
            )?;
        }
        // Age-based retention (default 48h)
        let hours = self
            .get_settings()
            .map(|s| s.log_retention_hours)
            .unwrap_or(48);
        let _ = self.purge_old_sync_logs(hours);
        Ok(())
    }

    /// Delete sync_log rows older than `hours`. Returns number of deleted rows.
    /// `hours == 0` skips age cleanup (only count cap applies on write).
    pub fn purge_old_sync_logs(&self, hours: u64) -> Result<usize> {
        if hours == 0 {
            return Ok(0);
        }
        let hours_i = i64::try_from(hours).unwrap_or(i64::MAX);
        let cutoff = (chrono::Utc::now() - chrono::Duration::hours(hours_i)).to_rfc3339();
        let conn = self.conn.lock();
        let n = conn.execute(
            "DELETE FROM sync_log WHERE created_at < ?1",
            params![cutoff],
        )?;
        Ok(n)
    }

    /// Apply retention from current settings.
    pub fn purge_old_sync_logs_from_settings(&self) -> Result<usize> {
        let hours = self.get_settings()?.log_retention_hours;
        self.purge_old_sync_logs(hours)
    }

    pub fn list_sync_log(&self, limit: usize) -> Result<Vec<serde_json::Value>> {
        let _ = self.purge_old_sync_logs_from_settings();
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, level, message, created_at FROM sync_log ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![limit as i64], |r| {
                Ok(serde_json::json!({
                    "id": r.get::<_, i64>(0)?,
                    "level": r.get::<_, String>(1)?,
                    "message": r.get::<_, String>(2)?,
                    "created_at": r.get::<_, String>(3)?,
                }))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}
