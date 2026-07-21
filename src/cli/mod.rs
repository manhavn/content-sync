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

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Initialize config directory and default settings
    Init {
        /// Watch directory for synced files (raw content)
        #[arg(long)]
        watch_dir: Option<String>,
    },

    /// Start Web UI + file watcher + sync engine
    Serve {
        /// Bind address (overrides settings)
        #[arg(long, short)]
        bind: Option<String>,
        /// Disable background file watcher / poll (API only)
        #[arg(long)]
        no_sync: bool,
    },

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

        Commands::Serve { bind, no_sync } => {
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
            info!("Web UI listening on http://{addr}");
            println!("Content Sync Web UI → http://{addr}");
            println!("Config            → {}", config::config_dir().display());
            println!("Press Ctrl+C to stop.");

            let serve = axum::serve(listener, app).with_graceful_shutdown(async move {
                let _ = tokio::signal::ctrl_c().await;
                info!("shutdown signal");
                let _ = shutdown_tx.send(true);
            });

            serve.await?;
            if let Some(h) = sync_handle {
                let _ = h.await;
            }
            Ok(())
        }
    }
}
