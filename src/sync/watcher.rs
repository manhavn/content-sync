use notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{new_debouncer, DebouncedEvent, Debouncer};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;
use tracing::debug;

use crate::models::validate_file_name;

/// Events we care about for watched files
#[derive(Debug, Clone)]
pub enum TokenFileEvent {
    Changed(PathBuf),
    Removed(PathBuf),
}

struct DirWatch {
    dir: PathBuf,
    _debouncer: Debouncer<RecommendedWatcher>,
    rx: mpsc::Receiver<Result<Vec<DebouncedEvent>, notify::Error>>,
}

/// Watches multiple directories at once (one per connection).
pub struct MultiWatcher {
    dirs: Vec<DirWatch>,
}

impl MultiWatcher {
    pub fn new(watch_dirs: &[PathBuf]) -> anyhow::Result<Self> {
        let mut unique: HashSet<PathBuf> = HashSet::new();
        let mut dirs = Vec::new();
        for d in watch_dirs {
            std::fs::create_dir_all(d)?;
            let abs = d.canonicalize().unwrap_or_else(|_| d.clone());
            if !unique.insert(abs.clone()) {
                continue; // already watching
            }
            let (tx, rx) = mpsc::channel();
            let mut debouncer = new_debouncer(Duration::from_millis(600), tx)
                .map_err(|e| anyhow::anyhow!("create file watcher: {e}"))?;
            notify::Watcher::watch(debouncer.watcher(), &abs, RecursiveMode::NonRecursive)
                .map_err(|e| anyhow::anyhow!("watch {}: {e}", abs.display()))?;
            dirs.push(DirWatch {
                dir: abs,
                _debouncer: debouncer,
                rx,
            });
        }
        Ok(Self { dirs })
    }

    /// Non-blocking poll; returns deduped events across all dirs
    pub fn try_recv(&self) -> Vec<TokenFileEvent> {
        let mut changed: HashSet<PathBuf> = HashSet::new();
        let mut removed: HashSet<PathBuf> = HashSet::new();

        for dw in &self.dirs {
            while let Ok(msg) = dw.rx.try_recv() {
                match msg {
                    Ok(events) => {
                        for ev in events {
                            match map_event(&ev, &dw.dir) {
                                Some(TokenFileEvent::Changed(p)) => {
                                    removed.remove(&p);
                                    changed.insert(p);
                                }
                                Some(TokenFileEvent::Removed(p)) => {
                                    changed.remove(&p);
                                    removed.insert(p);
                                }
                                None => {}
                            }
                        }
                    }
                    Err(e) => tracing::warn!("watcher error on {}: {e}", dw.dir.display()),
                }
            }
        }

        let mut out = Vec::with_capacity(changed.len() + removed.len());
        out.extend(changed.into_iter().map(TokenFileEvent::Changed));
        out.extend(removed.into_iter().map(TokenFileEvent::Removed));
        out
    }
}

fn is_watched_name(name: &str) -> bool {
    validate_file_name(name).is_ok()
}

fn is_same_dir(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => false,
    }
}

fn map_event(ev: &DebouncedEvent, watch_dir: &Path) -> Option<TokenFileEvent> {
    let path = &ev.path;
    if is_same_dir(path, watch_dir) {
        debug!("ignore watch-dir event: {}", path.display());
        return None;
    }
    if path.is_dir() {
        debug!("ignore directory event: {}", path.display());
        return None;
    }
    let name = path.file_name()?.to_str()?;
    if !is_watched_name(name) {
        debug!("ignore non-watched name: {name}");
        return None;
    }
    if path.is_file() {
        Some(TokenFileEvent::Changed(path.clone()))
    } else if !path.exists() {
        Some(TokenFileEvent::Removed(path.clone()))
    } else {
        None
    }
}

/// Scan directory for all watched files (non-hidden regular files)
pub fn scan_files(dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if !dir.exists() {
        return Ok(files);
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if is_watched_name(name) {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

/// Resolve absolute form of a path if possible
pub fn abs_path(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

/// True if `path` is a direct child of `dir` (works even if path was deleted)
pub fn is_under_dir(path: &Path, dir: &Path) -> bool {
    let dir = abs_path(dir);
    let parent = path
        .parent()
        .map(|p| p.canonicalize().unwrap_or_else(|_| p.to_path_buf()));
    parent.map(|p| p == dir).unwrap_or(false)
}

/// Map absolute file path → connection ids that watch its parent dir
pub fn connection_ids_for_path(path: &Path, conn_dirs: &HashMap<String, PathBuf>) -> Vec<String> {
    conn_dirs
        .iter()
        .filter(|(_, dir)| is_under_dir(path, dir))
        .map(|(id, _)| id.clone())
        .collect()
}
