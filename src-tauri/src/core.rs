//! Application state and the refresh engine.
//!
//! The engine decides *when* to ask providers for usage (on open, on a timer,
//! after a window resets, or on demand), applies per-provider backoff, keeps
//! the last good data cached on disk, and pushes every change to the UI.

use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

use crate::model::{ProviderId, ProviderState, ProviderStatus, Usage};
use crate::paths::{self, now_ms};
use crate::providers::{claude, codex, statusline, ProviderError};
use crate::settings::Settings;

pub const STATE_EVENT: &str = "quota://state";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Startup,
    Open,
    Timer,
    Manual,
    WindowReset,
}

#[derive(Debug, Clone)]
pub struct Tracked {
    pub state: ProviderState,
    pub failures: u32,
    /// Unix ms before which automatic refreshes are held back.
    pub next_allowed_at: i64,
}

impl Tracked {
    fn new(id: ProviderId) -> Self {
        Self { state: ProviderState::new(id), failures: 0, next_allowed_at: 0 }
    }
}

/// Everything the UI needs to render.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSnapshot {
    pub settings: Settings,
    pub providers: Vec<ProviderState>,
    pub shortcut_error: Option<String>,
    pub launch_at_startup: bool,
    pub version: &'static str,
    pub now: i64,
    /// True during the very first session, so the overlay can show a short hint.
    pub first_run: bool,
}

pub struct Core {
    pub app: AppHandle,
    settings: Mutex<Settings>,
    providers: Mutex<HashMap<ProviderId, Tracked>>,
    pub shortcut_error: Mutex<Option<String>>,
    pub overlay: Mutex<OverlayState>,
    pub launch_at_startup: AtomicBool,
    first_run: bool,
}

#[derive(Debug, Default)]
pub struct OverlayState {
    pub shown_at: i64,
    pub hidden_at: i64,
    /// Logical height requested by the UI.
    pub height: f64,
    pub ready: bool,
    pub show_when_ready: bool,
    /// The user dragged the overlay since it was last placed; save where it ended up.
    pub dragged: bool,
    /// Physical top-left corner Quota last placed the overlay at.
    pub placed_at: Option<(i32, i32)>,
    /// When the user last started dragging the overlay.
    pub drag_started_at: i64,
    /// When the overlay last moved, and whether a save is waiting for moves to settle.
    pub moved_at: i64,
    pub save_pending: bool,
    /// A settings window is being created on another thread.
    pub settings_opening: bool,
}

/// Minimum seconds between two network attempts for the same provider.
fn min_gap_secs(id: ProviderId, local_only: bool, reason: Reason) -> i64 {
    if local_only {
        return 2;
    }
    match (id, reason) {
        (_, Reason::Startup) => 0,
        (ProviderId::Codex, Reason::Manual) => 5,
        (ProviderId::Claude, Reason::Manual) => 30,
        (ProviderId::Codex, Reason::Open) => 20,
        (ProviderId::Claude, Reason::Open) => 60,
        (_, Reason::WindowReset) => 60,
        // Timer reasons are gated by the interval itself; Claude keeps a floor
        // because Anthropic rate limits this endpoint aggressively.
        (ProviderId::Codex, Reason::Timer) => 60,
        (ProviderId::Claude, Reason::Timer) => 120,
    }
}

/// How old data may be before opening the overlay triggers a refresh.
fn open_max_age_secs(id: ProviderId) -> i64 {
    match id {
        ProviderId::Codex => 60,
        ProviderId::Claude => 120,
    }
}

