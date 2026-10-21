//! System tray icon and menu.

use std::sync::Arc;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

use crate::core::{AppSnapshot, Core, Reason};
use crate::model::{remaining_percent, ProviderStatus, ProviderState};
use crate::settings::Settings;

const TRAY_ID: &str = "quota";

struct TrayMenu {
    show: MenuItem<Wry>,
}

fn show_label(shortcut: &str) -> String {
    // A tab right-aligns the shortcut in Windows menus.
    format!("Show Quota\t{shortcut}")
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let shortcut = app.state::<Arc<Core>>().settings().shortcut;
    let show = MenuItem::with_id(app, "show", show_label(&shortcut), true, None::<&str>)?;
    let refresh = MenuItem::with_id(app, "refresh", "Refresh now", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Quota", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[&show, &PredefinedMenuItem::separator(app)?, &refresh, &settings, &PredefinedMenuItem::separator(app)?, &quit],
    )?;
    app.manage(TrayMenu { show });

    let icon = app.default_window_icon().cloned().expect("bundled icon");
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("Quota")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => crate::overlay::show(app),
            "refresh" => app.state::<Arc<Core>>().refresh_all(Reason::Manual),
            "settings" => crate::overlay::open_settings(app, None),
            "quit" => {
                crate::overlay::remember_position(app);
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                let app = tray.app_handle();
                // The click itself may have just dismissed the overlay via focus loss.
                if !crate::overlay::just_hidden(app) {
                    crate::overlay::toggle(app);
                }
            }
        })
        .build(app)?;
    Ok(())
}

pub fn update_menu(app: &AppHandle) {
    if let Some(menu) = app.try_state::<TrayMenu>() {
        let shortcut = app.state::<Arc<Core>>().settings().shortcut;
        let _ = menu.show.set_text(show_label(&shortcut));
    }
}

/// One tooltip line: the first visible window the provider reports.
fn summary(p: &ProviderState, settings: &Settings, now_ms: i64) -> Option<String> {
    let name = p.id.display_name();
    if !matches!(p.status, ProviderStatus::Ready) {
        return None;
    }
    let usage = p.usage.as_ref()?;
    if usage.blocked {
        return Some(format!("{name}: included usage unavailable"));
    }
    let (label, window) = [
        (settings.show_five_hour, "5h", usage.five_hour.as_ref()),
        (settings.show_weekly, "week", usage.weekly.as_ref()),
    ]
    .into_iter()
    .find_map(|(shown, label, window)| if shown { window.map(|w| (label, w)) } else { None })?;
    if window.has_reset(now_ms) {
        return Some(format!("{name}: {label} limit reset"));
    }
    let left = remaining_percent(window.used_percent);
    Some(format!("{name}: {left}% left ({label})"))
}

/// Keeps the tray tooltip useful at a glance.
pub fn update_tooltip(app: &AppHandle, snapshot: &AppSnapshot) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else { return };
    let s = &snapshot.settings;
    let mut lines = vec![format!("Quota — {}", s.shortcut)];
    for p in &snapshot.providers {
        let visible = match p.id {
            crate::model::ProviderId::Codex => s.show_codex,
            crate::model::ProviderId::Claude => s.show_claude,
        };
        if visible {
            if let Some(line) = summary(p, s, snapshot.now) {
                lines.push(line);
            }
        }
    }
    let _ = tray.set_tooltip(Some(lines.join("\n")));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{LimitWindow, ProviderId, Usage, UsageSource};

    fn ready(five_hour: Option<LimitWindow>, weekly: Option<LimitWindow>) -> ProviderState {
        let usage = Usage { five_hour, weekly, plan: None, source: UsageSource::CodexAppServer, fetched_at: 0, blocked: false };
        ProviderState { status: ProviderStatus::Ready, usage: Some(usage), ..ProviderState::new(ProviderId::Codex) }
    }

    #[test]
    fn tooltip_rounds_down_and_skips_unreported_windows() {
        let settings = Settings::default();
        let both = ready(Some(LimitWindow::new(23.5, Some(2_000), Some(300))), Some(LimitWindow::new(10.0, None, None)));
        assert_eq!(summary(&both, &settings, 1_000_000).as_deref(), Some("Codex: 76% left (5h)"));
        let weekly_only = ready(None, Some(LimitWindow::new(10.0, None, Some(10_080))));
        assert_eq!(summary(&weekly_only, &settings, 0).as_deref(), Some("Codex: 90% left (week)"));
        let reset = ready(Some(LimitWindow::new(90.0, Some(1_000), Some(300))), None);
        assert_eq!(summary(&reset, &settings, 1_000_000).as_deref(), Some("Codex: 5h limit reset"));
    }
}
