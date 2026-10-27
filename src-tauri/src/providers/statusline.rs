//! Claude Code status line bridge.
//!
//! Claude Code runs a configurable status line command and passes it session
//! data as JSON on stdin. For Claude subscribers that JSON includes a documented
//! `rate_limits` object (`five_hour` / `seven_day`, each with `used_percentage`
//! and `resets_at` in Unix seconds). When the user enables the bridge, Quota
//! registers itself as the status line command (`quota.exe --quota-statusline`),
//! records `rate_limits` to a small file, and prints a compact status line.
//!
//! The bridge never replaces a status line the user already configured.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::model::{remaining_label, reset_passed, LimitWindow, Usage, UsageSource};
use crate::paths;

pub const BRIDGE_FLAG: &str = "--quota-statusline";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SnapshotWindow {
    pub used_percentage: f64,
    pub resets_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    /// Unix milliseconds when Claude Code last reported limits.
    pub captured_at: i64,
    pub five_hour: Option<SnapshotWindow>,
    pub seven_day: Option<SnapshotWindow>,
}

impl Snapshot {
    pub fn to_usage(&self) -> Option<Usage> {
        if self.five_hour.is_none() && self.seven_day.is_none() {
            return None;
        }
        let convert = |w: &SnapshotWindow, minutes| LimitWindow::new(w.used_percentage, w.resets_at, Some(minutes));
        Some(Usage {
            five_hour: self.five_hour.as_ref().map(|w| convert(w, 300)),
            weekly: self.seven_day.as_ref().map(|w| convert(w, 10_080)),
            plan: None,
            source: UsageSource::ClaudeStatusLine,
            fetched_at: self.captured_at,
            blocked: false,
            groups: Vec::new(),
        })
    }
}

pub fn read_snapshot(path: &Path) -> Option<Snapshot> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

fn parse_window(value: Option<&Value>) -> Option<SnapshotWindow> {
    let value = value?;
    Some(SnapshotWindow {
        used_percentage: value.get("used_percentage")?.as_f64()?,
        resets_at: value.get("resets_at").and_then(Value::as_i64),
    })
}

/// Extracts `rate_limits` from status line input JSON.
pub fn snapshot_from_input(input: &Value, now_ms: i64) -> Option<Snapshot> {
    let limits = input.get("rate_limits")?;
    let snapshot = Snapshot {
        captured_at: now_ms,
        five_hour: parse_window(limits.get("five_hour")),
        seven_day: parse_window(limits.get("seven_day")),
    };
    (snapshot.five_hour.is_some() || snapshot.seven_day.is_some()).then_some(snapshot)
}

impl SnapshotWindow {
    fn has_reset(&self, now_ms: i64) -> bool {
        reset_passed(self.resets_at, now_ms)
    }
}

/// Claude Code drops a window from `rate_limits` once its `resets_at` passes.
/// Keep the previously recorded window in that case, so Quota can say it reset
/// instead of claiming the window isn't reported.
fn carry_reset_windows(mut snapshot: Snapshot, previous: Option<Snapshot>, now_ms: i64) -> Snapshot {
    let Some(previous) = previous else { return snapshot };
    let carry = |new: &mut Option<SnapshotWindow>, old: Option<SnapshotWindow>| {
        if new.is_none() {
            *new = old.filter(|w| w.has_reset(now_ms));
        }
    };
    carry(&mut snapshot.five_hour, previous.five_hour);
    carry(&mut snapshot.seven_day, previous.seven_day);
    snapshot
}

/// The single line printed back to Claude Code.
pub fn render_line(input: &Value, snapshot: Option<&Snapshot>, now_ms: i64) -> String {
    let model = input
        .get("model")
        .and_then(|m| m.get("display_name"))
        .and_then(Value::as_str)
        .unwrap_or("Claude");
    let mut parts = vec![model.to_string()];
    if let Some(s) = snapshot {
        for (label, window) in [("5h", &s.five_hour), ("week", &s.seven_day)] {
            match window {
                Some(w) if w.has_reset(now_ms) => parts.push(format!("{label} reset")),
                Some(w) => parts.push(format!("{label} {} left", remaining_label(w.used_percentage))),
                None => {}
            }
        }
    }
    parts.join(" · ")
}