/// Pure refresh policy, kept separate for testing.
pub fn should_refresh(
    id: ProviderId,
    tracked: &Tracked,
    reason: Reason,
    now: i64,
    interval_secs: Option<i64>,
    local_only: bool,
) -> bool {
    let state = &tracked.state;
    if state.refreshing {
        return false;
    }
    let since_attempt = state.last_attempt_at.map(|t| (now - t) / 1000).unwrap_or(i64::MAX);
    if since_attempt < min_gap_secs(id, local_only, reason) {
        return false;
    }
    let held_back = !local_only && now < tracked.next_allowed_at;
    let rate_limited = matches!(state.status, ProviderStatus::RateLimited { .. });
    match reason {
        // Users may retry errors manually, but a provider's rate limit is always respected.
        Reason::Manual => !(held_back && rate_limited),
        Reason::Startup => !held_back,
        Reason::Open => {
            if held_back {
                return false;
            }
            let age = state.usage.as_ref().map(|u| (now - u.fetched_at) / 1000).unwrap_or(i64::MAX);
            age >= open_max_age_secs(id) || !matches!(state.status, ProviderStatus::Ready)
        }
        Reason::Timer => match interval_secs {
            Some(interval) => !held_back && since_attempt >= interval,
            None => false,
        },
        Reason::WindowReset => !held_back,
    }
}

/// Delay before the next automatic attempt after a failure.
pub fn backoff_ms(id: ProviderId, err: &ProviderError, failures: u32) -> i64 {
    let (base, cap) = match id {
        ProviderId::Codex => (30_000i64, 15 * 60_000i64),
        ProviderId::Claude => (120_000, 30 * 60_000),
    };
    let exponential = base.saturating_mul(1i64 << failures.saturating_sub(1).min(10)).min(cap);
    match err {
        ProviderError::RateLimited { retry_after_secs: Some(secs) } => ((*secs).max(60) * 1000).min(60 * 60_000),
        ProviderError::RateLimited { retry_after_secs: None } => exponential.max(5 * 60_000).min(cap),
        // Waiting on the user (sign in, install, consent): re-check cheaply and regularly.
        ProviderError::NotInstalled
        | ProviderError::SignedOut(_)
        | ProviderError::NeedsConsent
        | ProviderError::Expired
        | ProviderError::Waiting
        | ProviderError::Unsupported(_) => 60_000,
        ProviderError::Network(_) | ProviderError::Other(_) => exponential,
    }
}

/// True when a window's reset time has passed since the data was fetched.
pub fn has_reset_since_fetch(usage: &Usage, now: i64) -> bool {
    [&usage.five_hour, &usage.weekly].into_iter().flatten().any(|w| {
        w.resets_at.is_some_and(|r| {
            let reset_ms = r * 1000;
            reset_ms + 30_000 <= now && usage.fetched_at < reset_ms
        })
    })
}

impl Core {
    pub fn new(app: AppHandle, settings: Settings, first_run: bool) -> Arc<Self> {
        let mut providers: HashMap<_, _> = ProviderId::ALL.iter().map(|id| (*id, Tracked::new(*id))).collect();
        for (id, usage) in load_cache() {
            if let Some(t) = providers.get_mut(&id) {
                t.state.usage = Some(usage);
            }
        }
        // Don't show cached Claude data that came from a source the user has since turned off.
        if let Some(t) = providers.get_mut(&ProviderId::Claude) {
            let allowed = match t.state.usage.as_ref().map(|u| u.source) {
                Some(crate::model::UsageSource::ClaudeUsageEndpoint) => settings.claude_usage_endpoint,
                Some(crate::model::UsageSource::ClaudeStatusLine) => settings.claude_status_line,
                _ => true,
            };
            if !allowed {
                t.state.usage = None;
            }
        }
        Arc::new(Self {
            app,
            settings: Mutex::new(settings),
            providers: Mutex::new(providers),
            shortcut_error: Mutex::new(None),
            overlay: Mutex::new(OverlayState { height: 320.0, ..Default::default() }),
            launch_at_startup: AtomicBool::new(false),
            first_run,
        })
    }

    pub fn settings(&self) -> Settings {
        self.settings.lock().unwrap().clone()
    }

    /// Reads, changes and saves the settings as one step under the settings lock,
    /// so changes made at the same time (a drag being saved while a preference
    /// changes) can't overwrite each other or interleave their file writes.
    pub fn modify_settings(&self, change: impl FnOnce(&mut Settings)) -> std::io::Result<Settings> {
        let mut current = self.settings.lock().unwrap();
        let mut next = current.clone();
        change(&mut next);
        next.save()?;
        *current = next.clone();
        Ok(next)
    }

