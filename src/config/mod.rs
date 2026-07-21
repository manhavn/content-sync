mod db;

pub use db::*;

use std::path::PathBuf;

/// Config directory: `~/.content-sync`
pub fn config_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".content-sync")
}

/// Local SQLite config path: `~/.content-sync/config.sqlite`
pub fn config_db_path() -> PathBuf {
    config_dir().join("config.sqlite")
}

pub fn ensure_config_dir() -> anyhow::Result<PathBuf> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir)?;
    let tokens = dir.join("tokens");
    std::fs::create_dir_all(&tokens)?;
    Ok(dir)
}
