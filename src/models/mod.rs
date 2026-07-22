use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Hash raw file bytes/text for sync comparison
pub fn hash_content(content: &str) -> String {
    hex::encode(Sha256::digest(content.as_bytes()))
}

/// Validate a local file name (basename only — no path separators).
pub fn validate_file_name(name: &str) -> anyhow::Result<String> {
    let name = name.trim();
    if name.is_empty() {
        anyhow::bail!("file name is empty");
    }
    if name.contains('/') || name.contains('\\') || name == "." || name == ".." {
        anyhow::bail!("file name must be a single path segment");
    }
    if name.starts_with('.') {
        anyhow::bail!("hidden file names are not watched");
    }
    if name.len() > 255 {
        anyhow::bail!("file name too long");
    }
    Ok(name.to_string())
}

/// Watched file record (raw content — any format, not necessarily JSON)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileRecord {
    pub id: String,
    /// Basename under the watch directory
    pub file_name: String,
    /// Full absolute/local path when known
    #[serde(default)]
    pub file_path: String,
    /// Raw file body
    pub content: String,
    pub content_hash: String,
    pub updated_at: String,
    #[serde(default)]
    pub connection_id: Option<String>,
}

/// Local `file_cache` primary key for a (connection, file_name) pair.
///
/// Must **not** reuse the remote row `id`: two connections can share the same remote
/// DB/table (same remote ids) while pointing at different local watch dirs. Remote ids
/// would then collide on `file_cache.id` (global PRIMARY KEY) even though
/// `UNIQUE(connection_id, file_name)` is still satisfied.
pub fn file_cache_row_id(connection_id: &str, file_name: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(connection_id.as_bytes());
    hasher.update([0xff]);
    hasher.update(file_name.as_bytes());
    hex::encode(hasher.finalize())
}

impl FileRecord {
    pub fn from_content(
        file_name: &str,
        content: &str,
        file_path: Option<&str>,
    ) -> anyhow::Result<Self> {
        let file_name = validate_file_name(file_name)?;
        Ok(Self {
            id: uuid::Uuid::new_v4().to_string(),
            file_name: file_name.clone(),
            file_path: file_path.unwrap_or("").to_string(),
            content: content.to_string(),
            content_hash: hash_content(content),
            updated_at: Utc::now().to_rfc3339(),
            connection_id: None,
        })
    }
}

/// Default remote table name for synced file rows
pub const DEFAULT_CONTENT_TABLE: &str = "content_syncs";

/// How to talk to the remote database
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionDriver {
    /// Bunny SQL API over HTTP (`/v2/pipeline`) — see sql-api.md
    #[default]
    SqlApi,
    /// libSQL remote client (`Builder::new_remote`) — see sdk-rust.md
    Libsql,
    /// Local or file SQLite database
    Sqlite,
    /// PostgreSQL
    Postgres,
    /// MySQL
    Mysql,
    /// MariaDB (MySQL wire protocol)
    Mariadb,
    /// MongoDB collection
    Mongodb,
}

impl ConnectionDriver {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SqlApi => "sql_api",
            Self::Libsql => "libsql",
            Self::Sqlite => "sqlite",
            Self::Postgres => "postgres",
            Self::Mysql => "mysql",
            Self::Mariadb => "mariadb",
            Self::Mongodb => "mongodb",
        }
    }

    pub fn parse(s: &str) -> anyhow::Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "sql_api" | "sql-api" | "http" | "pipeline" | "bunny" => Ok(Self::SqlApi),
            "libsql" | "sdk" | "rust_sdk" | "libsql_sdk" => Ok(Self::Libsql),
            "sqlite" | "sqlite3" => Ok(Self::Sqlite),
            "postgres" | "postgresql" | "pg" => Ok(Self::Postgres),
            "mysql" => Ok(Self::Mysql),
            "mariadb" | "maria" => Ok(Self::Mariadb),
            "mongodb" | "mongo" => Ok(Self::Mongodb),
            other => anyhow::bail!(
                "unknown driver `{other}` (sql_api|libsql|sqlite|postgres|mysql|mariadb|mongodb)"
            ),
        }
    }

    /// Whether `table_name` is a SQL table (vs MongoDB collection name).
    #[allow(dead_code)]
    pub fn is_sql(self) -> bool {
        !matches!(self, Self::Mongodb)
    }

    /// Bunny drivers need a dedicated access token; other drivers may put
    /// credentials in the DSN (or need none for plain SQLite files).
    pub fn requires_secret(self) -> bool {
        matches!(self, Self::SqlApi | Self::Libsql)
    }
}

