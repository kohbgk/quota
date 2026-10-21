//! Claude Code subscription usage.
//!
//! Anthropic doesn't publish an API for Claude subscription limits. Quota offers
//! two opt-in sources:
//!
//! 1. **Usage endpoint** (`GET https://api.anthropic.com/api/oauth/usage`), an
//!    undocumented endpoint that accepts Claude Code's OAuth sign-in. Quota reads
//!    the access token from Claude Code's local credentials file at request time,
//!    sends it only to `api.anthropic.com`, and never stores, logs or refreshes it.
//!    Claude Code rotates its refresh token, so refreshing from a second program
//!    could sign Claude Code out — Quota deliberately leaves renewal to Claude Code.
//! 2. **Status line bridge** — the documented `rate_limits` field Claude Code
//!    passes to status line commands (see `statusline.rs`).

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::{find_on_path, statusline, ProviderError};
use crate::model::{redact, LimitWindow, Usage, UsageSource};
use crate::settings::Settings;

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const OAUTH_BETA: &str = "oauth-2025-04-20";

/// Access token wrapper that never prints its value.
pub struct Secret(String);

impl Secret {
    fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(***)")
    }
}

#[derive(Debug)]
pub struct Credentials {
    pub access_token: Secret,
    /// Unix milliseconds.
    pub expires_at: Option<i64>,
    pub scopes: Option<Vec<String>>,
    pub subscription_type: Option<String>,
}

/// Claude Code's configuration directory (`CLAUDE_CONFIG_DIR` or `~/.claude`).
pub fn config_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    crate::paths::home_dir().map(|h| h.join(".claude"))
}

pub fn credentials_file(dir: &Path) -> PathBuf {
    dir.join(".credentials.json")
}

/// Whether Claude Code appears to be installed on this machine.
pub fn is_installed(dir: &Path) -> bool {
    if credentials_file(dir).exists() || dir.join("settings.json").exists() || dir.join("projects").is_dir() {
        return true;
    }
    if let Some(home) = crate::paths::home_dir() {
        if home.join(".claude.json").exists() {
            return true;
        }
        let native = if cfg!(windows) { "claude.exe" } else { "claude" };
        if home.join(".local").join("bin").join(native).exists() {
            return true;
        }
    }
    find_on_path(if cfg!(windows) { &["claude.exe", "claude.cmd"] } else { &["claude"] }).is_some()
}

pub fn parse_credentials(bytes: &[u8]) -> Option<Credentials> {
    let value: Value = serde_json::from_slice(bytes).ok()?;
    let oauth = value.get("claudeAiOauth")?;
    let token = oauth.get("accessToken")?.as_str()?.trim();
    if token.is_empty() {
        return None;
    }
    Some(Credentials {
        access_token: Secret(token.to_string()),
        expires_at: oauth.get("expiresAt").and_then(Value::as_i64),
        scopes: oauth.get("scopes").and_then(Value::as_array).map(|a| {
            a.iter().filter_map(Value::as_str).map(str::to_string).collect()
        }),
        subscription_type: oauth.get("subscriptionType").and_then(Value::as_str).map(str::to_string),
    })
}

/// Validates the saved sign-in before any network request is made.
pub fn check_credentials(creds: &Credentials, now_ms: i64) -> Result<(), ProviderError> {
    if let Some(expires_at) = creds.expires_at {
        // Treat tokens expiring within a minute as expired to avoid a pointless 401.
        if expires_at <= now_ms + 60_000 {
            return Err(ProviderError::Expired);
        }
    }
    if let Some(scopes) = &creds.scopes {
        if !scopes.iter().any(|s| s == "user:profile") {
            return Err(ProviderError::Unsupported(
                "This Claude Code sign-in can't read usage. Run /login in Claude Code, then try again.".into(),
            ));
        }
    }
    Ok(())
}

