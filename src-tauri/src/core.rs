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
use crate::providers::{antigravity, claude, codex, statusline, ProviderError};
use crate::updates::{self, UpdateState};
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
    /// The provider asked Quota to slow down; kept even while fallback data is shown.
    pub rate_limited: bool,
    /// Bumped when the provider's configuration changes, so results from requests
    /// started before the change are discarded.
    pub generation: u64,
}

impl Tracked {
    fn new(id: ProviderId) -> Self {
        Self { state: ProviderState::new(id), failures: 0, next_allowed_at: 0, rate_limited: false, generation: 0 }
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
    pub update: UpdateState,
}

pub struct Core {
    pub app: AppHandle,
    settings: Mutex<Settings>,
    providers: Mutex<HashMap<ProviderId, Tracked>>,
    pub shortcut_error: Mutex<Option<String>>,
    pub overlay: Mutex<OverlayState>,
    pub launch_at_startup: AtomicBool,
    first_run: bool,
    /// Serializes usage cache writes.
    cache_lock: Mutex<()>,
    update: Mutex<UpdateState>,
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
        // Codex and Antigravity are asked through their own local processes.
        (ProviderId::Codex | ProviderId::Antigravity, Reason::Manual) => 5,
        (ProviderId::Claude, Reason::Manual) => 30,
        (ProviderId::Codex | ProviderId::Antigravity, Reason::Open) => 20,
        (ProviderId::Claude, Reason::Open) => 60,
        (_, Reason::WindowReset) => 60,
        // Timer reasons are gated by the interval itself; Claude keeps a floor
        // because Anthropic rate limits this endpoint aggressively.
        (ProviderId::Codex | ProviderId::Antigravity, Reason::Timer) => 60,
        (ProviderId::Claude, Reason::Timer) => 120,
    }
}