impl std::fmt::Display for ConnectionDriver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Database connection stored in local config
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connection {
    pub id: String,
    pub name: String,
    /// Connection URL / DSN (shape depends on `driver`):
    /// - sql_api: `https://xxx.lite.bunnydb.net/v2/pipeline`
    /// - libsql: `https://xxx…` or `libsql://xxx…`
    /// - sqlite: path or `sqlite:/path/to/db`
    /// - postgres: `postgresql://user@host/db`
    /// - mysql / mariadb: `mysql://user@host/db`
    /// - mongodb: `mongodb://host:27017/content_sync` (or `mongodb+srv://…`)
    pub url: String,
    /// Access token (Bunny) or password (optional if embedded in DSN)
    pub access_token: String,
    /// Remote table name (SQL) or collection name (MongoDB)
    #[serde(default = "default_content_table")]
    pub table_name: String,
    /// Local directory this connection syncs (one dir ↔ one DB table pipeline)
    #[serde(default)]
    pub watch_dir: String,
    /// Backend driver (sql_api, libsql, sqlite, postgres, mysql, mariadb, mongodb)
    #[serde(default)]
    pub driver: ConnectionDriver,
    /// Whether this connection is active for sync
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub last_sync_at: Option<String>,
}

fn default_content_table() -> String {
    DEFAULT_CONTENT_TABLE.to_string()
}

/// Default watch dir for a connection name under ~/.content-sync/files/<slug>
pub fn default_watch_dir_for(name: &str) -> String {
    let slug: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    let slug = if slug.is_empty() {
        "default".to_string()
    } else {
        slug
    };
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".content-sync")
        .join("files")
        .join(slug)
        .display()
        .to_string()
}

/// Safe view of connection (mask access token)
#[derive(Debug, Clone, Serialize)]
pub struct ConnectionView {
    pub id: String,
    pub name: String,
    pub url: String,
    pub access_token_masked: String,
    pub table_name: String,
    pub watch_dir: String,
    pub driver: ConnectionDriver,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
    pub last_error: Option<String>,
    pub last_sync_at: Option<String>,
}

impl From<&Connection> for ConnectionView {
    fn from(c: &Connection) -> Self {
        Self {
            id: c.id.clone(),
            name: c.name.clone(),
            url: c.url.clone(),
            access_token_masked: mask_secret(&c.access_token),
            table_name: c.table_name.clone(),
            watch_dir: c.watch_dir.clone(),
            driver: c.driver,
            enabled: c.enabled,
            created_at: c.created_at.clone(),
            updated_at: c.updated_at.clone(),
            last_error: c.last_error.clone(),
            last_sync_at: c.last_sync_at.clone(),
        }
    }
}

/// Connection mutation response (create / update / toggle / clone).
#[derive(Debug, Clone, Serialize)]
pub struct ConnectionMutationResult {
    #[serde(flatten)]
    pub connection: ConnectionView,
    /// Names of other connections auto-disabled because they share the same pipeline.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disabled_conflicts: Vec<String>,
}

impl ConnectionMutationResult {
    pub fn new(c: &Connection, disabled_conflicts: Vec<String>) -> Self {
        Self {
            connection: ConnectionView::from(c),
            disabled_conflicts,
        }
    }
}

/// Normalize watch_dir for pipeline conflict comparison (trim trailing separators).
pub fn normalize_watch_dir_key(dir: &str) -> String {
    let d = dir.trim().trim_end_matches(['/', '\\']);
    if d.is_empty() {
        "/".to_string()
    } else {
        d.to_string()
    }
}