    pub fn snapshot(&self) -> AppSnapshot {
        let providers = self.providers.lock().unwrap();
        AppSnapshot {
            settings: self.settings(),
            providers: ProviderId::ALL.iter().filter_map(|id| providers.get(id)).map(|t| t.state.clone()).collect(),
            shortcut_error: self.shortcut_error.lock().unwrap().clone(),
            launch_at_startup: self.launch_at_startup.load(Ordering::Relaxed),
            version: env!("CARGO_PKG_VERSION"),
            now: now_ms(),
            first_run: self.first_run,
        }
    }

    pub fn emit_state(&self) {
        let snapshot = self.snapshot();
        crate::tray::update_tooltip(&self.app, &snapshot);
        let _ = self.app.emit(STATE_EVENT, snapshot);
    }

    fn is_visible(&self, id: ProviderId) -> bool {
        let s = self.settings.lock().unwrap();
        match id {
            ProviderId::Codex => s.show_codex,
            ProviderId::Claude => s.show_claude,
        }
    }

    /// Claude without the usage endpoint only reads local files — no network.
    fn is_local_only(&self, id: ProviderId) -> bool {
        id == ProviderId::Claude && !self.settings.lock().unwrap().claude_usage_endpoint
    }

    /// Forgets a provider's data and backoff (used when its source changes).
    pub fn reset_provider(&self, id: ProviderId) {
        let mut providers = self.providers.lock().unwrap();
        let refreshing = providers.get(&id).is_some_and(|t| t.state.refreshing);
        let mut fresh = Tracked::new(id);
        fresh.state.refreshing = refreshing;
        providers.insert(id, fresh);
        drop(providers);
        self.save_cache();
    }

    pub fn refresh_all(self: &Arc<Self>, reason: Reason) {
        for id in ProviderId::ALL {
            self.clone().refresh(id, reason);
        }
    }

    /// Starts a refresh in the background if the policy allows it.
    pub fn refresh(self: Arc<Self>, id: ProviderId, reason: Reason) {
        if !self.is_visible(id) {
            return;
        }
        let settings = self.settings();
        let local_only = self.is_local_only(id);
        {
            let mut providers = self.providers.lock().unwrap();
            let Some(tracked) = providers.get_mut(&id) else { return };
            let now = now_ms();
            if !should_refresh(id, tracked, reason, now, settings.refresh_interval.seconds(), local_only) {
                return;
            }
            tracked.state.refreshing = true;
            tracked.state.last_attempt_at = Some(now);
        }
        self.emit_state();

        tauri::async_runtime::spawn(async move {
            let result = match id {
                ProviderId::Codex => codex::fetch(settings.codex_path.clone()).await,
                ProviderId::Claude => claude::fetch(&settings).await,
            };
            self.apply(id, result);
        });
    }

    fn apply(&self, id: ProviderId, result: Result<Usage, ProviderError>) {
        let now = now_ms();
        let mut save = false;
        {
            let mut providers = self.providers.lock().unwrap();
            let Some(tracked) = providers.get_mut(&id) else { return };
            tracked.state.refreshing = false;
            match result {
                Ok(usage) => {
                    tracked.state.status = ProviderStatus::Ready;
                    tracked.state.usage = Some(usage);
                    tracked.failures = 0;
                    tracked.next_allowed_at = 0;
                    save = true;
                }
                Err(err) => {
                    tracked.failures = tracked.failures.saturating_add(1);
                    let delay = backoff_ms(id, &err, tracked.failures);
                    tracked.next_allowed_at = now + delay;
                    let retry_at = matches!(err, ProviderError::RateLimited { .. }).then_some(now + delay);
                    tracked.state.status = err.to_status(now, retry_at);
                    // Signed out / not installed: old numbers belong to a session that no longer exists.
                    if matches!(
                        err,
                        ProviderError::NotInstalled | ProviderError::SignedOut(_) | ProviderError::NeedsConsent
                    ) {
                        tracked.state.usage = None;
                        save = true;
                    }
                }
            }
        }
        if save {
            self.save_cache();
        }
        self.emit_state();
    }

