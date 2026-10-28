//! Tells the user when a newer Quota release is out. Quota doesn't update itself:
//! it links to the release page, and the user installs the new version.
//!
//! Asks GitHub's public "latest release" API for this repository (no account, no
//! credentials). Drafts and pre-releases are never "latest", so they're never offered.

use serde::Serialize;
use serde_json::Value;
use std::time::Duration;

const LATEST_URL: &str = "https://api.github.com/repos/kohbgk/quota/releases/latest";
/// Only this repository's release pages are ever opened.
const RELEASES_PAGE: &str = "https://github.com/kohbgk/quota/releases";
/// How often to look for a new version while Quota runs.
pub const CHECK_EVERY_MS: i64 = 6 * 3600 * 1000;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailableUpdate {
    /// "0.3.0"
    pub version: String,
    /// The release page to download it from.
    pub url: String,
}

/// What the UI shows about updates.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateState {
    pub available: Option<AvailableUpdate>,
    /// Unix ms of the last completed check.
    pub checked_at: Option<i64>,
    pub checking: bool,
    /// Why the last check failed, if it did.
    pub error: Option<String>,
    /// Unix ms when the next automatic check is due.
    #[serde(skip)]
    pub next_check_at: i64,
}

/// Asks GitHub for the latest release. `Ok(None)` means this version is current.
pub async fn check() -> Result<Option<AvailableUpdate>, String> {
    tauri::async_runtime::spawn_blocking(fetch_latest)
        .await
        .map_err(|_| "The update check was interrupted.".to_string())?
        .map(|release| newer_release(&release, env!("CARGO_PKG_VERSION")))
}

fn fetch_latest() -> Result<Value, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .http_status_as_error(false)
        .user_agent(concat!("Quota/", env!("CARGO_PKG_VERSION"), " (+https://github.com/kohbgk/quota)"))
        .tls_config(ureq::tls::TlsConfig::builder().root_certs(ureq::tls::RootCerts::PlatformVerifier).build())
        .build()
        .into();
    let response = agent
        .get(LATEST_URL)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|_| "Couldn't reach GitHub to check for updates.".to_string())?;
    match response.status().as_u16() {
        200 => {}
        404 => return Ok(Value::Null), // No published release yet.
        403 | 429 => return Err("GitHub is limiting update checks. Quota will try again later.".into()),
        status => return Err(format!("GitHub returned HTTP {status} for the update check.")),
    }
    let text = response.into_body().with_config().limit(1024 * 1024).read_to_string().unwrap_or_default();
    serde_json::from_str(&text).map_err(|_| "GitHub returned an unexpected answer.".to_string())
}

/// Opens the release page of the update found by the last check, or the latest
/// release page when none is known. Only this repository's pages are opened.
pub fn open_release_page(app: &tauri::AppHandle) {
    use tauri_plugin_opener::OpenerExt;
    let core = crate::shared_core(app);
    let url = core
        .update_state()
        .available
        .map(|u| u.url)
        .filter(|u| is_release_page(u))
        .unwrap_or_else(|| format!("{RELEASES_PAGE}/latest"));
    if let Err(err) = app.opener().open_url(url, None::<&str>) {
        log::warn!("couldn't open the release page: {err}");
    }
}

/// One Windows notification per new version.
pub fn notify(app: &tauri::AppHandle, update: &AvailableUpdate) {
    use tauri_plugin_notification::NotificationExt;
    let shown = app
        .notification()
        .builder()
        .title(format!("Quota {} is available", update.version))
        .body("Open Quota and click Download to get the latest version.")
        .show();
    if let Err(err) = shown {
        log::warn!("couldn't show the update notification: {err}");
    }
}

/// The release if it's newer than `current`. Anything unexpected counts as "no update".
pub fn newer_release(release: &Value, current: &str) -> Option<AvailableUpdate> {
    if release.get("draft").and_then(Value::as_bool) == Some(true)
        || release.get("prerelease").and_then(Value::as_bool) == Some(true)
    {
        return None;
    }
    let tag = release.get("tag_name").and_then(Value::as_str)?;
    let latest = parse_version(tag)?;
    if latest <= parse_version(current)? {
        return None;
    }
    let version = format!("{}.{}.{}", latest.0, latest.1, latest.2);
    let page = release.get("html_url").and_then(Value::as_str).filter(|u| is_release_page(u));
    Some(AvailableUpdate { url: page.map_or_else(|| format!("{RELEASES_PAGE}/latest"), str::to_string), version })
}

/// "v0.3.0" or "0.3.0" → (0, 3, 0); anything else is ignored.
fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let mut parts = text.trim().trim_start_matches('v').split('.');
    let version = (parts.next()?.parse().ok()?, parts.next()?.parse().ok()?, parts.next()?.parse().ok()?);
    parts.next().is_none().then_some(version)
}

/// Whether `url` is one of this repository's release pages (the only links Quota opens).
pub fn is_release_page(url: &str) -> bool {
    url.strip_prefix(RELEASES_PAGE).is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
        && !url.contains("..")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn release(tag: &str) -> Value {
        json!({ "tag_name": tag, "html_url": format!("https://github.com/kohbgk/quota/releases/tag/{tag}"),
                "draft": false, "prerelease": false })
    }

    #[test]
    fn offers_only_newer_versions() {
        let update = newer_release(&release("v0.3.0"), "0.2.0").unwrap();
        assert_eq!(update.version, "0.3.0");
        assert_eq!(update.url, "https://github.com/kohbgk/quota/releases/tag/v0.3.0");
        assert!(newer_release(&release("v0.10.0"), "0.9.9").is_some(), "numeric, not text, comparison");
        assert!(newer_release(&release("v0.2.0"), "0.2.0").is_none());
        assert!(newer_release(&release("v0.1.4"), "0.2.0").is_none());
    }

    #[test]
    fn ignores_drafts_prereleases_and_odd_tags() {
        let mut pre = release("v0.3.0");
        pre["prerelease"] = json!(true);
        assert!(newer_release(&pre, "0.2.0").is_none());
        assert!(newer_release(&release("v0.3.0-beta.1"), "0.2.0").is_none());
        assert!(newer_release(&release("nightly"), "0.2.0").is_none());
        assert!(newer_release(&Value::Null, "0.2.0").is_none());
    }

    #[test]
    fn only_opens_this_repositorys_release_pages() {
        assert!(is_release_page("https://github.com/kohbgk/quota/releases/tag/v0.3.0"));
        assert!(is_release_page("https://github.com/kohbgk/quota/releases"));
        assert!(!is_release_page("https://github.com/kohbgk/quota/releases-evil/x"));
        assert!(!is_release_page("https://github.com/someone/else/releases/tag/v9"));
        assert!(!is_release_page("https://github.com/kohbgk/quota/releases/../../other"));
        let mut odd = release("v0.3.0");
        odd["html_url"] = json!("https://example.com/download");
        assert_eq!(newer_release(&odd, "0.2.0").unwrap().url, "https://github.com/kohbgk/quota/releases/latest");
    }

    /// Live check against GitHub. Run with `cargo test -- --ignored live_update`.
    #[test]
    #[ignore]
    fn live_update_check() {
        let release = fetch_latest().expect("GitHub answers");
        eprintln!("latest: {:?}", release.get("tag_name"));
        assert!(newer_release(&release, "0.0.1").is_some(), "an old version sees an update");
        assert!(newer_release(&release, "999.0.0").is_none());
    }
}