/// True when two connections target the same sync pipeline and cannot both be enabled.
/// Conflict key: driver + table/collection + watch_dir + url.
pub fn same_sync_pipeline(a: &Connection, b: &Connection) -> bool {
    if a.driver != b.driver {
        return false;
    }
    if a.table_name != b.table_name {
        return false;
    }
    if normalize_watch_dir_key(&a.watch_dir) != normalize_watch_dir_key(&b.watch_dir) {
        return false;
    }
    let ua = normalize_connection_url(&a.url, a.driver);
    let ub = normalize_connection_url(&b.url, b.driver);
    ua == ub
}

/// Normalize URL / DSN for the chosen driver.
pub fn normalize_connection_url(url: &str, driver: ConnectionDriver) -> String {
    let u = url.trim().to_string();
    match driver {
        ConnectionDriver::SqlApi => {
            let u = u.trim_end_matches('/');
            if u.ends_with("/v2/pipeline") || u.contains("/v2/") {
                u.to_string()
            } else {
                format!("{u}/v2/pipeline")
            }
        }
        ConnectionDriver::Libsql => u
            .trim_end_matches('/')
            .trim_end_matches("/v2/pipeline")
            .trim_end_matches("/v2")
            .to_string(),
        ConnectionDriver::Sqlite => {
            // Accept path or sqlite: URL
            if u.starts_with("sqlite:") {
                u
            } else {
                format!("sqlite:{u}")
            }
        }
        ConnectionDriver::Postgres
        | ConnectionDriver::Mysql
        | ConnectionDriver::Mariadb
        | ConnectionDriver::Mongodb => u,
    }
}

/// Validate SQL table identifier: `[A-Za-z_][A-Za-z0-9_]*`, max 64 chars.
pub fn validate_table_name(name: &str) -> anyhow::Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Ok(DEFAULT_CONTENT_TABLE.to_string());
    }
    if name.len() > 64 {
        anyhow::bail!("table_name too long (max 64)");
    }
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        anyhow::bail!("table_name empty");
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        anyhow::bail!("table_name must start with letter or underscore");
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        anyhow::bail!("table_name may only contain letters, digits, underscore");
    }
    let lower = name.to_ascii_lowercase();
    if matches!(lower.as_str(), "sqlite_master" | "sqlite_sequence") {
        anyhow::bail!("reserved table_name");
    }
    Ok(name.to_string())
}

/// Auth token for Web UI login
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthToken {
    pub id: String,
    pub name: String,
    /// SHA-256 hex of the raw token
    pub token_hash: String,
    /// First 8 chars for display
    pub token_prefix: String,
    /// Full raw token (stored for local CLI `token show`; not exposed in Web UI list)
    #[serde(default, skip_serializing)]
    pub raw_token: Option<String>,
    pub enabled: bool,
    pub created_at: String,
    pub last_used_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthTokenView {
    pub id: String,
    pub name: String,
    pub token_prefix: String,
    pub enabled: bool,
    pub created_at: String,
    pub last_used_at: Option<String>,
}

impl From<&AuthToken> for AuthTokenView {
    fn from(t: &AuthToken) -> Self {
        Self {
            id: t.id.clone(),
            name: t.name.clone(),
            token_prefix: t.token_prefix.clone(),
            enabled: t.enabled,
            created_at: t.created_at.clone(),
            last_used_at: t.last_used_at.clone(),
        }
    }
}

/// Session after Web UI login
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub auth_token_id: String,
    pub created_at: String,
    pub expires_at: String,
}

pub fn mask_secret(s: &str) -> String {
    if s.len() <= 8 {
        "****".to_string()
    } else {
        format!("{}…{}", &s[..4], &s[s.len().saturating_sub(4)..])
    }
}

pub fn now_rfc3339() -> String {
    Utc::now().to_rfc3339()
}

pub fn hash_token(raw: &str) -> String {
    hex::encode(Sha256::digest(raw.as_bytes()))
}

