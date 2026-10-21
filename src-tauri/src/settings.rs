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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub show_five_hour: bool,
    pub show_weekly: bool,
    pub show_codex: bool,
    pub show_claude: bool,
    pub shortcut: String,
    pub hide_on_blur: bool,
    pub theme: Theme,
    pub refresh_interval: RefreshInterval,
    /// Explicit opt-in to read Claude usage through Anthropic's undocumented
    /// OAuth usage endpoint with Claude Code's local sign-in.
    pub claude_usage_endpoint: bool,
    /// Whether Quota installed its Claude Code status line bridge.
    pub claude_status_line: bool,
    /// Optional explicit path to the `codex` executable.
    pub codex_path: Option<String>,
    /// Set after the first launch so first-run behavior only happens once.
    pub onboarded: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            show_five_hour: true,
            show_weekly: true,
            show_codex: true,
            show_claude: true,
            shortcut: DEFAULT_SHORTCUT.to_string(),
            hide_on_blur: true,
            theme: Theme::System,
            refresh_interval: RefreshInterval::FiveMinutes,
            claude_usage_endpoint: false,
            claude_status_line: false,
            codex_path: None,
            onboarded: false,
        }
    }
}

impl Settings {
    /// Rejects combinations that would leave the overlay empty.
    pub fn validate(&self) -> Result<(), String> {
        if !self.show_five_hour && !self.show_weekly {
            return Err("Keep at least one limit type visible.".into());
        }
        if !self.show_codex && !self.show_claude {
            return Err("Keep at least one provider visible.".into());
        }
        if self.shortcut.trim().is_empty() {
            return Err("The shortcut can't be empty.".into());
        }
        Ok(())
    }

    /// Repairs invalid values loaded from disk instead of failing to start.
    fn sanitize(mut self) -> Self {
        if !self.show_five_hour && !self.show_weekly {
            self.show_five_hour = true;
            self.show_weekly = true;
        }
        if !self.show_codex && !self.show_claude {
            self.show_codex = true;
            self.show_claude = true;
        }
        if self.shortcut.trim().is_empty() {
            self.shortcut = DEFAULT_SHORTCUT.to_string();
        }
        if let Some(path) = &self.codex_path {
            if path.trim().is_empty() {
                self.codex_path = None;
            }
        }
        self
    }

    pub fn load() -> Self {
        Self::load_from(&paths::settings_file())
    }

    pub fn load_from(path: &Path) -> Self {
        match std::fs::read(path) {
            Ok(bytes) => match serde_json::from_slice::<Settings>(&bytes) {
                Ok(settings) => settings.sanitize(),
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
        assert!(s.validate().is_ok());
    }

    #[test]
    fn validation_requires_a_limit_and_a_provider() {
        let s = Settings { show_five_hour: false, show_weekly: false, ..Default::default() };
        assert!(s.validate().is_err());
        let s = Settings { show_codex: false, show_claude: false, ..Default::default() };
        assert!(s.validate().is_err());
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
