use crate::config::{self, ConfigDb};
use crate::models::*;
use crate::remote;
use crate::sync::{self, AppState};
use clap::{Parser, Subcommand};
use std::sync::Arc;
use tracing::info;

#[derive(Parser, Debug)]
#[command(
    name = "content-sync",
    about = "Content Sync — watch local files and bidirectionally sync raw content to Bunny/libSQL databases",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

impl Cli {
    /// Whether this invocation should silence runtime tracing / serve logs.
    pub fn no_log(&self) -> bool {
        match &self.command {
            Commands::Serve { no_log, .. } | Commands::Background { no_log, .. } => *no_log,
            _ => false,
        }
    }
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Initialize config directory and default settings
    Init {
        /// Watch directory for synced files (raw content)
        #[arg(long)]
        watch_dir: Option<String>,
    },

    /// Start Web UI + file watcher + sync engine (foreground)
    Serve {
        /// Bind address (overrides settings)
        #[arg(long, short)]
        bind: Option<String>,
        /// Disable background file watcher / poll (API only)
        #[arg(long)]
        no_sync: bool,
        /// Disable runtime logs (tracing + serve banner)
        #[arg(long)]
        no_log: bool,
    },

    /// Start Web UI + file watcher + sync engine in the background (daemon)
    Background {
        /// Bind address (overrides settings)
        #[arg(long, short)]
        bind: Option<String>,
        /// Disable background file watcher / poll (API only)
        #[arg(long)]
        no_sync: bool,
        /// Disable runtime logs and do not write the background log file
        #[arg(long)]
        no_log: bool,
    },

    /// Stop the background process started by `background`
    Quit,

    /// Run a one-shot bidirectional sync and exit
    Sync,

    /// Manage Web UI auth tokens
    #[command(subcommand)]
    Token(TokenCmd),

    /// Manage remote database connections
    #[command(subcommand)]
    Connection(ConnectionCmd),

    /// Show config paths and status
    Status,

    /// Print config directory path
    ConfigPath,
}