pub fn generate_auth_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    format!("sa_{}", hex::encode(bytes))
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncStatus {
    /// Summary of watched dirs (from enabled connections)
    pub watch_dirs: Vec<String>,
    pub running: bool,
    pub local_file_count: usize,
    pub connections_enabled: usize,
    pub last_sync_message: Option<String>,
    pub last_sync_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Fallback / default parent when creating connections without an explicit watch_dir
    #[serde(default = "default_files_root")]
    pub default_files_root: String,
    /// @deprecated kept for older UI clients; prefer per-connection watch_dir
    #[serde(default = "default_files_root")]
    pub watch_dir: String,
    /// When true, run full pull+push on a timer (`poll_interval_secs`).
    /// When false, only initial sync at start + file-watcher / manual Sync now.
    /// Persisted in config-sqlite; survives restarts. Default true.
    #[serde(default = "default_auto_poll")]
    pub auto_poll: bool,
    /// Normal poll interval when connections are healthy (used only if `auto_poll` is true)
    pub poll_interval_secs: u64,
    /// Base backoff (seconds) after a failed remote call; doubles each failure up to max
    #[serde(default = "default_error_backoff_secs")]
    pub error_backoff_secs: u64,
    /// Cap for exponential backoff after repeated failures
    #[serde(default = "default_error_backoff_max_secs")]
    pub error_backoff_max_secs: u64,
    /// Auto-delete sync_log rows older than this many hours (default 48). Set 0 to disable age cleanup.
    #[serde(default = "default_log_retention_hours")]
    pub log_retention_hours: u64,
    pub web_bind: String,
}

fn default_auto_poll() -> bool {
    true
}
fn default_error_backoff_secs() -> u64 {
    120
}
fn default_error_backoff_max_secs() -> u64 {
    900
}
fn default_log_retention_hours() -> u64 {
    48
}

fn default_files_root() -> String {
    dirs::home_dir()
        .map(|h| h.join(".content-sync").join("files").display().to_string())
        .unwrap_or_else(|| "./files".to_string())
}

impl Default for Settings {
    fn default() -> Self {
        let root = default_files_root();
        Self {
            default_files_root: root.clone(),
            watch_dir: root,
            auto_poll: default_auto_poll(),
            poll_interval_secs: 30,
            error_backoff_secs: default_error_backoff_secs(),
            error_backoff_max_secs: default_error_backoff_max_secs(),
            log_retention_hours: default_log_retention_hours(),
            web_bind: "127.0.0.1:8787".to_string(),
        }
    }
}

/// Portable backup of system configuration (settings, connections, auth tokens).
///
/// Intentionally excluded (not system config):
/// - sync logs
/// - file cache (`file_cache` table)
/// - raw file contents under watch directories
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigExport {
    /// Format version for forward compatibility
    #[serde(default = "default_config_export_version")]
    pub version: u32,
    pub exported_at: String,
    pub settings: Settings,
    #[serde(default)]
    pub connections: Vec<Connection>,
    #[serde(default)]
    pub auth_tokens: Vec<AuthTokenExport>,
}

fn default_config_export_version() -> u32 {
    1
}

/// Auth token fields needed to restore Web UI login after import.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthTokenExport {
    pub id: String,
    pub name: String,
    pub token_hash: String,
    pub token_prefix: String,
    /// Full raw token when available (required to log in after restore)
    #[serde(default)]
    pub raw_token: Option<String>,
    pub enabled: bool,
    pub created_at: String,
    #[serde(default)]
    pub last_used_at: Option<String>,
}

impl From<&AuthToken> for AuthTokenExport {
    fn from(t: &AuthToken) -> Self {
        Self {
            id: t.id.clone(),
            name: t.name.clone(),
            token_hash: t.token_hash.clone(),
            token_prefix: t.token_prefix.clone(),
            raw_token: t.raw_token.clone(),
            enabled: t.enabled,
            created_at: t.created_at.clone(),
            last_used_at: t.last_used_at.clone(),
        }
    }
}

/// Build download filename: `export.content.sync.YYYY-MM-DD.HH-MM-SS.json`
/// Special characters become `-`; date and time blocks separated by `.`; no spaces.
pub fn config_export_filename(at: DateTime<Utc>) -> String {
    let stamp = at.format("%Y-%m-%d.%H-%M-%S").to_string();
    format!("export.content.sync.{stamp}.json")
}

#[allow(dead_code)]
pub type Timestamp = DateTime<Utc>;
