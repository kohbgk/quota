//! User preferences, persisted as JSON in the app data directory.

use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::paths;

pub const DEFAULT_SHORTCUT: &str = "Ctrl+Shift+Space";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

/// How often usage is refreshed in the background.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum RefreshInterval {
    TwoMinutes,
    #[default]
    FiveMinutes,
    FifteenMinutes,
    /// No background polling; refresh only when the overlay opens.
    OnOpen,
}

impl RefreshInterval {
    /// Background interval in seconds, or `None` when polling is off.
    pub fn seconds(self) -> Option<i64> {
        match self {
            RefreshInterval::TwoMinutes => Some(120),
            RefreshInterval::FiveMinutes => Some(300),
            RefreshInterval::FifteenMinutes => Some(900),
            RefreshInterval::OnOpen => None,
        }
    }
}

/// Where the user dragged the overlay: the monitor it was dropped on, and the spot as
/// fractions of that monitor's work area, so it survives resolution and scaling changes
/// (and maps onto another monitor if that one is gone). `x` spans the free width
/// (0 = left edge, 1 = right edge); `y` is the top edge's offset as a fraction of the height.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OverlayPosition {
    pub x: f64,
    pub y: f64,
    #[serde(default)]
    pub monitor: Option<String>,
}

impl OverlayPosition {
    /// Centered horizontally, a fifth of the way down, on the monitor with the cursor.
    pub const DEFAULT: OverlayPosition = OverlayPosition { x: 0.5, y: 0.2, monitor: None };

    /// `None` for values that can't place a window (corrupt settings file).
    pub fn sanitized(self) -> Option<Self> {
        (self.x.is_finite() && self.y.is_finite())
            .then(|| OverlayPosition { x: self.x.clamp(0.0, 1.0), y: self.y.clamp(0.0, 1.0), monitor: self.monitor })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub show_five_hour: bool,
    pub show_weekly: bool,
    pub show_codex: bool,
    pub show_claude: bool,
    pub show_antigravity: bool,
    pub shortcut: String,
    pub hide_on_blur: bool,
    pub theme: Theme,
    pub refresh_interval: RefreshInterval,
    /// Explicit opt-in to read Claude usage through Anthropic's undocumented
    /// OAuth usage endpoint with Claude Code's local sign-in.
    pub claude_usage_endpoint: bool,
    /// Whether Quota installed its Claude Code status line bridge.
    pub claude_status_line: bool,
    /// Explicit opt-in to read Antigravity quota from the running Antigravity
    /// app's undocumented local language server.
    pub antigravity_app: bool,
    /// Optional explicit path to the `codex` executable.
    pub codex_path: Option<String>,
    /// Set after the first launch so first-run behavior only happens once.
    pub onboarded: bool,
    /// Where the user moved the overlay; `None` uses the default spot.
    pub overlay_position: Option<OverlayPosition>,
    /// Look for new Quota releases on GitHub and say when one is out.
    pub check_for_updates: bool,
    /// The newest version a notification was already shown for.
    pub notified_update: Option<String>,
    /// The version whose "update available" bar the user closed.
    pub dismissed_update: Option<String>,
    /// Format of this settings file, for one-time adjustments on upgrade.
    /// Files written before versions existed read as 0.
    #[serde(default)]
    pub settings_version: u32,
}

/// Bumped when an upgrade needs to adjust saved settings (see `migrate`).
pub const SETTINGS_VERSION: u32 = 1;

impl Default for Settings {
    fn default() -> Self {
        Self {
            show_five_hour: true,
            show_weekly: true,
            show_codex: true,
            show_claude: true,
            // Antigravity is optional: hidden until the user turns it on.
            show_antigravity: false,
            shortcut: DEFAULT_SHORTCUT.to_string(),
            hide_on_blur: true,
            theme: Theme::System,
            refresh_interval: RefreshInterval::FiveMinutes,
            claude_usage_endpoint: false,
            claude_status_line: false,
            antigravity_app: false,
            codex_path: None,
            onboarded: false,
            overlay_position: None,
            check_for_updates: true,
            notified_update: None,
            dismissed_update: None,
            settings_version: SETTINGS_VERSION,
        }
    }
}

impl Settings {
    fn shows_a_provider(&self) -> bool {
        self.show_codex || self.show_claude || self.show_antigravity
    }

    /// Rejects combinations that would leave the overlay empty.
    pub fn validate(&self) -> Result<(), String> {
        if !self.show_five_hour && !self.show_weekly {
            return Err("Keep at least one limit type visible.".into());
        }
        if !self.shows_a_provider() {
            return Err("Keep at least one provider visible.".into());
        }
        if self.shortcut.trim().is_empty() {
            return Err("The shortcut can't be empty.".into());
        }
        Ok(())
    }

    /// One-time adjustments for settings saved by older versions.
    fn migrate(mut self) -> Self {
        if self.settings_version < 1 {
            // 0.2.0–0.2.1 showed Antigravity by default. Hide it unless the user
            // connected it (or it's the only provider they chose to show).
            if !self.antigravity_app && (self.show_codex || self.show_claude) {
                self.show_antigravity = false;
            }
        }
        self.settings_version = SETTINGS_VERSION;
        self
    }

    /// Repairs invalid values loaded from disk instead of failing to start.
    fn sanitize(mut self) -> Self {
        if !self.show_five_hour && !self.show_weekly {
            self.show_five_hour = true;
            self.show_weekly = true;
        }
        if !self.shows_a_provider() {
            self.show_codex = true;
            self.show_claude = true;
            self.show_antigravity = true;
        }
        if self.shortcut.trim().is_empty() {
            self.shortcut = DEFAULT_SHORTCUT.to_string();
        }
        if let Some(path) = &self.codex_path {
            if path.trim().is_empty() {
                self.codex_path = None;
            }
        }
        self.overlay_position = self.overlay_position.and_then(OverlayPosition::sanitized);
        self
    }

