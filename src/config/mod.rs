mod db;

pub use db::*;

use std::path::PathBuf;

/// Config directory: `~/.content-sync`
pub fn config_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".content-sync")
}

/// Local config DB path: `~/.content-sync/config-sqlite`
///
/// Uses a `-sqlite` suffix (not `.sqlite`) so object stores / scanners that
/// block the `.sqlite` extension (e.g. some GCS policies) still accept the file.
pub fn config_db_path() -> PathBuf {
    config_dir().join("config-sqlite")
}

/// Rename legacy `config.sqlite` (+ WAL/SHM/journal sidecars) to `config-sqlite`
/// when the new path does not exist yet. Best-effort; ignores errors.
pub fn migrate_legacy_config_db() {
    let dir = config_dir();
    let new_main = dir.join("config-sqlite");
    let old_main = dir.join("config.sqlite");
    if new_main.exists() || !old_main.exists() {
        return;
    }
    let pairs = [
        ("config.sqlite", "config-sqlite"),
        ("config.sqlite-wal", "config-sqlite-wal"),
        ("config.sqlite-shm", "config-sqlite-shm"),
        ("config.sqlite-journal", "config-sqlite-journal"),
    ];
    for (from, to) in pairs {
        let src = dir.join(from);
        let dst = dir.join(to);
        if src.exists() && !dst.exists() {
            let _ = std::fs::rename(&src, &dst);
        }
    }
}

/// PID file for the background daemon: `~/.content-sync/content-sync.pid`
pub fn pid_file_path() -> PathBuf {
    config_dir().join("content-sync.pid")
}

/// Log file for the background daemon: `~/.content-sync/content-sync.log`
pub fn background_log_path() -> PathBuf {
    config_dir().join("content-sync.log")
}

pub fn ensure_config_dir() -> anyhow::Result<PathBuf> {
    let dir = config_dir();
    // Only the config root — no child dirs (legacy `tokens/`, empty `files/` roots are unused).
    // Auth tokens live in config-sqlite; per-connection watch dirs are created on demand.
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Read PID from the daemon pid file, if present and parseable.
pub fn read_daemon_pid() -> Option<u32> {
    let s = std::fs::read_to_string(pid_file_path()).ok()?;
    s.trim().parse().ok()
}

pub fn write_daemon_pid(pid: u32) -> anyhow::Result<()> {
    ensure_config_dir()?;
    std::fs::write(pid_file_path(), format!("{pid}\n"))?;
    Ok(())
}

pub fn remove_daemon_pid() {
    let _ = std::fs::remove_file(pid_file_path());
}

/// Whether a process with this PID is still alive (Linux `/proc`, else kill -0).
pub fn process_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(target_os = "linux")]
    {
        return std::path::Path::new(&format!("/proc/{pid}")).exists();
    }
    #[cfg(all(unix, not(target_os = "linux")))]
    {
        // kill(pid, 0) == 0 means process exists (or EPERM: exists but not ours)
        let rc = unsafe { libc::kill(pid as i32, 0) };
        return rc == 0
            || (rc == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM));
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

/// Send SIGTERM to a process. Returns Ok if the signal was delivered or the process is already gone.
pub fn terminate_process(pid: u32) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        let rc = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
        if rc == 0 {
            return Ok(());
        }
        let err = std::io::Error::last_os_error();
        // ESRCH: no such process — treat as already stopped
        if err.raw_os_error() == Some(libc::ESRCH) {
            return Ok(());
        }
        anyhow::bail!("failed to signal pid {pid}: {err}");
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        anyhow::bail!("background/quit is only supported on Unix");
    }
}