/// How old data may be before opening the overlay triggers a refresh.
fn open_max_age_secs(id: ProviderId) -> i64 {
    match id {
        ProviderId::Codex | ProviderId::Antigravity => 60,
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
    let rate_limited = tracked.rate_limited || matches!(state.status, ProviderStatus::RateLimited { .. });
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
        ProviderId::Codex | ProviderId::Antigravity => (30_000i64, 15 * 60_000i64),
        ProviderId::Claude => (120_000, 30 * 60_000),
    };
    let exponential = base.saturating_mul(1i64 << failures.saturating_sub(1).min(10)).min(cap);
    match err {
        // Honor the provider's own deadline, even a long one (capped at a day as a safety net).
        ProviderError::RateLimited { retry_after_secs: Some(secs) } => (*secs).clamp(60, 24 * 3600) * 1000,
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

/// Records a refresh result. Returns whether the usage cache needs saving.
fn apply_result(
    tracked: &mut Tracked,
    id: ProviderId,
    result: Result<Usage, ProviderError>,
    fallback: Option<Usage>,
    now: i64,
) -> bool {
    let mut save = false;
    tracked.state.refreshing = false;
    match result {
        Ok(usage) => {
            tracked.state.status = ProviderStatus::Ready;
            tracked.state.usage = Some(usage);
            tracked.failures = 0;
            tracked.next_allowed_at = 0;
            tracked.rate_limited = false;
            save = true;
        }
        Err(err) => {
            tracked.failures = tracked.failures.saturating_add(1);
            let delay = backoff_ms(id, &err, tracked.failures);
            tracked.next_allowed_at = now + delay;
            tracked.rate_limited = matches!(err, ProviderError::RateLimited { .. });
            let retry_at = tracked.rate_limited.then_some(now + delay);
            tracked.state.status = err.to_status(now, retry_at);
            // Signed out / not installed: old numbers belong to a session that no longer exists.
            if matches!(
                err,
                ProviderError::NotInstalled | ProviderError::SignedOut(_) | ProviderError::NeedsConsent
            ) {
                tracked.state.usage = None;
                save = true;
            }
            // Status line data newer than what's shown replaces it. The endpoint's
            // backoff stays in place; if that data is recent the card shows it as
            // current, otherwise the endpoint's error stays visible beside it.
            let newer = |u: &Usage| tracked.state.usage.as_ref().is_none_or(|cur| u.fetched_at > cur.fetched_at);
            if let Some(usage) = fallback.filter(|u| newer(u) && !matches!(err, ProviderError::NotInstalled)) {
                if now - usage.fetched_at < FALLBACK_FRESH_MS {
                    tracked.state.status = ProviderStatus::Ready;
                }
                tracked.state.usage = Some(usage);
                save = true;
            }
        }
    }
    save
}

/// Status line data younger than this counts as current when it stands in for a
/// failed usage endpoint request.
const FALLBACK_FRESH_MS: i64 = 15 * 60_000;

/// True when a window's reset time has passed since the data was fetched.
pub fn has_reset_since_fetch(usage: &Usage, now: i64) -> bool {
    usage.windows().into_iter().any(|(_, _, w)| {
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
        if !settings.antigravity_app {
            if let Some(t) = providers.get_mut(&ProviderId::Antigravity) {
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
            cache_lock: Mutex::new(()),
            // The first check waits a little, so it never slows down startup.
            update: Mutex::new(UpdateState { next_check_at: now_ms() + 20_000, ..Default::default() }),
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
            update: self.update_state(),
        }
    }

    pub fn update_state(&self) -> UpdateState {
        self.update.lock().unwrap().clone()
    }

    /// Looks for a newer release in the background (one check at a time).
    pub fn check_for_updates(self: &Arc<Self>) {
        {
            let mut update = self.update.lock().unwrap();
            if update.checking {
                return;
            }
            update.checking = true;
        }
        self.emit_state();
        let core = self.clone();
        tauri::async_runtime::spawn(async move {
            let result = updates::check().await;
            let now = now_ms();
            let found = {
                let mut update = core.update.lock().unwrap();
                update.checking = false;
                update.checked_at = Some(now);
                match result {
                    Ok(available) => {
                        update.available = available;
                        update.error = None;
                        update.next_check_at = now + updates::CHECK_EVERY_MS;
                    }
                    // Keep what's known; try again sooner than usual.
                    Err(err) => {
                        update.error = Some(err);
                        update.next_check_at = now + 3_600_000;
                    }
                }
                update.available.clone()
            };
            if let Some(update) = found {
                if core.settings().notified_update.as_deref() != Some(update.version.as_str()) {
                    updates::notify(&core.app, &update);
                    let _ = core.modify_settings(|s| s.notified_update = Some(update.version.clone()));
                }
            }
            crate::tray::update_menu(&core.app);
            core.emit_state();
        });
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
            ProviderId::Antigravity => s.show_antigravity,
        }
    }

    /// Claude without the usage endpoint only reads local files — no network.
    fn is_local_only(&self, id: ProviderId) -> bool {
        id == ProviderId::Claude && !self.settings.lock().unwrap().claude_usage_endpoint
    }

    /// Forgets a provider's data and backoff (used when its source changes).
    /// A request still in flight belongs to the old configuration; its result is discarded.
    pub fn reset_provider(&self, id: ProviderId) {
        let mut providers = self.providers.lock().unwrap();
        let generation = providers.get(&id).map_or(0, |t| t.generation) + 1;
        providers.insert(id, Tracked { generation, ..Tracked::new(id) });
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
        let generation = {
            let mut providers = self.providers.lock().unwrap();
            let Some(tracked) = providers.get_mut(&id) else { return };
            let now = now_ms();
            if !should_refresh(id, tracked, reason, now, settings.refresh_interval.seconds(), local_only) {
                return;
            }
            tracked.state.refreshing = true;
            tracked.state.last_attempt_at = Some(now);
            tracked.generation
        };
        self.emit_state();

        tauri::async_runtime::spawn(async move {
            let result = match id {
                ProviderId::Codex => codex::fetch(settings.codex_path.clone()).await,
                ProviderId::Claude => claude::fetch(&settings).await,
                ProviderId::Antigravity => antigravity::fetch(settings.antigravity_app).await,
            };
            // When Claude's usage endpoint fails, Claude Code's own status line
            // report (if enabled and newer than what's shown) can stand in.
            let fallback = (id == ProviderId::Claude && settings.claude_status_line && result.is_err())
                .then(|| statusline::read_snapshot(&paths::statusline_snapshot_file()))
                .flatten()
                .and_then(|s| s.to_usage());
            self.apply(id, generation, result, fallback);
        });
    }

    fn apply(&self, id: ProviderId, generation: u64, result: Result<Usage, ProviderError>, fallback: Option<Usage>) {
        let now = now_ms();
        let save;
        {
            let mut providers = self.providers.lock().unwrap();
            let Some(tracked) = providers.get_mut(&id) else { return };
            if tracked.generation != generation {
                return; // Started before the provider's configuration changed.
            }
            save = apply_result(tracked, id, result, fallback, now);
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
        // One writer at a time, so an older snapshot can't land after a newer one.
        let _writing = self.cache_lock.lock().unwrap();
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
                let due = now_ms() >= core.update.lock().unwrap().next_check_at;
                if due && core.settings().check_for_updates {
                    core.check_for_updates();
                }
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
            groups: Vec::new(),
        }
    }

    fn bridge(fetched_at: i64) -> Usage {
        Usage { source: UsageSource::ClaudeStatusLine, ..usage(fetched_at) }
    }

    #[test]
    fn status_line_fallback_keeps_the_endpoints_rate_limit() {
        let now = 50_000_000;
        let mut t = tracked(ProviderId::Claude);
        t.state.usage = Some(usage(now - 600_000));
        let limited = Err(ProviderError::RateLimited { retry_after_secs: Some(1800) });
        apply_result(&mut t, ProviderId::Claude, limited, Some(bridge(now - 60_000)), now);
        // Fresh status line data is shown as current...
        assert_eq!(t.state.status, ProviderStatus::Ready);
        assert_eq!(t.state.usage.as_ref().unwrap().source, UsageSource::ClaudeStatusLine);
        // ...but Anthropic's Retry-After still holds every kind of refresh back.
        assert_eq!(t.next_allowed_at, now + 1_800_000);
        assert!(t.rate_limited);
        for reason in [Reason::Open, Reason::Timer, Reason::Manual] {
            assert!(!should_refresh(ProviderId::Claude, &t, reason, now + 600_000, Some(300), false), "{reason:?}");
        }
        assert!(should_refresh(ProviderId::Claude, &t, Reason::Open, now + 1_800_001, Some(300), false));
    }

    #[test]
    fn status_line_fallback_never_replaces_newer_data_or_hides_errors() {
        let now = 50_000_000;
        let mut t = tracked(ProviderId::Claude);
        t.state.usage = Some(usage(now - 60_000));
        let failed = || Err(ProviderError::Network("offline".into()));
        // Older than what's shown: ignored.
        apply_result(&mut t, ProviderId::Claude, failed(), Some(bridge(now - 3_600_000)), now);
        assert_eq!(t.state.usage.as_ref().unwrap().source, UsageSource::CodexAppServer);
        assert!(matches!(t.state.status, ProviderStatus::Error { .. }));

        // Newer but not recent: shown, with the endpoint's error kept visible.
        let mut t = tracked(ProviderId::Claude);
        t.state.usage = Some(usage(now - 7_200_000));
        apply_result(&mut t, ProviderId::Claude, failed(), Some(bridge(now - 3_600_000)), now);
        assert_eq!(t.state.usage.as_ref().unwrap().fetched_at, now - 3_600_000);
        assert!(matches!(t.state.status, ProviderStatus::Error { .. }));
    }

    #[test]
    fn success_clears_the_rate_limit() {
        let mut t = tracked(ProviderId::Claude);
        t.rate_limited = true;
        t.next_allowed_at = i64::MAX;
        apply_result(&mut t, ProviderId::Claude, Ok(usage(1)), None, 2);
        assert!(!t.rate_limited);
        assert_eq!(t.next_allowed_at, 0);
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