/// Entry point for `quota --quota-statusline`. Must be fast and never fail loudly:
/// Claude Code shows whatever is printed.
pub fn run_bridge(stdin: &mut impl Read, stdout: &mut impl Write, snapshot_path: &Path, now_ms: i64) {
    let mut raw = Vec::new();
    let _ = stdin.take(1024 * 1024).read_to_end(&mut raw);
    let input: Value = serde_json::from_slice(&raw).unwrap_or(Value::Null);
    let snapshot = snapshot_from_input(&input, now_ms)
        .map(|s| carry_reset_windows(s, read_snapshot(snapshot_path), now_ms));
    if let Some(snapshot) = &snapshot {
        if let Ok(json) = serde_json::to_vec(snapshot) {
            let _ = paths::write_atomic(snapshot_path, &json);
        }
    }
    let _ = writeln!(stdout, "{}", render_line(&input, snapshot.as_ref(), now_ms));
    let _ = stdout.flush();
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum BridgeState {
    /// No status line configured.
    Available,
    /// Quota's bridge is configured.
    Installed,
    /// The user has their own status line; Quota won't touch it.
    Occupied,
    /// Claude Code isn't set up on this machine.
    Unavailable,
}

fn settings_path(claude_dir: &Path) -> PathBuf {
    claude_dir.join("settings.json")
}

fn read_settings(path: &Path) -> Result<Map<String, Value>, String> {
    match std::fs::read(path) {
        Ok(bytes) if bytes.iter().all(u8::is_ascii_whitespace) => Ok(Map::new()),
        Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
            Ok(Value::Object(map)) => Ok(map),
            _ => Err("Claude Code's settings.json couldn't be read, so Quota left it unchanged.".into()),
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
        Err(_) => Err("Couldn't open Claude Code's settings.json.".into()),
    }
}

fn is_ours(map: &Map<String, Value>) -> bool {
    map.get("statusLine")
        .and_then(|s| s.get("command"))
        .and_then(Value::as_str)
        .is_some_and(|c| c.contains(BRIDGE_FLAG))
}

pub fn state(claude_dir: &Path) -> BridgeState {
    if !claude_dir.exists() {
        return BridgeState::Unavailable;
    }
    match read_settings(&settings_path(claude_dir)) {
        Ok(map) if is_ours(&map) => BridgeState::Installed,
        Ok(map) if map.get("statusLine").is_some_and(|v| !v.is_null()) => BridgeState::Occupied,
        Ok(_) => BridgeState::Available,
        Err(_) => BridgeState::Occupied,
    }
}

/// Builds a shell-friendly command for `exe`. Forward slashes and, on Windows,
/// the space-free 8.3 path make it work from Git Bash, cmd and PowerShell alike.
pub fn bridge_command(exe: &Path) -> String {
    let path = short_path(exe).unwrap_or_else(|| exe.to_path_buf());
    let mut text = path.to_string_lossy().replace('\\', "/");
    if text.contains(' ') {
        text = format!("\"{text}\"");
    }
    format!("{text} {BRIDGE_FLAG}")
}

#[cfg(windows)]
fn short_path(path: &Path) -> Option<PathBuf> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use windows_sys::Win32::Storage::FileSystem::GetShortPathNameW;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut buf = vec![0u16; 1024];
    let len = unsafe { GetShortPathNameW(wide.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) } as usize;
    if len == 0 || len >= buf.len() {
        return None;
    }
    Some(PathBuf::from(std::ffi::OsString::from_wide(&buf[..len])))
}

#[cfg(not(windows))]
fn short_path(_path: &Path) -> Option<PathBuf> {
    None
}

pub fn install(claude_dir: &Path, exe: &Path) -> Result<(), String> {
    if !claude_dir.exists() {
        return Err("Claude Code isn't set up on this PC yet.".into());
    }
    let path = settings_path(claude_dir);
    let mut map = read_settings(&path)?;
    if !is_ours(&map) && map.get("statusLine").is_some_and(|v| !v.is_null()) {
        return Err("You already have a custom Claude Code status line. Quota won't replace it.".into());
    }
    let mut status_line = Map::new();
    status_line.insert("type".into(), Value::from("command"));
    status_line.insert("command".into(), Value::from(bridge_command(exe)));
    status_line.insert("padding".into(), Value::from(0));
    map.insert("statusLine".into(), Value::Object(status_line));
    write_settings(&path, &map)
}