#[derive(Subcommand, Debug)]
pub enum TokenCmd {
    /// Create a new auth token for Web UI login
    Create {
        #[arg(long, short)]
        name: String,
    },
    /// Show (print) raw auth token by name or id — e.g. `token show admin`
    Show {
        /// Token name (e.g. admin) or id
        name_or_id: String,
    },
    /// List auth tokens
    List,
    /// Delete an auth token by id
    Delete { id: String },
    /// Enable / disable
    Set {
        id: String,
        #[arg(long)]
        enabled: Option<bool>,
        #[arg(long)]
        name: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum ConnectionCmd {
    /// Add a database connection
    Add {
        #[arg(long)]
        name: String,
        /// DSN / URL (shape depends on --driver)
        #[arg(long)]
        url: String,
        /// Bunny access token, or DB password (optional if in DSN / SQLite)
        #[arg(long, default_value = "")]
        access_token: String,
        /// Remote table (SQL) or collection (MongoDB); default: content_syncs
        #[arg(long, default_value = "content_syncs")]
        table: String,
        /// Local directory to sync for this connection
        #[arg(long)]
        watch_dir: Option<String>,
        /// Driver: sql_api|libsql|sqlite|postgres|mysql|mariadb|mongodb
        #[arg(long, default_value = "sql_api")]
        driver: String,
        #[arg(long, default_value_t = true)]
        enabled: bool,
    },
    List,
    Delete {
        id: String,
    },
    /// Enable or disable a connection
    Set {
        id: String,
        #[arg(long)]
        enabled: Option<bool>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        url: Option<String>,
        #[arg(long)]
        access_token: Option<String>,
        /// Remote table (SQL) or collection (MongoDB)
        #[arg(long)]
        table: Option<String>,
        /// Local watch directory
        #[arg(long)]
        watch_dir: Option<String>,
        /// Driver: sql_api|libsql|sqlite|postgres|mysql|mariadb|mongodb
        #[arg(long)]
        driver: Option<String>,
    },
    /// Test connectivity and ensure/migrate remote table/collection schema
    Test {
        id: String,
    },
}

pub async fn run(cli: Cli) -> anyhow::Result<()> {
    config::ensure_config_dir()?;
    let db_path = config::config_db_path();
    let db = ConfigDb::open(&db_path)?;

    // Seed default settings if empty
    if db.get_setting("watch_dir")?.is_none() {
        let defaults = Settings::default();
        db.save_settings(&defaults)?;
        std::fs::create_dir_all(&defaults.watch_dir)?;
    }

    match cli.command {
        Commands::Init { watch_dir } => {
            let mut s = db.get_settings()?;
            if let Some(w) = watch_dir {
                s.watch_dir = w;
            }
            db.save_settings(&s)?;
            std::fs::create_dir_all(&s.watch_dir)?;
            println!("Config dir : {}", config::config_dir().display());
            println!("Config db  : {}", db_path.display());
            println!("Watch dir  : {}", s.watch_dir);
            println!("Web bind   : {}", s.web_bind);

            // Create first admin token if none
            let tokens = db.list_auth_tokens()?;
            if tokens.is_empty() {
                let (t, raw) = db.create_auth_token("admin")?;
                println!();
                println!("Created initial auth token:");
                println!("  id   : {}", t.id);
                println!("  name : {}", t.name);
                println!("  token: {raw}");
                println!("  (retrieve later: content-sync token show admin)");
            } else {
                println!("Auth tokens already exist ({}).", tokens.len());
                println!("Show a token: content-sync token show <name>");
            }
            Ok(())
        }

        Commands::ConfigPath => {
            println!("{}", config::config_dir().display());
            Ok(())
        }

        Commands::Status => {
            let s = db.get_settings()?;
            let conns = db.list_connections()?;
            let tokens = db.list_auth_tokens()?;
            let files = db.list_file_cache()?;
            println!("Config dir     : {}", config::config_dir().display());
            println!("Config sqlite  : {}", db_path.display());
            println!("Default files  : {}", s.default_files_root);
            println!("Poll interval  : {}s", s.poll_interval_secs);
            println!(
                "Error backoff  : {}s base / {}s max",
                s.error_backoff_secs, s.error_backoff_max_secs
            );
            if s.log_retention_hours == 0 {
                println!("Log retention  : disabled (age cleanup off)");
            } else {
                println!("Log retention  : {}h", s.log_retention_hours);
            }
            println!("Web bind       : {}", s.web_bind);
            match config::read_daemon_pid() {
                Some(pid) if config::process_alive(pid) => {
                    println!("Background     : running (pid {pid})");
                }
                Some(pid) => {
                    println!("Background     : not running (stale pid {pid})");
                }
                None => {
                    println!("Background     : not running");
                }
            }
            println!("Auth tokens    : {}", tokens.len());
            println!(
                "Connections    : {} ({} enabled)",
                conns.len(),
                conns.iter().filter(|c| c.enabled).count()
            );
            println!("Cached files   : {}", files.len());
            for c in &conns {
                println!(
                    "  - [{}] {} table={} dir={} {}",
                    if c.enabled { "ON " } else { "OFF" },
                    c.name,
                    c.table_name,
                    c.watch_dir,
                    c.last_error.as_deref().unwrap_or("ok")
                );
            }
            Ok(())
        }

        Commands::Token(cmd) => match cmd {
            TokenCmd::Create { name } => {
                let (t, raw) = db.create_auth_token(&name)?;
                println!("Created auth token");
                println!("  id    : {}", t.id);
                println!("  name  : {}", t.name);
                println!("  token : {raw}");
                println!("Retrieve later: content-sync token show {}", t.name);
                Ok(())
            }
            TokenCmd::Show { name_or_id } => {
                let (t, raw, rotated) = db.show_auth_token(&name_or_id)?;
                if rotated {
                    println!(
                        "note: legacy token had no stored raw value — issued a new one (old value no longer works)"
                    );
                }
                println!("id      : {}", t.id);
                println!("name    : {}", t.name);
                println!("enabled : {}", t.enabled);
                println!("prefix  : {}…", t.token_prefix);
                println!("token   : {raw}");
                Ok(())
            }
            TokenCmd::List => {
                for t in db.list_auth_tokens()? {
                    println!(
                        "{}  {:8}  {}…  enabled={}  created={}  last_used={}",
                        t.id,
                        t.name,
                        t.token_prefix,
                        t.enabled,
                        t.created_at,
                        t.last_used_at.as_deref().unwrap_or("-")
                    );
                }
                Ok(())
            }
            TokenCmd::Delete { id } => {
                db.delete_auth_token(&id)?;
                println!("Deleted {id}");
                Ok(())
            }
            TokenCmd::Set { id, enabled, name } => {
                db.update_auth_token(&id, name.as_deref(), enabled)?;
                println!("Updated {id}");
                Ok(())
            }
        },

        Commands::Connection(cmd) => match cmd {
            ConnectionCmd::Add {
                name,
                url,
                access_token,
                table,
                watch_dir,
                driver,
                enabled,
            } => {
                let driver = ConnectionDriver::parse(&driver)?;
                if driver.requires_secret() && access_token.trim().is_empty() {
                    anyhow::bail!("--access-token is required for sql_api and libsql drivers");
                }
                let c = db.create_connection(
                    &name,
                    &url,
                    &access_token,
                    &table,
                    watch_dir.as_deref(),
                    driver,
                    enabled,
                )?;
                println!("Created connection {}", c.id);
                println!("  name     : {}", c.name);
                println!("  driver   : {}", c.driver);
                println!("  url      : {}", c.url);
                println!("  table    : {}", c.table_name);
                println!("  watch_dir: {}", c.watch_dir);
                println!("  on       : {}", c.enabled);
                Ok(())
            }
            ConnectionCmd::List => {
                for c in db.list_connections()? {
                    println!(
                        "{}  [{:3}]  {}  driver={}  table={}  dir={}  {}",
                        c.id,
                        if c.enabled { "ON" } else { "OFF" },
                        c.name,
                        c.driver,
                        c.table_name,
                        c.watch_dir,
                        c.url,
                    );
                }
                Ok(())
            }
            ConnectionCmd::Delete { id } => {
                db.delete_connection(&id)?;
                println!("Deleted {id}");
                Ok(())
            }
            ConnectionCmd::Set {
                id,
                enabled,
                name,
                url,
                access_token,
                table,
                watch_dir,
                driver,
            } => {
                let driver = driver.as_deref().map(ConnectionDriver::parse).transpose()?;
                let c = db.update_connection(
                    &id,
                    name.as_deref(),
                    url.as_deref(),
                    access_token.as_deref(),
                    table.as_deref(),
                    watch_dir.as_deref(),
                    driver,
                    enabled,
                )?;
                println!(
                    "Updated {} enabled={} driver={} table={} dir={}",
                    c.id, c.enabled, c.driver, c.table_name, c.watch_dir
                );
                Ok(())
            }
            ConnectionCmd::Test { id } => {
                let c = db
                    .get_connection(&id)?
                    .ok_or_else(|| anyhow::anyhow!("connection not found"))?;
                println!(
                    "Testing {} driver={} table=`{}` dir={} …",
                    c.name, c.driver, c.table_name, c.watch_dir
                );
                let report = remote::test_connection(&c).await?;
                db.set_connection_status(&id, None, Some(&now_rfc3339()))?;
                println!("OK — table `{}` via {}", report.table, c.driver);
                println!("  columns: {}", report.columns.join(", "));
                if !report.added_columns.is_empty() {
                    println!("  migrated: added {}", report.added_columns.join(", "));
                }
                Ok(())
            }
        },

        Commands::Sync => {
            let state = AppState::new(db);
            sync::sync_once(&state).await?;
            println!(
                "{}",
                state.last_sync_message.read().clone().unwrap_or_default()
            );
            Ok(())
        }

        Commands::Serve {
            bind,
            no_sync,
            no_log,
        } => {
            let mut settings = db.get_settings()?;
            if let Some(b) = bind {
                settings.web_bind = b;
                db.save_settings(&settings)?;
            }
            let state = AppState::new(db);
            let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

            let sync_handle = if !no_sync {
                let st = Arc::clone(&state);
                Some(tokio::spawn(async move {
                    sync::run_sync_loop(st, shutdown_rx).await;
                }))
            } else {
                None
            };

            let app = crate::web::router(Arc::clone(&state));
            let listener = tokio::net::TcpListener::bind(&settings.web_bind).await?;
            let addr = listener.local_addr()?;
            if !no_log {
                info!("Web UI listening on http://{addr}");
                println!("Content Sync Web UI → http://{addr}");
                println!("Config            → {}", config::config_dir().display());
                println!("Press Ctrl+C to stop.");
            }

            let serve = axum::serve(listener, app).with_graceful_shutdown(async move {
                wait_shutdown_signal().await;
                info!("shutdown signal");
                let _ = shutdown_tx.send(true);
            });

            serve.await?;
            if let Some(h) = sync_handle {
                let _ = h.await;
            }
            // Clear pid file if this process was the daemon
            if let Some(pid) = config::read_daemon_pid() {
                if pid == std::process::id() {
                    config::remove_daemon_pid();
                }
            }
            Ok(())
        }

        Commands::Background {
            bind,
            no_sync,
            no_log,
        } => start_background(bind, no_sync, no_log),

        Commands::Quit => quit_background(),
    }
}

/// Wait for Ctrl+C (SIGINT) or SIGTERM (used by `content-sync quit`).
async fn wait_shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                let _ = sig.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

fn start_background(bind: Option<String>, no_sync: bool, no_log: bool) -> anyhow::Result<()> {
    if let Some(pid) = config::read_daemon_pid() {
        if config::process_alive(pid) {
            anyhow::bail!(
                "already running in background (pid {pid}). Stop it first: content-sync quit"
            );
        }
        // Stale pid file from a previous crash
        config::remove_daemon_pid();
    }

    let exe = std::env::current_exe()
        .map_err(|e| anyhow::anyhow!("cannot resolve current executable: {e}"))?;
    let log_path = config::background_log_path();
    config::ensure_config_dir()?;

    let mut cmd = std::process::Command::new(&exe);
    cmd.arg("serve");
    if let Some(ref b) = bind {
        cmd.arg("--bind").arg(b);
    }
    if no_sync {
        cmd.arg("--no-sync");
    }
    if no_log {
        // Core serve also silences tracing / banner.
        cmd.arg("--no-log");
    }
    cmd.stdin(std::process::Stdio::null());

    if no_log {
        cmd.stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
    } else {
        let log_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .map_err(|e| anyhow::anyhow!("cannot open log {}: {e}", log_path.display()))?;
        let log_err = log_file
            .try_clone()
            .map_err(|e| anyhow::anyhow!("cannot clone log handle: {e}"))?;
        cmd.stdout(std::process::Stdio::from(log_file))
            .stderr(std::process::Stdio::from(log_err));
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // New session so the daemon is not killed when the terminal closes (SIGHUP).
        unsafe {
            cmd.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| anyhow::anyhow!("failed to start background process: {e}"))?;
    let pid = child.id();

    // Give the child a moment to bind the port / crash early (e.g. address in use).
    std::thread::sleep(std::time::Duration::from_millis(400));
    match child.try_wait() {
        Ok(Some(status)) => {
            config::remove_daemon_pid();
            if no_log {
                anyhow::bail!(
                    "background process exited immediately (pid {pid}, {status}). \
                     Re-run without --no-log to capture error output."
                );
            }
            let hint = tail_log_lines(&log_path, 8);
            anyhow::bail!(
                "background process exited immediately (pid {pid}, {status}).\n\
                 Check log: {}\n{hint}",
                log_path.display()
            );
        }
        Ok(None) => {
            // Still running — detach and keep going.
            std::mem::forget(child);
        }
        Err(e) => {
            let _ = child.kill();
            anyhow::bail!("failed to check background process status: {e}");
        }
    }

    config::write_daemon_pid(pid)?;

    let settings = {
        // best-effort bind for user message (may already be overridden in child)
        let db_path = config::config_db_path();
        ConfigDb::open(&db_path)
            .ok()
            .and_then(|db| db.get_settings().ok())
            .map(|s| s.web_bind)
            .unwrap_or_else(|| "127.0.0.1:8787".into())
    };
    let bind_display = bind.unwrap_or(settings);

    println!("Started in background (pid {pid})");
    println!("Web UI  → http://{bind_display}");
    if no_log {
        println!("Log     → disabled (--no-log)");
    } else {
        println!("Log     → {}", log_path.display());
    }
    println!("PID     → {}", config::pid_file_path().display());
    println!("Stop with: content-sync quit");
    Ok(())
}

fn tail_log_lines(path: &std::path::Path, n: usize) -> String {
    let Ok(content) = std::fs::read_to_string(path) else {
        return String::new();
    };
    let lines: Vec<&str> = content.lines().rev().take(n).collect();
    if lines.is_empty() {
        return String::new();
    }
    let mut out = String::from("--- last log lines ---\n");
    for line in lines.into_iter().rev() {
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn quit_background() -> anyhow::Result<()> {
    let Some(pid) = config::read_daemon_pid() else {
        println!("No background process (pid file not found).");
        return Ok(());
    };

    if !config::process_alive(pid) {
        config::remove_daemon_pid();
        println!("Background process not running (cleared stale pid {pid}).");
        return Ok(());
    }

    config::terminate_process(pid)?;

    // Wait briefly for graceful exit, then SIGKILL if needed
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while config::process_alive(pid) && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    if config::process_alive(pid) {
        #[cfg(unix)]
        {
            let rc = unsafe { libc::kill(pid as i32, libc::SIGKILL) };
            if rc != 0 {
                let err = std::io::Error::last_os_error();
                if err.raw_os_error() != Some(libc::ESRCH) {
                    anyhow::bail!("process {pid} did not exit; SIGKILL failed: {err}");
                }
            }
            // brief wait after kill
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        #[cfg(not(unix))]
        {
            anyhow::bail!("process {pid} did not exit after SIGTERM");
        }
    }

    config::remove_daemon_pid();
    if config::process_alive(pid) {
        anyhow::bail!("failed to stop background process (pid {pid})");
    }
    println!("Stopped background process (pid {pid}).");
    Ok(())
}
