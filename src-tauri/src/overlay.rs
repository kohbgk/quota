//! The overlay window: showing, hiding, placing and moving it on the right monitor.

use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, LogicalSize, Manager, Monitor, PhysicalPosition, WebviewWindow};

use crate::core::{Core, Reason};
use crate::paths::now_ms;
use crate::settings::{OverlayPosition, Theme};

pub const OVERLAY: &str = "overlay";
pub const SETTINGS: &str = "settings";
pub const WIDTH: f64 = 372.0;

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
    let shown_at = now_ms();
    core.overlay.lock().unwrap().shown_at = shown_at;
    #[cfg(windows)]
    watch_outside_clicks(app, shown_at);
    let _ = app.emit_to(OVERLAY, "quota://overlay-shown", ());
    core.refresh_all(Reason::Open);
}

/// Hides the overlay when the user clicks anywhere outside it. Focus-loss events
/// alone miss this when Windows didn't hand the overlay focus (for example right
/// after launch), so the mouse buttons are checked directly while it's open.
#[cfg(windows)]
fn watch_outside_clicks(app: &AppHandle, shown_at: i64) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON};
    let button_down = || {
        [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON].iter().any(|&vk| {
            // SAFETY: GetAsyncKeyState only reads input state; the high bit means "down".
            let state = unsafe { GetAsyncKeyState(i32::from(vk)) };
            state < 0
        })
    };
    let app = app.clone();
    std::thread::spawn(move || {
        let mut was_down = button_down();
        loop {
            std::thread::sleep(Duration::from_millis(50));
            let core = self::core(&app);
            let (open, ignore) = {
                let overlay = core.overlay.lock().unwrap();
                let now = now_ms();
                // A newer show starts its own watcher; a hide ends this one.
                let open = overlay.shown_at == shown_at && overlay.hidden_at < shown_at;
                let busy = now - shown_at < 300 || now - overlay.drag_started_at < 1000 || now - overlay.moved_at < SETTLE_MS;
                (open, busy)
            };
            if !open {
                return;
            }
            let down = button_down();
            let pressed = down && !was_down;
            was_down = down;
            if !pressed || ignore || !core.settings().hide_on_blur {
                continue;
            }
            let Some(win) = window(&app) else { return };
            let (Ok(cursor), Ok(pos), Ok(size)) = (app.cursor_position(), win.outer_position(), win.outer_size()) else {
                continue;
            };
            let inside = cursor.x >= pos.x as f64
                && cursor.x < (pos.x + size.width as i32) as f64
                && cursor.y >= pos.y as f64
                && cursor.y < (pos.y + size.height as i32) as f64;
            if !inside {
                hide(&app);
                return;
            }
        }
    });
}

pub fn hide(app: &AppHandle) {
    let Some(win) = window(app) else { return };
    remember_position(app);
    let _ = win.hide();
    core(app).overlay.lock().unwrap().hidden_at = now_ms();
}

/// Called when the overlay loses focus.
pub fn on_blur(app: &AppHandle) {
    let core = core(app);
    if !core.settings().hide_on_blur || !is_visible(app) {
        return;
    }
    let (just_shown, dragging) = {
        let overlay = core.overlay.lock().unwrap();
        let now = now_ms();
        (now - overlay.shown_at < 250, now - overlay.drag_started_at < 1000 || now - overlay.moved_at < SETTLE_MS)
    };
    // Ignore the focus churn that can happen right after the window appears.
    if just_shown {
        return;
    }
    if dragging {
        // Some systems move focus away during a drag. Decide once the drag has
        // settled: if the overlay still isn't focused, the user clicked away.
        let app = app.clone();
        std::thread::spawn(move || {
            // Wait until no drag has started or moved the window for a while
            // (another drag may begin before this one is judged).
            loop {
                std::thread::sleep(Duration::from_millis(250));
                let core = self::core(&app);
                let overlay = core.overlay.lock().unwrap();
                let now = now_ms();
                if now - overlay.drag_started_at >= 1000 && now - overlay.moved_at >= SETTLE_MS {
                    break;
                }
            }
            let focused = window(&app).and_then(|w| w.is_focused().ok()).unwrap_or(true);
            if !focused && is_visible(&app) && self::core(&app).settings().hide_on_blur {
                hide(&app);
            }
        });
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
        if win.is_visible().unwrap_or(false) {
            keep_on_screen(&win, height);
        }
    }
}

// ---------- Placement ----------

/// A monitor's work area (excluding the taskbar) in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Area {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

impl Area {
    fn of(monitor: &Monitor) -> Area {
        let area = monitor.work_area();
        Area { x: area.position.x, y: area.position.y, width: area.size.width as i32, height: area.size.height as i32 }
    }
}

/// The overlay's size in physical pixels on a monitor with this scale factor.
fn size_px(height: f64, scale: f64) -> (i32, i32) {
    ((WIDTH * scale).round() as i32, (height * scale).round() as i32)
}