/// Points Quota's status line entry at `exe` if it names another path (the portable
/// exe was moved, or an update installed elsewhere). Returns whether anything changed.
pub fn repair(claude_dir: &Path, exe: &Path) -> Result<bool, String> {
    let path = settings_path(claude_dir);
    let mut map = read_settings(&path)?;
    if !is_ours(&map) {
        return Ok(false);
    }
    let command = bridge_command(exe);
    let entry = map.get_mut("statusLine").and_then(Value::as_object_mut).expect("checked by is_ours");
    if entry.get("command").and_then(Value::as_str) == Some(command.as_str()) {
        return Ok(false);
    }
    entry.insert("command".into(), Value::from(command));
    write_settings(&path, &map)?;
    Ok(true)
}

pub fn uninstall(claude_dir: &Path) -> Result<(), String> {
    let path = settings_path(claude_dir);
    if !path.exists() {
        return Ok(());
    }
    let mut map = read_settings(&path)?;
    if is_ours(&map) {
        map.remove("statusLine");
        write_settings(&path, &map)?;
    }
    Ok(())
}

fn write_settings(path: &Path, map: &Map<String, Value>) -> Result<(), String> {
    let mut json = serde_json::to_vec_pretty(map).map_err(|e| e.to_string())?;
    json.push(b'\n');
    paths::write_atomic(path, &json).map_err(|_| "Couldn't update Claude Code's settings.json.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn input() -> Value {
        json!({
            "model": { "id": "claude-x", "display_name": "Opus" },
            "rate_limits": {
                "five_hour": { "used_percentage": 23.5, "resets_at": 1_738_425_600 },
                "seven_day": { "used_percentage": 41.2, "resets_at": 1_738_857_600 }
            }
        })
    }

    #[test]
    fn repair_follows_a_moved_exe_and_leaves_other_lines_alone() {
        let dir = tempfile::tempdir().unwrap();
        let claude = dir.path();
        install(claude, Path::new("/old/place/quota")).unwrap();
        assert!(repair(claude, Path::new("/new/place/quota")).unwrap());
        let saved: Value = serde_json::from_slice(&std::fs::read(claude.join("settings.json")).unwrap()).unwrap();
        assert_eq!(saved["statusLine"]["command"], "/new/place/quota --quota-statusline");
        assert_eq!(saved["statusLine"]["padding"], 0);
        assert!(!repair(claude, Path::new("/new/place/quota")).unwrap());

        let custom = r#"{"statusLine":{"type":"command","command":"my-line.sh"}}"#;
        std::fs::write(claude.join("settings.json"), custom).unwrap();
        assert!(!repair(claude, Path::new("/new/place/quota")).unwrap());
        assert_eq!(std::fs::read_to_string(claude.join("settings.json")).unwrap(), custom);
    }

    #[test]
    fn bridge_records_rate_limits_and_prints_a_line() {
        let dir = tempfile::tempdir().unwrap();
        let snapshot_path = dir.path().join("snap.json");
        let mut stdin = serde_json::to_vec(&input()).unwrap();
        let mut out = Vec::new();
        run_bridge(&mut stdin.as_slice(), &mut out, &snapshot_path, 99);
        stdin.clear();
        assert_eq!(String::from_utf8(out).unwrap(), "Opus · 5h 76% left · week 58% left\n");
        let snap = read_snapshot(&snapshot_path).unwrap();
        assert_eq!(snap.captured_at, 99);
        let usage = snap.to_usage().unwrap();
        assert_eq!(usage.five_hour.unwrap().used_percent, 23.5);
        assert_eq!(usage.weekly.unwrap().resets_at, Some(1_738_857_600));
        assert_eq!(usage.source, UsageSource::ClaudeStatusLine);
        assert_eq!(usage.fetched_at, 99);
    }

    #[test]
    fn bridge_remembers_windows_claude_code_dropped_after_reset() {
        let dir = tempfile::tempdir().unwrap();
        let snapshot_path = dir.path().join("snap.json");
        run_bridge(&mut serde_json::to_vec(&input()).unwrap().as_slice(), &mut Vec::new(), &snapshot_path, 1);

        // After the 5-hour reset, Claude Code sends only the weekly window.
        let after_reset = 1_738_425_600_000 + 60_000;
        let weekly_only = json!({
            "model": { "display_name": "Opus" },
            "rate_limits": { "seven_day": { "used_percentage": 42.0, "resets_at": 1_738_857_600 } }
        });
        let mut out = Vec::new();
        run_bridge(&mut serde_json::to_vec(&weekly_only).unwrap().as_slice(), &mut out, &snapshot_path, after_reset);
        assert_eq!(String::from_utf8(out).unwrap(), "Opus · 5h reset · week 58% left\n");
        let snap = read_snapshot(&snapshot_path).unwrap();
        assert_eq!(snap.five_hour.unwrap().resets_at, Some(1_738_425_600));
        assert_eq!(snap.seven_day.unwrap().used_percentage, 42.0);

        // A window that disappears before its reset time is not carried over.
        run_bridge(&mut serde_json::to_vec(&input()).unwrap().as_slice(), &mut Vec::new(), &snapshot_path, 1);
        run_bridge(&mut serde_json::to_vec(&weekly_only).unwrap().as_slice(), &mut Vec::new(), &snapshot_path, 2);
        assert_eq!(read_snapshot(&snapshot_path).unwrap().five_hour, None);
    }

    #[test]
    fn bridge_keeps_previous_snapshot_when_limits_are_absent() {
        let dir = tempfile::tempdir().unwrap();
        let snapshot_path = dir.path().join("snap.json");
        run_bridge(&mut serde_json::to_vec(&input()).unwrap().as_slice(), &mut Vec::new(), &snapshot_path, 1);
        let mut out = Vec::new();
        run_bridge(&mut br#"{"model":{"display_name":"Sonnet"}}"#.as_slice(), &mut out, &snapshot_path, 2);
        assert_eq!(String::from_utf8(out).unwrap(), "Sonnet\n");
        assert_eq!(read_snapshot(&snapshot_path).unwrap().captured_at, 1);
        let mut out = Vec::new();
        run_bridge(&mut b"garbage".as_slice(), &mut out, &snapshot_path, 3);
        assert_eq!(String::from_utf8(out).unwrap(), "Claude\n");
    }

    #[test]
    fn installs_and_uninstalls_without_touching_other_settings() {
        let dir = tempfile::tempdir().unwrap();
        let claude = dir.path();
        std::fs::write(claude.join("settings.json"), r#"{"model":"opus","permissions":{"allow":["Bash"]}}"#).unwrap();
        assert_eq!(state(claude), BridgeState::Available);
        let exe = Path::new("/opt/My Apps/quota");
        install(claude, exe).unwrap();
        assert_eq!(state(claude), BridgeState::Installed);
        let saved: Value = serde_json::from_slice(&std::fs::read(claude.join("settings.json")).unwrap()).unwrap();
        assert_eq!(saved["model"], "opus");
        assert_eq!(saved["permissions"]["allow"][0], "Bash");
        assert_eq!(saved["statusLine"]["command"], "\"/opt/My Apps/quota\" --quota-statusline");
        // Key order is preserved: the user's keys stay first.
        let keys: Vec<_> = saved.as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys, ["model", "permissions", "statusLine"]);
        install(claude, exe).unwrap(); // idempotent
        uninstall(claude).unwrap();
        assert_eq!(state(claude), BridgeState::Available);
        let saved: Value = serde_json::from_slice(&std::fs::read(claude.join("settings.json")).unwrap()).unwrap();
        assert!(saved.get("statusLine").is_none());
        assert_eq!(saved["model"], "opus");
    }

    #[test]
    fn never_replaces_a_custom_status_line_or_broken_settings() {
        let dir = tempfile::tempdir().unwrap();
        let claude = dir.path();
        let custom = r#"{"statusLine":{"type":"command","command":"~/bin/my-line.sh"}}"#;
        std::fs::write(claude.join("settings.json"), custom).unwrap();
        assert_eq!(state(claude), BridgeState::Occupied);
        assert!(install(claude, Path::new("/x/quota")).is_err());
        uninstall(claude).unwrap();
        assert_eq!(std::fs::read_to_string(claude.join("settings.json")).unwrap(), custom);

        std::fs::write(claude.join("settings.json"), "{ // comments are not JSON").unwrap();
        assert!(install(claude, Path::new("/x/quota")).is_err());
        assert_eq!(std::fs::read_to_string(claude.join("settings.json")).unwrap(), "{ // comments are not JSON");
    }

    #[test]
    fn creates_settings_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        install(dir.path(), Path::new("/x/quota")).unwrap();
        assert_eq!(state(dir.path()), BridgeState::Installed);
        assert!(bridge_command(Path::new("/x/quota")).ends_with("/x/quota --quota-statusline"));
    }
}
