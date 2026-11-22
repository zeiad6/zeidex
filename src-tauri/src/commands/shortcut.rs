//! The global launcher shortcut.
//!
//! Registration used to be a one-shot `on_shortcut("Alt+Space")` in setup whose
//! failure went to `eprintln!` - which a release Windows build has no console
//! for. On a machine where another program already owned the combination,
//! Zeidex simply never opened and nothing said why. This module owns
//! registration, keeps the result visible to the UI, persists the user's
//! choice, and falls back to the default when a saved combination is taken.
//!
//! `Alt+Space` stays the default because it is what PowerToys Run and Flow
//! Launcher ship, and Windows only routes it to the focused window's system
//! menu when no application has claimed it globally. The combination is
//! configurable precisely because that claim can fail.

use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use crate::state::AppState;

/// What Zeidex registers when the user has not chosen anything.
pub const DEFAULT_HOTKEY: &str = "Alt+Space";
/// Key in the `settings` table. A constant, never user input.
pub const HOTKEY_SETTING_KEY: &str = "global_shortcut";

#[derive(Clone, Debug, Serialize)]
pub struct HotkeyStatus {
    /// The combination actually registered.
    pub accelerator: String,
    pub registered: bool,
    /// Why registration failed, or why the saved combination was replaced.
    pub error: Option<String>,
    pub default: String,
}

pub struct HotkeyState(pub Mutex<HotkeyStatus>);

fn register(app: &AppHandle, accelerator: &str) -> Result<(), String> {
    let handle = app.clone();
    app.global_shortcut()
        .on_shortcut(accelerator, move |_app, _shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                crate::toggle_window(&handle);
            }
        })
        .map_err(|err| err.to_string())
}

/// Registers `accelerator`, falling back to the default when it is unavailable.
///
/// Unregistering everything first is what keeps this to exactly one live
/// shortcut: re-registering on top of the old one would leave two handlers for
/// the same key and toggle the window twice per press.
pub fn install(app: &AppHandle, accelerator: &str) -> HotkeyStatus {
    let _ = app.global_shortcut().unregister_all();
    match register(app, accelerator) {
        Ok(()) => HotkeyStatus {
            accelerator: accelerator.to_string(),
            registered: true,
            error: None,
            default: DEFAULT_HOTKEY.to_string(),
        },
        Err(err) if accelerator != DEFAULT_HOTKEY => {
            let _ = app.global_shortcut().unregister_all();
            match register(app, DEFAULT_HOTKEY) {
                Ok(()) => HotkeyStatus {
                    accelerator: DEFAULT_HOTKEY.to_string(),
                    registered: true,
                    error: Some(format!(
                        "{accelerator} is unavailable ({err}); using {DEFAULT_HOTKEY}"
                    )),
                    default: DEFAULT_HOTKEY.to_string(),
                },
                Err(fallback_err) => HotkeyStatus {
                    accelerator: accelerator.to_string(),
                    registered: false,
                    error: Some(format!(
                        "{accelerator}: {err}; {DEFAULT_HOTKEY}: {fallback_err}"
                    )),
                    default: DEFAULT_HOTKEY.to_string(),
                },
            }
        }
        Err(err) => HotkeyStatus {
            accelerator: accelerator.to_string(),
            registered: false,
            error: Some(err),
            default: DEFAULT_HOTKEY.to_string(),
        },
    }
}

/// The shortcut the user saved, or `None` when they never chose one.
pub fn saved(state: &AppState) -> Option<String> {
    let store = state
        .store
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    store
        .load_app_setting(HOTKEY_SETTING_KEY)
        .ok()
        .flatten()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[tauri::command(async)]
pub fn hotkey_status(hotkey: State<'_, HotkeyState>) -> HotkeyStatus {
    hotkey
        .0
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// Changes the global shortcut; `None` resets it to the default.
///
/// The change is persisted only when it registered: writing a combination the
/// system refused would make the next launch try, fail, and fall back silently
/// again. A failed attempt leaves the previously working shortcut in place.
#[tauri::command(async)]
pub fn hotkey_set(
    app: AppHandle,
    state: State<'_, AppState>,
    hotkey: State<'_, HotkeyState>,
    accelerator: Option<String>,
) -> Result<HotkeyStatus, String> {
    let wanted = accelerator
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_HOTKEY.to_string());

    let status = install(&app, &wanted);
    {
        let mut guard = hotkey
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = status.clone();
    }

    if !status.registered {
        return Err(status
            .error
            .clone()
            .unwrap_or_else(|| format!("{wanted} could not be registered as a global shortcut")));
    }

    let mut store = state
        .store
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Err(err) = store.save_app_setting(HOTKEY_SETTING_KEY, &status.accelerator) {
        // The shortcut works for this session; only remembering it failed.
        eprintln!("zeidex: the shortcut works but could not be saved: {err}");
    }
    Ok(status)
}