    pub fn load() -> Self {
        Self::load_from(&paths::settings_file())
    }

    pub fn load_from(path: &Path) -> Self {
        match std::fs::read(path) {
            Ok(bytes) => match serde_json::from_slice::<Settings>(&bytes) {
                Ok(settings) => settings.migrate().sanitize(),
                Err(err) => {
                    log::warn!("settings file is invalid, using defaults: {err}");
                    // Keep the broken file for inspection instead of silently losing it.
                    let _ = std::fs::rename(path, path.with_extension("json.invalid"));
                    Settings::default()
                }
            },
            Err(_) => Settings::default(),
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(&paths::settings_file())
    }

    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        let json = serde_json::to_vec_pretty(self).expect("settings serialize");
        paths::write_atomic(path, &json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_show_everything() {
        let s = Settings::default();
        assert!(s.show_five_hour && s.show_weekly && s.show_codex && s.show_claude);
        assert_eq!(s.shortcut, "Ctrl+Shift+Space");
        assert!(s.hide_on_blur);
        assert!(!s.claude_usage_endpoint, "undocumented integration must be opt-in");
        assert!(!s.antigravity_app, "undocumented integration must be opt-in");
        assert!(s.validate().is_ok());
    }

    #[test]
    fn validation_requires_a_limit_and_a_provider() {
        let s = Settings { show_five_hour: false, show_weekly: false, ..Default::default() };
        assert!(s.validate().is_err());
        let s = Settings { show_codex: false, show_claude: false, show_antigravity: false, ..Default::default() };
        assert!(s.validate().is_err());
        let s = Settings { show_codex: false, show_claude: false, show_antigravity: true, ..Default::default() };
        assert!(s.validate().is_ok(), "Antigravity alone is enough");
        let s = Settings { show_weekly: false, show_codex: false, ..Default::default() };
        assert!(s.validate().is_ok());
    }

    #[test]
    fn round_trips_and_tolerates_missing_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let s = Settings {
            show_weekly: false,
            shortcut: "Ctrl+Alt+Q".into(),
            refresh_interval: RefreshInterval::OnOpen,
            theme: Theme::Dark,
            ..Default::default()
        };
        s.save_to(&path).unwrap();
        assert_eq!(Settings::load_from(&path), s);

        std::fs::write(&path, r#"{"showWeekly": false}"#).unwrap();
        let loaded = Settings::load_from(&path);
        assert!(!loaded.show_weekly);
        assert!(loaded.show_five_hour);
        assert_eq!(loaded.shortcut, DEFAULT_SHORTCUT);
    }

    #[test]
    fn antigravity_starts_hidden_and_older_files_hide_it_once() {
        assert!(!Settings::default().show_antigravity && !Settings::default().antigravity_app);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let load = |json: &str| {
            std::fs::write(&path, json).unwrap();
            Settings::load_from(&path)
        };
        // Saved by 0.2.x with Antigravity shown but never connected: hidden.
        let old = load(r#"{"showCodex": true, "showClaude": true, "showAntigravity": true, "antigravityApp": false}"#);
        assert!(!old.show_antigravity);
        assert_eq!(old.settings_version, SETTINGS_VERSION);
        // Connected, or the only provider shown: kept.
        assert!(load(r#"{"showAntigravity": true, "antigravityApp": true}"#).show_antigravity);
        assert!(load(r#"{"showCodex": false, "showClaude": false, "showAntigravity": true}"#).show_antigravity);
        // Once migrated, the user's own choice to show it stays.
        assert!(load(&format!(r#"{{"showAntigravity": true, "settingsVersion": {SETTINGS_VERSION}}}"#)).show_antigravity);
    }

    #[test]
    fn remembers_the_overlay_position() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let position = OverlayPosition { x: 0.25, y: 0.6, monitor: Some(r"\\.\DISPLAY2".into()) };
        let s = Settings { overlay_position: Some(position.clone()), ..Default::default() };
        s.save_to(&path).unwrap();
        assert_eq!(Settings::load_from(&path).overlay_position, Some(position));

        // Files from older versions have no position; values without a monitor still load.
        std::fs::write(&path, r#"{"overlayPosition": {"x": 0.5, "y": 0.1}}"#).unwrap();
        let loaded = Settings::load_from(&path).overlay_position.unwrap();
        assert_eq!((loaded.x, loaded.y, loaded.monitor), (0.5, 0.1, None));

        // Out-of-range values are pulled back on screen.
        std::fs::write(&path, r#"{"overlayPosition": {"x": 7.0, "y": -2.0}}"#).unwrap();
        let loaded = Settings::load_from(&path).overlay_position.unwrap();
        assert_eq!((loaded.x, loaded.y), (1.0, 0.0));
    }

    #[test]
    fn repairs_invalid_combinations_and_bad_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"showFiveHour": false, "showWeekly": false, "shortcut": " "}"#)
            .unwrap();
        let loaded = Settings::load_from(&path);
        assert!(loaded.show_five_hour && loaded.show_weekly);
        assert_eq!(loaded.shortcut, DEFAULT_SHORTCUT);

        std::fs::write(&path, "{not json").unwrap();
        assert_eq!(Settings::load_from(&path), Settings::default());
        assert!(path.with_extension("json.invalid").exists());
    }
}