    /// Picks up fresh status line data without any network request.
    fn poll_status_line(&self) {
        let settings = self.settings();
        if !settings.show_claude || !settings.claude_status_line {
            return;
        }
        let Some(usage) = statusline::read_snapshot(&paths::statusline_snapshot_file()).and_then(|s| s.to_usage())
        else {
            return;
        };
        let changed = {
            let mut providers = self.providers.lock().unwrap();
            let Some(t) = providers.get_mut(&ProviderId::Claude) else { return };
            let newer = t.state.usage.as_ref().is_none_or(|u| usage.fetched_at > u.fetched_at);
            if newer && !t.state.refreshing {
                t.state.usage = Some(usage);
                if !matches!(t.state.status, ProviderStatus::Expired | ProviderStatus::RateLimited { .. }) {
                    t.state.status = ProviderStatus::Ready;
                }
                true
            } else {
                false
            }
        };
        if changed {
            self.save_cache();
            self.emit_state();
        }
    }

    fn save_cache(&self) {
        let providers = self.providers.lock().unwrap();
        let cache: HashMap<ProviderId, Usage> =
            providers.iter().filter_map(|(id, t)| t.state.usage.clone().map(|u| (*id, u))).collect();
        drop(providers);
        if let Ok(json) = serde_json::to_vec(&cache) {
            if let Err(err) = paths::write_atomic(&paths::cache_file(), &json) {
                log::warn!("couldn't save usage cache: {err}");
            }
        }
    }

    /// Background loop: timer refreshes, window resets and status line polling.
    pub fn start_ticker(self: &Arc<Self>) {
        let core = self.clone();
        tauri::async_runtime::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(15));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            // Providers whose window reset without a refresh, already shown as reset.
            let mut announced = std::collections::HashSet::new();
            loop {
                ticker.tick().await;
                core.poll_status_line();
                let interval = core.settings().refresh_interval.seconds();
                let overlay_visible = crate::overlay::is_visible(&core.app);
                let mut announce = false;
                for id in ProviderId::ALL {
                    let reset = {
                        let providers = core.providers.lock().unwrap();
                        providers
                            .get(&id)
                            .and_then(|t| t.state.usage.as_ref())
                            .is_some_and(|u| has_reset_since_fetch(u, now_ms()))
                    };
                    if reset && (overlay_visible || interval.is_some()) {
                        core.clone().refresh(id, Reason::WindowReset);
                    } else if interval.is_some() {
                        core.clone().refresh(id, Reason::Timer);
                    } else if reset {
                        // No refresh will run (refresh only when opened), but the
                        // tray tooltip should still stop showing the old numbers.
                        announce |= announced.insert(id);
                        continue;
                    }
                    announced.remove(&id);
                }
                if announce {
                    core.emit_state();
                }
            }
        });
    }

    pub fn mark_overlay_ready(&self) -> bool {
        let mut overlay = self.overlay.lock().unwrap();
        overlay.ready = true;
        std::mem::take(&mut overlay.show_when_ready)
    }
}

