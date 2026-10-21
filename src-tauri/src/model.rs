//! Provider-neutral usage types shared by the providers, the refresh engine and the UI.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderId {
    Codex,
    Claude,
}

impl ProviderId {
    pub const ALL: [ProviderId; 2] = [ProviderId::Codex, ProviderId::Claude];

    pub fn display_name(self) -> &'static str {
        match self {
            ProviderId::Codex => "Codex",
            ProviderId::Claude => "Claude Code",
        }
    }
}

/// One rate-limit window as reported by the provider.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LimitWindow {
    /// Percentage of the window already consumed, clamped to 0–100.
    pub used_percent: f64,
    /// Unix seconds when the window resets. `None` when the provider reports
    /// no active window (for example, no usage yet in the current period).
    pub resets_at: Option<i64>,
    /// Window length in minutes, when reported.
    pub window_minutes: Option<i64>,
}

impl LimitWindow {
    pub fn new(used_percent: f64, resets_at: Option<i64>, window_minutes: Option<i64>) -> Self {
        let used_percent = if used_percent.is_finite() { used_percent.clamp(0.0, 100.0) } else { 0.0 };
        Self { used_percent, resets_at, window_minutes }
    }

    /// The window's reset time has passed, so its usage numbers are out of date.
    pub fn has_reset(&self, now_ms: i64) -> bool {
        self.resets_at.is_some_and(|at| at.saturating_mul(1000) <= now_ms)
    }
}

/// Where a usage snapshot came from. Shown to the user so the data is never ambiguous.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UsageSource {
    /// Official `codex app-server` `account/rateLimits/read`.
    CodexAppServer,
    /// Anthropic's undocumented OAuth usage endpoint (opt-in).
    ClaudeUsageEndpoint,
    /// `rate_limits` from Claude Code's documented status line input.
    ClaudeStatusLine,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub five_hour: Option<LimitWindow>,
    pub weekly: Option<LimitWindow>,
    /// Subscription plan label as reported (for example "plus" or "pro").
    pub plan: Option<String>,
    pub source: UsageSource,
    /// Unix milliseconds when the provider produced (or Quota retrieved) this data.
    pub fetched_at: i64,
    /// The provider says included usage is currently not allowed for this account,
    /// whatever the percentages show (Codex `ordinaryUsageAllowed: false`).
    #[serde(default)]
    pub blocked: bool,
}

/// The provider's connection state, independent of whether cached data exists.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ProviderStatus {
    /// Nothing attempted yet.
    Unknown,
    /// Last attempt succeeded.
    Ready,
    NotInstalled,
    SignedOut { message: String },
    /// Claude Code is signed in, but the user hasn't allowed Quota to read usage yet.
    NeedsConsent,
    /// Claude Code's saved sign-in has expired; Claude Code renews it on next use.
    Expired,
    /// Only the status line bridge is enabled and it hasn't reported yet.
    Waiting,
    /// Installed and signed in, but this account or sign-in type can't report limits.
    Unsupported { message: String },
    /// The provider is rate limiting Quota; it will retry automatically.
    RateLimited { retry_at: Option<i64> },
    /// Network unreachable or the provider returned an unexpected error.
    Error { message: String },
}