/// Top-left corner for a window of `size` at `pos`, kept fully inside `area`.
fn place_in(area: Area, size: (i32, i32), pos: &OverlayPosition) -> (i32, i32) {
    let free_w = (area.width - size.0).max(0);
    let free_h = (area.height - size.1).max(0);
    let x = area.x + (pos.x * free_w as f64).round() as i32;
    let y = area.y + ((pos.y * area.height as f64).round() as i32).clamp(0, free_h);
    (x, y)
}

/// The inverse of `place_in`: where a window at `top_left` sits within `area`.
fn position_in(area: Area, size: (i32, i32), top_left: (i32, i32)) -> OverlayPosition {
    let free_w = area.width - size.0;
    let x = if free_w > 0 { (top_left.0 - area.x) as f64 / free_w as f64 } else { 0.5 };
    let y = (top_left.1 - area.y) as f64 / area.height.max(1) as f64;
    OverlayPosition { x: x.clamp(0.0, 1.0), y: y.clamp(0.0, 1.0), monitor: None }
}

/// Places the overlay where the user left it (or at the default spot on the monitor
/// with the mouse cursor), using that monitor's own scale factor.
fn place_on_active_monitor(app: &AppHandle, win: &WebviewWindow) {
    let core = core(app);
    let height = core.overlay.lock().unwrap().height;
    let pos = core.settings().overlay_position.unwrap_or(OverlayPosition::DEFAULT);
    let saved_monitor = pos.monitor.as_deref().and_then(|name| {
        app.available_monitors().ok()?.into_iter().find(|m| m.name().map(String::as_str) == Some(name))
    });
    let monitor = saved_monitor
        .or_else(|| {
            let p = app.cursor_position().ok()?;
            app.monitor_from_point(p.x, p.y).ok().flatten()
        })
        .or_else(|| win.current_monitor().ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else { return };

    let (x, y) = place_in(Area::of(&monitor), size_px(height, monitor.scale_factor()), &pos);
    {
        let mut overlay = core.overlay.lock().unwrap();
        overlay.dragged = false;
        overlay.placed_at = Some((x, y));
    }
    let _ = win.set_position(PhysicalPosition::new(x, y));
    // Re-apply the logical size: moving across monitors can change the scale factor.
    let _ = win.set_size(LogicalSize::new(WIDTH, height));
}

/// Nudges the overlay back inside its monitor after it grew taller.
fn keep_on_screen(win: &WebviewWindow, height: f64) {
    let (Ok(top_left), Ok(Some(monitor))) = (win.outer_position(), win.current_monitor()) else { return };
    let area = Area::of(&monitor);
    let size = size_px(height, monitor.scale_factor());
    let x = top_left.x.clamp(area.x, area.x + (area.width - size.0).max(0));
    let y = top_left.y.clamp(area.y, area.y + (area.height - size.1).max(0));
    if (x, y) != (top_left.x, top_left.y) {
        let _ = win.set_position(PhysicalPosition::new(x, y));
    }
}

// ---------- Moving ----------

/// Starts moving the overlay with the mouse (called on mouse down).
pub fn start_drag(app: &AppHandle) {
    let Some(win) = window(app) else { return };
    {
        let core = core(app);
        let mut overlay = core.overlay.lock().unwrap();
        overlay.dragged = true;
        overlay.drag_started_at = now_ms();
    }
    let _ = win.start_dragging();
}

/// Called for every overlay move. Once a drag has settled, saves the new spot
/// right away, so it survives even if Quota isn't closed normally.
pub fn on_moved(app: &AppHandle) {
    let core = core(app);
    {
        let mut overlay = core.overlay.lock().unwrap();
        if !overlay.dragged {
            return; // Moved by Quota, not the user.
        }
        overlay.moved_at = now_ms();
        if std::mem::replace(&mut overlay.save_pending, true) {
            return;
        }
    }
    let app = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(SETTLE_MS as u64));
        let core = self::core(&app);
        {
            let mut overlay = core.overlay.lock().unwrap();
            if now_ms() - overlay.moved_at < SETTLE_MS {
                continue;
            }
            overlay.save_pending = false;
        }
        remember_position(&app);
        return;
    });
}

/// How long the overlay must stay put before a drag counts as finished.
const SETTLE_MS: i64 = 400;