fn load_cache() -> HashMap<ProviderId, Usage> {
    std::fs::read(paths::cache_file())
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{LimitWindow, UsageSource};

    fn tracked(id: ProviderId) -> Tracked {
        Tracked::new(id)
    }

    fn usage(fetched_at: i64) -> Usage {
        Usage {
            five_hour: Some(LimitWindow::new(10.0, Some(2_000), Some(300))),
            weekly: None,
            plan: None,
            source: UsageSource::CodexAppServer,
            fetched_at,
            blocked: false,
        }
    }

    #[test]
    fn never_runs_two_refreshes_at_once() {
        let mut t = tracked(ProviderId::Codex);
        t.state.refreshing = true;
        assert!(!should_refresh(ProviderId::Codex, &t, Reason::Manual, 1_000_000, Some(300), false));
    }

    #[test]
    fn open_only_refreshes_stale_data() {
        let now = 10_000_000;
        let mut t = tracked(ProviderId::Codex);
        t.state.status = ProviderStatus::Ready;
        t.state.usage = Some(usage(now - 30_000));
        t.state.last_attempt_at = Some(now - 30_000);
        assert!(!should_refresh(ProviderId::Codex, &t, Reason::Open, now, Some(300), false));
        t.state.usage = Some(usage(now - 90_000));
        t.state.last_attempt_at = Some(now - 90_000);
        assert!(should_refresh(ProviderId::Codex, &t, Reason::Open, now, Some(300), false));
        // Claude tolerates older data before hitting the network again.
        let mut c = t.clone();
        c.state.id = ProviderId::Claude;
        assert!(!should_refresh(ProviderId::Claude, &c, Reason::Open, now, Some(300), false));
    }

    #[test]
    fn timer_respects_interval_and_manual_mode() {
        let now = 10_000_000;
        let mut t = tracked(ProviderId::Codex);
        t.state.last_attempt_at = Some(now - 200_000);
        assert!(!should_refresh(ProviderId::Codex, &t, Reason::Timer, now, Some(300), false));
        assert!(should_refresh(ProviderId::Codex, &t, Reason::Timer, now, Some(120), false));
        assert!(!should_refresh(ProviderId::Codex, &t, Reason::Timer, now, None, false));
    }

    #[test]
    fn rate_limits_are_respected_even_for_manual_refresh() {
        let now = 10_000_000;
        let mut t = tracked(ProviderId::Claude);
        t.state.last_attempt_at = Some(now - 120_000);
        t.state.status = ProviderStatus::RateLimited { retry_at: Some(now + 60_000) };
        t.next_allowed_at = now + 60_000;
        assert!(!should_refresh(ProviderId::Claude, &t, Reason::Manual, now, Some(300), false));
        assert!(!should_refresh(ProviderId::Claude, &t, Reason::Open, now, Some(300), false));
        // A plain error can be retried by hand.
        t.state.status = ProviderStatus::Error { message: "x".into() };
        assert!(should_refresh(ProviderId::Claude, &t, Reason::Manual, now, Some(300), false));
        assert!(!should_refresh(ProviderId::Claude, &t, Reason::Timer, now, Some(60), false));
    }

    #[test]
    fn claude_manual_refresh_has_a_floor() {
        let now = 10_000_000;
        let mut t = tracked(ProviderId::Claude);
        t.state.last_attempt_at = Some(now - 10_000);
        assert!(!should_refresh(ProviderId::Claude, &t, Reason::Manual, now, Some(300), false));
        assert!(should_refresh(ProviderId::Claude, &t, Reason::Manual, now, Some(300), true), "local reads are cheap");
    }

    #[test]
    fn backoff_grows_and_honors_retry_after() {
        let err = ProviderError::Network("x".into());
        assert_eq!(backoff_ms(ProviderId::Codex, &err, 1), 30_000);
        assert_eq!(backoff_ms(ProviderId::Codex, &err, 2), 60_000);
        assert_eq!(backoff_ms(ProviderId::Codex, &err, 40), 15 * 60_000);
        assert_eq!(backoff_ms(ProviderId::Claude, &ProviderError::RateLimited { retry_after_secs: Some(600) }, 1), 600_000);
        assert_eq!(backoff_ms(ProviderId::Claude, &ProviderError::RateLimited { retry_after_secs: Some(1) }, 1), 60_000);
        assert!(backoff_ms(ProviderId::Claude, &ProviderError::RateLimited { retry_after_secs: None }, 1) >= 5 * 60_000);
        assert_eq!(backoff_ms(ProviderId::Claude, &ProviderError::Expired, 9), 60_000);
    }

    #[test]
    fn detects_window_resets() {
        let u = usage(1_000_000); // resets_at = 2_000 s = 2_000_000 ms
        assert!(!has_reset_since_fetch(&u, 1_900_000));
        assert!(!has_reset_since_fetch(&u, 2_010_000), "waits a little after the reset");
        assert!(has_reset_since_fetch(&u, 2_040_000));
        assert!(!has_reset_since_fetch(&usage(2_100_000), 2_200_000), "data fetched after the reset is current");
    }
}