/// Produces Claude usage according to the user's opt-ins.
pub async fn fetch(settings: &Settings) -> Result<Usage, ProviderError> {
    let Some(dir) = config_dir() else { return Err(ProviderError::NotInstalled) };
    let bridge = settings
        .claude_status_line
        .then(|| statusline::read_snapshot(&crate::paths::statusline_snapshot_file()))
        .flatten()
        .and_then(|s| s.to_usage());

    if settings.claude_usage_endpoint {
        let endpoint = fetch_from_endpoint(&dir).await;
        return match (endpoint, bridge) {
            (Ok(usage), _) => Ok(usage),
            // The endpoint failed, but Claude Code itself reported usage recently.
            (Err(err), Some(bridge)) if !matches!(err, ProviderError::NotInstalled) => {
                log::info!("usage endpoint unavailable ({err:?}); using status line data");
                Ok(bridge)
            }
            (Err(err), _) => Err(err),
        };
    }

    if !is_installed(&dir) {
        return Err(ProviderError::NotInstalled);
    }
    if settings.claude_status_line {
        return bridge.ok_or(ProviderError::Waiting);
    }
    if credentials_file(&dir).exists() {
        Err(ProviderError::NeedsConsent)
    } else {
        Err(ProviderError::SignedOut("Open Claude Code and sign in with your Claude account.".into()))
    }
}

async fn fetch_from_endpoint(dir: &Path) -> Result<Usage, ProviderError> {
    let path = credentials_file(dir);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(if is_installed(dir) {
                ProviderError::SignedOut("Open Claude Code and sign in with your Claude account.".into())
            } else {
                ProviderError::NotInstalled
            });
        }
        Err(_) => return Err(ProviderError::Other("Couldn't read Claude Code's sign-in file.".into())),
    };
    let creds = parse_credentials(&bytes).ok_or_else(|| {
        ProviderError::SignedOut("Claude Code isn't signed in with a Claude subscription.".into())
    })?;
    drop(bytes);
    check_credentials(&creds, crate::paths::now_ms())?;

    tauri::async_runtime::spawn_blocking(move || request_usage(&creds))
        .await
        .map_err(|_| ProviderError::Other("Usage request was interrupted.".into()))?
}

fn agent() -> ureq::Agent {
    use ureq::tls::{RootCerts, TlsConfig};
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .http_status_as_error(false)
        .user_agent(concat!("Quota/", env!("CARGO_PKG_VERSION"), " (+https://github.com/kohbgk/quota)"))
        .tls_config(TlsConfig::builder().root_certs(RootCerts::PlatformVerifier).build())
        .build()
        .into()
}

fn request_usage(creds: &Credentials) -> Result<Usage, ProviderError> {
    let response = agent()
        .get(USAGE_URL)
        .header("Authorization", &format!("Bearer {}", creds.access_token.expose()))
        .header("anthropic-beta", OAUTH_BETA)
        .header("Accept", "application/json")
        .call()
        .map_err(|err| {
            log::warn!("claude usage request failed: {}", redact(&err.to_string()));
            ProviderError::Network("Can't reach Anthropic. Check your connection.".into())
        })?;

    let status = response.status().as_u16();
    let retry_after = response
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<i64>().ok());
    let mut body = response.into_body();
    let text = body.with_config().limit(256 * 1024).read_to_string().unwrap_or_default();

    match status {
        200 => {
            let value: Value = serde_json::from_str(&text)
                .map_err(|_| ProviderError::Other("Anthropic returned an unexpected usage format.".into()))?;
            parse_usage_response(&value, creds.subscription_type.clone(), crate::paths::now_ms())
        }
        401 => Err(ProviderError::Expired),
        403 if text.contains("scope") => Err(ProviderError::Unsupported(
            "This Claude Code sign-in can't read usage. Run /login in Claude Code, then try again.".into(),
        )),
        429 => Err(ProviderError::RateLimited { retry_after_secs: retry_after }),
        _ => Err(ProviderError::Other(format!("Anthropic returned HTTP {status}."))),
    }
}

fn parse_window(value: Option<&Value>) -> Option<LimitWindow> {
    let window = value?.as_object()?;
    let utilization = window.get("utilization").and_then(Value::as_f64)?;
    let resets_at = window
        .get("resets_at")
        .and_then(Value::as_str)
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.timestamp());
    Some(LimitWindow::new(utilization, resets_at, None))
}