/// Saves where the user dragged the overlay, relative to the monitor it ended up on.
pub fn remember_position(app: &AppHandle) {
    let Some(win) = window(app) else { return };
    let core = core(app);
    let (height, placed_at) = {
        let overlay = core.overlay.lock().unwrap();
        if !overlay.dragged {
            return;
        }
        (overlay.height, overlay.placed_at)
    };
    let (Ok(top_left), Ok(Some(monitor))) = (win.outer_position(), win.current_monitor()) else { return };
    if placed_at == Some((top_left.x, top_left.y)) {
        return; // A click without movement.
    }
    let position = OverlayPosition {
        monitor: monitor.name().cloned(),
        ..position_in(Area::of(&monitor), size_px(height, monitor.scale_factor()), (top_left.x, top_left.y))
    };
    let mut settings = core.settings();
    if settings.overlay_position.as_ref() == Some(&position) {
        return;
    }
    settings.overlay_position = Some(position);
    if core.set_settings(settings).is_ok() {
        core.emit_state();
    }
}

/// Moves the overlay back to its default spot.
pub fn reset_position(app: &AppHandle) {
    let core = core(app);
    core.overlay.lock().unwrap().dragged = false;
    let mut settings = core.settings();
    settings.overlay_position = None;
    let _ = core.set_settings(settings);
    if let Some(win) = window(app) {
        if win.is_visible().unwrap_or(false) {
            place_on_active_monitor(app, &win);
        }
    }
    core.emit_state();
}

// ---------- Theme ----------

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

// ---------- Settings window ----------

/// Opens (or focuses) the settings window, optionally scrolled to a section.
pub fn open_settings(app: &AppHandle, section: Option<String>) {
    hide(app);
    if let Some(win) = app.get_webview_window(SETTINGS) {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
        if let Some(section) = section {
            let _ = app.emit_to(SETTINGS, "quota://open-section", section);
        }
        return;
    }
    {
        let core = core(app);
        let mut overlay = core.overlay.lock().unwrap();
        if overlay.settings_opening {
            return;
        }
        overlay.settings_opening = true;
    }
    // WebView2 deadlocks when a window is created on the main thread from a
    // command or event handler, so the window is built from a worker thread.
    let app = app.clone();
    std::thread::spawn(move || {
        let built = build_settings(&app, section);
        core(&app).overlay.lock().unwrap().settings_opening = false;
        match built {
            Ok(win) => {
                center_on_active_monitor(&app, &win, 600.0, 680.0);
                // The page shows itself once it has painted. If it hasn't after a
                // moment, show it anyway rather than leave an invisible window.
                std::thread::sleep(Duration::from_millis(1500));
                if let Some(win) = app.get_webview_window(SETTINGS) {
                    if !win.is_visible().unwrap_or(true) {
                        let _ = win.show();
                        let _ = win.set_focus();
                    }
                }
            }
            Err(err) => log::error!("couldn't open settings: {err}"),
        }
    });
}

fn build_settings(app: &AppHandle, section: Option<String>) -> tauri::Result<WebviewWindow> {
    let url = match &section {
        Some(s) => format!("settings.html#{s}"),
        None => "settings.html".into(),
    };
    let theme = core(app).settings().theme;
    tauri::WebviewWindowBuilder::new(app, SETTINGS, tauri::WebviewUrl::App(url.into()))
        .title("Quota Settings")
        .inner_size(600.0, 680.0)
        .min_inner_size(520.0, 480.0)
        .resizable(true)
        .maximizable(false)
        .decorations(false)
        .shadow(true)
        .visible(false)
        .background_color(background_for(app, theme))
        .build()
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

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Area = Area { x: 1920, y: 0, width: 2560, height: 1400 };

    #[test]
    fn default_spot_is_centered_in_the_upper_part() {
        let (x, y) = place_in(AREA, (558, 450), &OverlayPosition::DEFAULT);
        assert_eq!(x, 1920 + (2560 - 558) / 2);
        assert_eq!(y, 280);
    }

    #[test]
    fn dragged_spot_round_trips() {
        let size = (558, 450);
        for top_left in [(1920, 0), (1920 + 2560 - 558, 1400 - 450), (2500, 700)] {
            let pos = position_in(AREA, size, top_left);
            assert_eq!(place_in(AREA, size, &pos), top_left, "{pos:?}");
        }
    }

    #[test]
    fn stays_fully_on_screen() {
        // Dragged partly off the right and bottom edges.
        let pos = position_in(AREA, (558, 450), (1920 + 2400, 1300));
        assert_eq!(pos.x, 1.0);
        let (x, y) = place_in(AREA, (558, 450), &pos);
        assert_eq!((x, y), (1920 + 2560 - 558, 1400 - 450));
        // A spot near the bottom on a smaller monitor is pulled up to fit.
        let small = Area { x: 0, y: 0, width: 1280, height: 680 };
        assert_eq!(place_in(small, (372, 600), &OverlayPosition { x: 0.0, y: 0.9, monitor: None }), (0, 80));
    }

    #[test]
    fn same_fractions_on_another_monitor() {
        let pos = position_in(AREA, (558, 450), (1920, 0)); // top-left corner
        let other = Area { x: 0, y: 40, width: 1920, height: 1040 };
        assert_eq!(place_in(other, (372, 300), &pos), (0, 40));
    }
}