impl ProviderStatus {
    /// Whether the status means Quota is waiting on the user rather than on a retry.
    pub fn needs_user(&self) -> bool {
        matches!(
            self,
            ProviderStatus::NotInstalled
                | ProviderStatus::SignedOut { .. }
                | ProviderStatus::NeedsConsent
                | ProviderStatus::Expired
                | ProviderStatus::Unsupported { .. }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderState {
    pub id: ProviderId,
    pub status: ProviderStatus,
    /// Last successfully retrieved usage (possibly from the on-disk cache).
    pub usage: Option<Usage>,
    pub refreshing: bool,
    /// Unix milliseconds of the most recent attempt, successful or not.
    pub last_attempt_at: Option<i64>,
}

impl ProviderState {
    pub fn new(id: ProviderId) -> Self {
        Self { id, status: ProviderStatus::Unknown, usage: None, refreshing: false, last_attempt_at: None }
    }
}

/// Classifies a window by its length so provider-specific ordering never matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowKind {
    FiveHour,
    Weekly,
    Other,
}

pub fn classify_window_minutes(minutes: i64) -> WindowKind {
    match minutes {
        // Allow a little slack in case a provider reports e.g. 299 or 301.
        280..=320 => WindowKind::FiveHour,
        9_900..=10_200 => WindowKind::Weekly,
        _ => WindowKind::Other,
    }
}

/// Percentage left, rounded down so Quota never overstates what remains.
/// Matches `remainingPercent` in the UI.
pub fn remaining_percent(used_percent: f64) -> i64 {
    let used = if used_percent.is_finite() { used_percent.clamp(0.0, 100.0) } else { 0.0 };
    (100.0 - used + 1e-9).floor().clamp(0.0, 100.0) as i64
}

/// Removes anything that looks like a secret from text before it is shown or logged.
pub fn redact(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for word in text.split_inclusive(|c: char| c.is_whitespace() || c == '"' || c == '\'') {
        let trimmed = word.trim_end_matches(|c: char| c.is_whitespace() || c == '"' || c == '\'');
        let suffix = &word[trimmed.len()..];
        let looks_secret = trimmed.len() >= 24
            && trimmed.chars().all(|c| c.is_ascii_alphanumeric() || "-_.=+/".contains(c))
            && trimmed.chars().any(|c| c.is_ascii_digit());
        let has_prefix = trimmed.starts_with("sk-") || trimmed.starts_with("eyJ") || trimmed.starts_with("Bearer");
        if looks_secret || (has_prefix && trimmed.len() > 8) {
            out.push_str("[redacted]");
        } else {
            out.push_str(trimmed);
        }
        out.push_str(suffix);
    }
    let mut out = out.trim().to_string();
    if out.chars().count() > 240 {
        out = out.chars().take(240).collect::<String>() + "…";
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_percent() {
        assert_eq!(LimitWindow::new(140.0, None, None).used_percent, 100.0);
        assert_eq!(LimitWindow::new(-3.0, None, None).used_percent, 0.0);
        assert_eq!(LimitWindow::new(f64::NAN, None, None).used_percent, 0.0);
    }

    #[test]
    fn remaining_rounds_down() {
        assert_eq!(remaining_percent(23.5), 76);
        assert_eq!(remaining_percent(0.4), 99);
        assert_eq!(remaining_percent(28.0), 72);
        assert_eq!(remaining_percent(100.0), 0);
        assert_eq!(remaining_percent(-5.0), 100);
    }

    #[test]
    fn classifies_windows() {
        assert_eq!(classify_window_minutes(300), WindowKind::FiveHour);
        assert_eq!(classify_window_minutes(10_080), WindowKind::Weekly);
        assert_eq!(classify_window_minutes(1_440), WindowKind::Other);
    }

    #[test]
    fn redacts_tokens() {
        let text = r#"failed: {"token":"sk-ant-oat01-abcdefghijklmnopqrstuvwxyz0123"} Bearer eyJhbGciOiJIUzI1NiJ9.abc"#;
        let out = redact(text);
        assert!(!out.contains("abcdefghijkl"), "{out}");
        assert!(!out.contains("eyJhbGci"), "{out}");
        assert!(out.contains("failed"));
        assert_eq!(redact("connection refused"), "connection refused");
    }

    #[test]
    fn status_serializes_with_kind_tag() {
        let json = serde_json::to_value(ProviderStatus::SignedOut { message: "x".into() }).unwrap();
        assert_eq!(json["kind"], "signedOut");
        assert_eq!(json["message"], "x");
    }
}