/// Maps the usage endpoint response. `utilization` is a 0–100 percentage of the
/// window consumed and `resets_at` an RFC 3339 timestamp.
pub fn parse_usage_response(value: &Value, plan: Option<String>, now_ms: i64) -> Result<Usage, ProviderError> {
    if !value.is_object() {
        return Err(ProviderError::Other("Anthropic returned an unexpected usage format.".into()));
    }
    let five_hour = parse_window(value.get("five_hour")).map(|mut w| {
        w.window_minutes = Some(300);
        w
    });
    let weekly = parse_window(value.get("seven_day")).map(|mut w| {
        w.window_minutes = Some(10_080);
        w
    });
    Ok(Usage { five_hour, weekly, plan, source: UsageSource::ClaudeUsageEndpoint, fetched_at: now_ms })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_usage_response() {
        let value = json!({
            "five_hour": { "utilization": 62.0, "resets_at": "2026-10-21T07:59:59.943648+00:00" },
            "seven_day": { "utilization": 19.4, "resets_at": "2026-10-25T03:00:00Z" },
            "seven_day_opus": null,
            "extra_usage": { "is_enabled": false }
        });
        let usage = parse_usage_response(&value, Some("pro".into()), 7).unwrap();
        let five = usage.five_hour.unwrap();
        assert_eq!(five.used_percent, 62.0);
        assert_eq!(five.resets_at, Some(1_792_569_599));
        assert_eq!(five.window_minutes, Some(300));
        assert_eq!(usage.weekly.unwrap().resets_at, Some(1_792_897_200));
        assert_eq!(usage.plan.as_deref(), Some("pro"));
        assert_eq!(usage.source, UsageSource::ClaudeUsageEndpoint);
    }

    #[test]
    fn missing_or_null_windows_are_not_invented() {
        let usage = parse_usage_response(&json!({ "five_hour": null, "seven_day": { "utilization": null } }), None, 0).unwrap();
        assert!(usage.five_hour.is_none());
        assert!(usage.weekly.is_none());
        let idle = parse_usage_response(&json!({ "five_hour": { "utilization": 0.0, "resets_at": null } }), None, 0).unwrap();
        let five = idle.five_hour.unwrap();
        assert_eq!(five.used_percent, 0.0);
        assert_eq!(five.resets_at, None);
    }

    #[test]
    fn parses_credentials_without_exposing_token() {
        let creds = parse_credentials(
            br#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-secret","refreshToken":"r","expiresAt":1800000000000,"scopes":["user:inference","user:profile"],"subscriptionType":"pro"}}"#,
        )
        .unwrap();
        assert_eq!(creds.expires_at, Some(1_800_000_000_000));
        assert_eq!(creds.subscription_type.as_deref(), Some("pro"));
        let debug = format!("{creds:?}");
        assert!(!debug.contains("secret"), "{debug}");
        assert!(parse_credentials(b"{}").is_none());
        assert!(parse_credentials(br#"{"claudeAiOauth":{"accessToken":""}}"#).is_none());
    }

    #[test]
    fn rejects_expired_or_underscoped_tokens_before_any_request() {
        let make = |expires_at: Option<i64>, scopes: Option<Vec<&str>>| Credentials {
            access_token: Secret("t".into()),
            expires_at,
            scopes: scopes.map(|s| s.into_iter().map(String::from).collect()),
            subscription_type: None,
        };
        let now = 1_000_000;
        assert_eq!(check_credentials(&make(Some(now - 1), None), now), Err(ProviderError::Expired));
        assert_eq!(check_credentials(&make(Some(now + 30_000), None), now), Err(ProviderError::Expired));
        assert!(check_credentials(&make(Some(now + 3_600_000), Some(vec!["user:profile"])), now).is_ok());
        assert!(matches!(
            check_credentials(&make(None, Some(vec!["user:inference"])), now),
            Err(ProviderError::Unsupported(_))
        ));
        assert!(check_credentials(&make(None, None), now).is_ok());
    }
}

#[cfg(test)]
mod live {
    use super::*;

    /// Network check: a bogus token must be rejected as an expired sign-in,
    /// proving TLS, headers and status mapping work end to end.
    /// Run with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn endpoint_rejects_bogus_token() {
        let creds = Credentials {
            access_token: Secret("sk-ant-oat01-not-a-real-token".into()),
            expires_at: None,
            scopes: None,
            subscription_type: None,
        };
        let result = request_usage(&creds);
        assert!(matches!(result, Err(ProviderError::Expired)), "{result:?}");
    }
}
