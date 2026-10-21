//! System tray icon and menu.

use std::sync::Arc;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

use crate::core::{AppSnapshot, Core, Reason};
use crate::model::{ProviderStatus, ProviderState};

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
            "quit" => app.exit(0),
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

fn summary(p: &ProviderState, five_hour: bool) -> Option<String> {
    let name = p.id.display_name();
    if !matches!(p.status, ProviderStatus::Ready) {
        return None;
    }
    let usage = p.usage.as_ref()?;
    let (label, window) = if five_hour { ("5h", usage.five_hour.as_ref()) } else { ("week", usage.weekly.as_ref()) };
    let window = window?;
    let left = (100.0 - window.used_percent).round().clamp(0.0, 100.0);
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
            if let Some(line) = summary(p, s.show_five_hour) {
                lines.push(line);
            }
        }
    }
    let _ = tray.set_tooltip(Some(lines.join("\n")));
}
