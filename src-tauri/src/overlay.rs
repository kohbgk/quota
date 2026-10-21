//! The overlay window: showing, hiding and placing it on the right monitor.

use std::sync::Arc;
use tauri::{AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, WebviewWindow};

use crate::core::{Core, Reason};
use crate::paths::now_ms;
use crate::settings::Theme;

pub const OVERLAY: &str = "overlay";
pub const WIDTH: f64 = 372.0;
/// The overlay's top edge sits this far down the monitor's work area.
const TOP_FRACTION: f64 = 0.2;

fn window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(OVERLAY)
}

fn core(app: &AppHandle) -> Arc<Core> {
    app.state::<Arc<Core>>().inner().clone()
}

pub fn is_visible(app: &AppHandle) -> bool {
    window(app).and_then(|w| w.is_visible().ok()).unwrap_or(false)
}

pub fn toggle(app: &AppHandle) {
    if is_visible(app) {
        hide(app);
    } else {
        show(app);
    }
}

pub fn show(app: &AppHandle) {
    let Some(win) = window(app) else { return };
    let core = core(app);
    {
        let mut overlay = core.overlay.lock().unwrap();
        if !overlay.ready {
            // The page is still loading; show it as soon as it has rendered once.
            overlay.show_when_ready = true;
            return;
        }
    }
    place_on_active_monitor(app, &win);
    let _ = win.show();
    let _ = win.unminimize();
    let _ = win.set_focus();
    core.overlay.lock().unwrap().shown_at = now_ms();
    let _ = app.emit_to(OVERLAY, "quota://overlay-shown", ());
    core.refresh_all(Reason::Open);
}

pub fn hide(app: &AppHandle) {
    let Some(win) = window(app) else { return };
    let _ = win.hide();
    core(app).overlay.lock().unwrap().hidden_at = now_ms();
}

/// Called when the overlay loses focus.
pub fn on_blur(app: &AppHandle) {
    let core = core(app);
    if !core.settings().hide_on_blur || !is_visible(app) {
        return;
    }
    // Ignore the focus churn that can happen right after the window appears.
    if now_ms() - core.overlay.lock().unwrap().shown_at < 250 {
        return;
    }
    hide(app);
}

/// Whether the overlay was hidden a moment ago (by the click that is now
/// arriving at the tray icon); used to keep tray clicks a clean toggle.
pub fn just_hidden(app: &AppHandle) -> bool {
    now_ms() - core(app).overlay.lock().unwrap().hidden_at < 350
}

pub fn set_height(app: &AppHandle, height: f64) {
    let height = height.clamp(120.0, 900.0).round();
    let core = core(app);
    core.overlay.lock().unwrap().height = height;
    if let Some(win) = window(app) {
        let _ = win.set_size(LogicalSize::new(WIDTH, height));
    }
}

/// Centers the overlay horizontally in the upper part of the monitor that
/// contains the mouse cursor, using that monitor's own scale factor.
fn place_on_active_monitor(app: &AppHandle, win: &WebviewWindow) {
    let height = core(app).overlay.lock().unwrap().height;
    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| win.current_monitor().ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else { return };

    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    let width_px = (WIDTH * scale).round() as i32;
    let height_px = (height * scale).round() as i32;
    let x = area.position.x + (area.size.width as i32 - width_px) / 2;
    let free = (area.size.height as i32 - height_px).max(0);
    let y = area.position.y + ((area.size.height as f64 * TOP_FRACTION) as i32).min(free);

    let _ = win.set_position(PhysicalPosition::new(x, y));
    // Re-apply the logical size: moving across monitors can change the scale factor.
    let _ = win.set_size(LogicalSize::new(WIDTH, height));
}

/// Window background shown before the page paints, matched to the theme.
pub fn background_for(app: &AppHandle, theme: Theme) -> tauri::window::Color {
    let dark = match theme {
        Theme::Dark => true,
        Theme::Light => false,
        Theme::System => window(app)
            .and_then(|w| w.theme().ok())
            .map(|t| t == tauri::Theme::Dark)
            .unwrap_or(true),
    };
    if dark {
        tauri::window::Color(20, 20, 22, 255)
    } else {
        tauri::window::Color(250, 250, 251, 255)
    }
}

pub fn apply_theme(app: &AppHandle, theme: Theme) {
    let color = background_for(app, theme);
    for win in app.webview_windows().values() {
        let _ = win.set_background_color(Some(color));
    }
}

/// Opens (or focuses) the settings window, optionally scrolled to a section.
pub fn open_settings(app: &AppHandle, section: Option<String>) {
    hide(app);
    if let Some(win) = app.get_webview_window("settings") {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
        if let Some(section) = section {
            let _ = app.emit_to("settings", "quota://open-section", section);
        }
        return;
    }
    let url = match &section {
        Some(s) => format!("settings.html#{s}"),
        None => "settings.html".into(),
    };
    let theme = core(app).settings().theme;
    let builder = tauri::WebviewWindowBuilder::new(app, "settings", tauri::WebviewUrl::App(url.into()))
        .title("Quota Settings")
        .inner_size(600.0, 680.0)
        .min_inner_size(520.0, 480.0)
        .resizable(true)
        .maximizable(false)
        .decorations(false)
        .shadow(true)
        .visible(false)
        .background_color(background_for(app, theme));
    match builder.build() {
        Ok(win) => center_on_active_monitor(app, &win, 600.0, 680.0),
        Err(err) => log::error!("couldn't open settings: {err}"),
    }
}

fn center_on_active_monitor(app: &AppHandle, win: &WebviewWindow, width: f64, height: f64) {
    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else {
        let _ = win.center();
        return;
    };
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    let w = ((width * scale) as i32).min(area.size.width as i32);
    let h = ((height * scale) as i32).min(area.size.height as i32);
    let x = area.position.x + (area.size.width as i32 - w) / 2;
    let y = area.position.y + (area.size.height as i32 - h) / 2;
    let _ = win.set_position(PhysicalPosition::new(x, y));
    let _ = win.set_size(LogicalSize::new(width, height));
}
