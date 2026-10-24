//! The global keyboard shortcut that toggles the overlay.

use std::str::FromStr;
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

use crate::core::Core;

/// Parses and validates an accelerator such as `Ctrl+Shift+Space`, returning
/// a canonical spelling (`Ctrl+Alt+Shift+Win+Key`) and the parsed shortcut.
pub fn normalize(accelerator: &str) -> Result<(String, Shortcut), String> {
    let cleaned: String = accelerator
        .split('+')
        .map(|t| match t.trim().to_ascii_lowercase().as_str() {
            "win" | "windows" | "meta" => "Super".to_string(),
            _ => t.trim().to_string(),
        })
        .collect::<Vec<_>>()
        .join("+");
    let shortcut = Shortcut::from_str(&cleaned).map_err(|_| format!("“{accelerator}” isn't a valid shortcut."))?;
    let mods = shortcut.mods;
    let key = shortcut.key;

    if matches!(
        key,
        Code::ShiftLeft
            | Code::ShiftRight
            | Code::ControlLeft
            | Code::ControlRight
            | Code::AltLeft
            | Code::AltRight
            | Code::MetaLeft
            | Code::MetaRight
    ) {
        return Err("Add a regular key to the modifiers.".into());
    }
    let is_function_key = format!("{key:?}").strip_prefix('F').is_some_and(|n| n.parse::<u8>().is_ok());
    if !is_function_key && !mods.intersects(Modifiers::CONTROL | Modifiers::ALT | Modifiers::SUPER) {
        return Err("Use Ctrl, Alt or Win with another key, so typing isn't interrupted.".into());
    }

    let mut parts = Vec::new();
    if mods.contains(Modifiers::CONTROL) {
        parts.push("Ctrl".to_string());
    }
    if mods.contains(Modifiers::ALT) {
        parts.push("Alt".to_string());
    }
    if mods.contains(Modifiers::SHIFT) {
        parts.push("Shift".to_string());
    }
    if mods.contains(Modifiers::SUPER) {
        parts.push("Win".to_string());
    }
    parts.push(key_name(key));
    Ok((parts.join("+"), shortcut))
}

fn key_name(key: Code) -> String {
    let debug = format!("{key:?}");
    if let Some(letter) = debug.strip_prefix("Key") {
        return letter.to_string();
    }
    if let Some(digit) = debug.strip_prefix("Digit") {
        return digit.to_string();
    }
    debug
}

/// Converts Quota's canonical spelling (with `Win`) into the parser's.
fn to_parser(canonical: &str) -> String {
    canonical.replace("Win", "Super")
}

/// Replaces any registered shortcut with `canonical`.
pub fn register(app: &AppHandle, canonical: &str) -> Result<(), String> {
    let (_, shortcut) = normalize(&to_parser(canonical))?;
    let manager = app.global_shortcut();
    let _ = manager.unregister_all();
    manager
        .on_shortcut(shortcut, |app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                crate::overlay::toggle(app);
            }
        })
        .map_err(|_| {
            format!("{canonical} is already used by another app. Choose a different shortcut in Settings.")
        })
}

/// Registers the saved shortcut at startup, recording any failure for the UI.
pub fn register_saved(app: &AppHandle) {
    let core = app.state::<Arc<Core>>().inner().clone();
    let shortcut = core.settings().shortcut;
    let error = register(app, &shortcut).err();
    *core.shortcut_error.lock().unwrap() = error;
}

/// Validates, registers and persists a new shortcut, keeping the old one on failure.
pub fn change(app: &AppHandle, requested: &str) -> Result<String, String> {
    let core = app.state::<Arc<Core>>().inner().clone();
    let (canonical, _) = normalize(&to_parser(requested))?;
    let previous = core.settings().shortcut;
    if let Err(err) = register(app, &canonical) {
        let restore = register(app, &previous).err();
        *core.shortcut_error.lock().unwrap() = restore;
        core.emit_state();
        return Err(err.replace(" Choose a different shortcut in Settings.", " Try a different combination."));
    }
    if core.modify_settings(|s| s.shortcut = canonical.clone()).is_err() {
        // Keep what's registered in line with what's saved.
        *core.shortcut_error.lock().unwrap() = register(app, &previous).err();
        core.emit_state();
        return Err("Couldn't save the shortcut.".to_string());
    }
    *core.shortcut_error.lock().unwrap() = None;
    crate::tray::update_menu(app);
    core.emit_state();
    Ok(canonical)
}

/// Temporarily releases the shortcut while the user records a new one.
pub fn pause(app: &AppHandle, paused: bool) {
    if paused {
        let _ = app.global_shortcut().unregister_all();
    } else {
        register_saved(app);
        app.state::<Arc<Core>>().emit_state();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalizes_shortcuts() {
        assert_eq!(normalize("ctrl+shift+space").unwrap().0, "Ctrl+Shift+Space");
        assert_eq!(normalize("Shift+Control+KeyQ").unwrap().0, "Ctrl+Shift+Q");
        assert_eq!(normalize("Alt+Digit1").unwrap().0, "Alt+1");
        assert_eq!(normalize("Super+Alt+K").unwrap().0, "Alt+Win+K");
        assert_eq!(normalize("Win+Backquote").unwrap().0, "Win+Backquote");
        assert_eq!(normalize("F8").unwrap().0, "F8");
        assert_eq!(normalize("Shift+F12").unwrap().0, "Shift+F12");
        // Canonical output parses back to itself.
        let canonical = normalize("Ctrl+Alt+Shift+Super+ArrowUp").unwrap().0;
        assert_eq!(canonical, "Ctrl+Alt+Shift+Win+ArrowUp");
        assert_eq!(normalize(&to_parser(&canonical)).unwrap().0, canonical);
    }

    #[test]
    fn rejects_shortcuts_that_would_break_typing() {
        assert!(normalize("A").is_err());
        assert!(normalize("Shift+A").is_err());
        assert!(normalize("Ctrl+Shift").is_err());
        assert!(normalize("").is_err());
        assert!(normalize("Ctrl+Nope").is_err());
    }
}
