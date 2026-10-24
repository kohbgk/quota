//! Quota — your AI usage, one shortcut away.

pub mod commands;
pub mod core;
pub mod model;
pub mod overlay;
pub mod paths;
pub mod providers;
pub mod settings;
pub mod shortcut;
pub mod tray;

use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::{Manager, RunEvent, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

use crate::core::{Core, Reason};
use crate::settings::Settings;

/// Command-line flag passed by the startup entry so Quota starts hidden.
pub const BACKGROUND_FLAG: &str = "--background";

pub fn run(start_hidden: bool) {
    tauri::Builder::default()
        // Must be registered first: a second launch just reveals the running instance.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| overlay::show(app)))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec![BACKGROUND_FLAG])))
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::update_settings,
            commands::set_shortcut,
            commands::pause_shortcut,
            commands::set_launch_at_startup,
            commands::refresh_now,
            commands::hide_overlay,
            commands::overlay_ready,
            commands::resize_overlay,
            commands::open_settings,
            commands::start_overlay_drag,
            commands::reset_overlay_position,
            commands::quit_app,
            commands::set_claude_usage_endpoint,
            commands::set_claude_status_line,
            commands::provider_details,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            let settings = Settings::load();
            let first_run = !settings.onboarded;

            if first_run {
                // Quota is only useful while it runs, so start with Windows by default.
                // The user can turn this off in Settings.
                let _ = app.autolaunch().enable();
            }
            let core = Core::new(handle.clone(), settings.clone(), first_run);
            core.launch_at_startup.store(app.autolaunch().is_enabled().unwrap_or(false), Ordering::Relaxed);
            app.manage(core.clone());

            if let Some(win) = app.get_webview_window(overlay::OVERLAY) {
                let _ = win.set_background_color(Some(overlay::background_for(&handle, settings.theme)));
                let app_for_events = handle.clone();
                win.on_window_event(move |event| match event {
                    WindowEvent::Focused(false) => overlay::on_blur(&app_for_events),
                    WindowEvent::Moved(to) => overlay::on_moved(&app_for_events, *to),
                    WindowEvent::CloseRequested { api, .. } => {
                        api.prevent_close();
                        overlay::hide(&app_for_events);
                    }
                    _ => {}
                });
            }

            shortcut::register_saved(&handle);
            tray::create(&handle)?;
            core.start_ticker();
            core.refresh_all(Reason::Startup);

            // Show the overlay when launched by hand, on first run, or when the
            // shortcut couldn't be registered (so the user learns about it).
            let shortcut_failed = core.shortcut_error.lock().unwrap().is_some();
            if !start_hidden || first_run || shortcut_failed {
                overlay::show(&handle);
            }
            if first_run {
                let _ = core.modify_settings(|s| s.onboarded = true);
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to start Quota")
        .run(|app, event| {
            // Closing the settings window must not quit the app; only Quit does.
            if let RunEvent::ExitRequested { api, code, .. } = event {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
            let _ = app;
        });
}

/// Removes Quota's integrations. Run by the uninstaller.
pub fn cleanup() {
    if let Some(dir) = providers::claude::config_dir() {
        let _ = providers::statusline::uninstall(&dir);
    }
}

pub fn shared_core(app: &tauri::AppHandle) -> Arc<Core> {
    app.state::<Arc<Core>>().inner().clone()
}
