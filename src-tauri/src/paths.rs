//! Where Quota keeps its own files.
//!
//! These paths are computed without Tauri so that the lightweight
//! `--quota-statusline` bridge process (which never starts Tauri) and the main
//! app always agree on locations.

use std::path::{Path, PathBuf};

pub const APP_IDENTIFIER: &str = "com.kohbgk.quota";

/// `%APPDATA%\com.kohbgk.quota` on Windows (roaming config + small state).
pub fn data_dir() -> PathBuf {
    dirs::config_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join(APP_IDENTIFIER)
}

pub fn settings_file() -> PathBuf {
    data_dir().join("settings.json")
}

/// Last successfully retrieved usage. Never contains credentials.
pub fn cache_file() -> PathBuf {
    data_dir().join("usage-cache.json")
}

/// Written by the Claude Code status line bridge.
pub fn statusline_snapshot_file() -> PathBuf {
    data_dir().join("claude-statusline.json")
}

pub fn home_dir() -> Option<PathBuf> {
    dirs::home_dir()
}

/// Writes `bytes` to `path` atomically (write to a sibling temp file, then rename),
/// creating parent directories as needed.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    // Unique per write, so concurrent writers never share a temp file.
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = path.with_file_name(format!(".{file_name}.{}.{n}.tmp", std::process::id()));
    std::fs::write(&tmp, bytes)?;
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(err) => {
            let _ = std::fs::remove_file(&tmp);
            Err(err)
        }
    }
}

/// Current time as Unix milliseconds.
pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_creates_parents_and_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a").join("b.json");
        write_atomic(&path, b"one").unwrap();
        write_atomic(&path, b"two").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "two");
        let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty());
    }
}
