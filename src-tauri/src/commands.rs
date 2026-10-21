//! Commands invoked by the overlay and settings pages.

use serde::Serialize;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt;

use crate::core::{AppSnapshot, Core, Reason};
use crate::model::ProviderId;
use crate::providers::{claude, codex, statusline};
use crate::settings::Settings;

type CoreState<'a> = State<'a, Arc<Core>>;

#[tauri::command]
pub fn get_state(core: CoreState) -> AppSnapshot {
    core.snapshot()
}

/// Applies display and general preferences. Shortcut, Claude access, overlay
/// position and first-run flags have dedicated paths and are not changed here.
#[tauri::command]
pub fn update_settings(app: AppHandle, core: CoreState, settings: Settings) -> Result<AppSnapshot, String> {
    let current = core.settings();
    let next = Settings {
        shortcut: current.shortcut.clone(),
        claude_usage_endpoint: current.claude_usage_endpoint,
        claude_status_line: current.claude_status_line,
        onboarded: current.onboarded,
        // Changed only by dragging the overlay or the reset command.
        overlay_position: current.overlay_position,
        codex_path: settings.codex_path.as_ref().map(|p| p.trim().to_string()).filter(|p| !p.is_empty()),
        ..settings
    };
    next.validate()?;
    core.set_settings(next.clone()).map_err(|_| "Couldn't save settings.".to_string())?;

    if next.theme != current.theme {
        crate::overlay::apply_theme(&app, next.theme);
    }
    if next.codex_path != current.codex_path {
        core.reset_provider(ProviderId::Codex);
    }
    let core = core.inner().clone();
    if (next.show_codex && !current.show_codex) || next.codex_path != current.codex_path {
        core.clone().refresh(ProviderId::Codex, Reason::Startup);
    }
    if next.show_claude && !current.show_claude {
        core.clone().refresh(ProviderId::Claude, Reason::Startup);
    }
    core.emit_state();
    Ok(core.snapshot())
}

#[tauri::command]
pub fn set_shortcut(app: AppHandle, shortcut: String) -> Result<String, String> {
    crate::shortcut::change(&app, &shortcut)
}

#[tauri::command]
pub fn pause_shortcut(app: AppHandle, paused: bool) {
    crate::shortcut::pause(&app, paused);
}

#[tauri::command]
pub fn set_launch_at_startup(app: AppHandle, core: CoreState, enabled: bool) -> Result<bool, String> {
    let autolaunch = app.autolaunch();
    let result = if enabled { autolaunch.enable() } else { autolaunch.disable() };
    result.map_err(|_| "Windows didn't allow changing the startup setting.".to_string())?;
    let actual = autolaunch.is_enabled().unwrap_or(enabled);
    core.launch_at_startup.store(actual, Ordering::Relaxed);
    core.emit_state();
    Ok(actual)
}

#[tauri::command]
pub fn refresh_now(core: CoreState, provider: Option<ProviderId>) {
    match provider {
        Some(id) => core.inner().clone().refresh(id, Reason::Manual),
        None => core.refresh_all(Reason::Manual),
    }
}

#[tauri::command]
pub fn hide_overlay(app: AppHandle) {
    crate::overlay::hide(&app);
}

#[tauri::command]
pub fn overlay_ready(app: AppHandle, core: CoreState) {
    if core.mark_overlay_ready() {
        crate::overlay::show(&app);
    }
}

#[tauri::command]
pub fn resize_overlay(app: AppHandle, height: f64) {
    crate::overlay::set_height(&app, height);
}

/// Async so it never runs on the main thread (see `overlay::open_settings`).
#[tauri::command]
pub async fn open_settings(app: AppHandle, section: Option<String>) {
    crate::overlay::open_settings(&app, section);
}

#[tauri::command]
pub fn start_overlay_drag(app: AppHandle) {
    crate::overlay::start_drag(&app);
}

#[tauri::command]
pub fn reset_overlay_position(app: AppHandle) {
    crate::overlay::reset_position(&app);
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    crate::overlay::remember_position(&app);
    app.exit(0);
}

/// Grants or revokes permission to use Anthropic's undocumented usage endpoint.
#[tauri::command]
pub fn set_claude_usage_endpoint(core: CoreState, enabled: bool) -> Result<AppSnapshot, String> {
    let mut settings = core.settings();
    if settings.claude_usage_endpoint == enabled {
        return Ok(core.snapshot());
    }
    settings.claude_usage_endpoint = enabled;
    core.set_settings(settings).map_err(|_| "Couldn't save settings.".to_string())?;
    core.reset_provider(ProviderId::Claude);
    core.inner().clone().refresh(ProviderId::Claude, Reason::Startup);
    core.emit_state();
    Ok(core.snapshot())
}

/// Installs or removes the Claude Code status line bridge.
#[tauri::command]
pub fn set_claude_status_line(core: CoreState, enabled: bool) -> Result<AppSnapshot, String> {
    let dir = claude::config_dir().ok_or("Couldn't find your home folder.")?;
    if enabled {
        let exe = std::env::current_exe().map_err(|_| "Couldn't locate Quota's executable.".to_string())?;
        statusline::install(&dir, &exe)?;
    } else {
        statusline::uninstall(&dir)?;
        let _ = std::fs::remove_file(crate::paths::statusline_snapshot_file());
    }
    let mut settings = core.settings();
    settings.claude_status_line = enabled;
    core.set_settings(settings).map_err(|_| "Couldn't save settings.".to_string())?;
    core.reset_provider(ProviderId::Claude);
    core.inner().clone().refresh(ProviderId::Claude, Reason::Startup);
    core.emit_state();
    Ok(core.snapshot())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDetails {
    codex_path: Option<String>,
    claude_installed: bool,
    claude_signed_in: bool,
    claude_status_line: statusline::BridgeState,
}

/// Local detection details shown in Settings. Reads no credentials.
#[tauri::command]
pub async fn provider_details(core: State<'_, Arc<Core>>) -> Result<ProviderDetails, String> {
    let explicit = core.settings().codex_path;
    let details = tauri::async_runtime::spawn_blocking(move || {
        let codex_path = codex::locate(explicit.as_deref()).ok().map(|p| p.display().to_string());
        let dir = claude::config_dir();
        ProviderDetails {
            codex_path,
            claude_installed: dir.as_deref().is_some_and(claude::is_installed),
            claude_signed_in: dir.as_deref().is_some_and(|d| claude::credentials_file(d).exists()),
            claude_status_line: dir
                .as_deref()
                .map(statusline::state)
                .unwrap_or(statusline::BridgeState::Unavailable),
        }
    })
    .await
    .map_err(|e| e.to_string())?;
    Ok(details)
}
