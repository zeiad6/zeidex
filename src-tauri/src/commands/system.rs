use std::path::Path;
use std::process::Command;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

use tauri::State;

use crate::state::AppState;

// These commands start a process or touch SQLite. Neither belongs on the main
// thread - `Command::spawn` and a connection waiting on the write lock can both
// block - so every command here runs on the async runtime.

/// Opens a file or folder with the platform handler. `path` comes from an
/// indexed candidate, so it is a path the walker already vetted; a failure is
/// reported to the UI rather than unwrapped.
#[tauri::command(async)]
pub fn open_target(path: String) -> Result<(), String> {
    open_path(&path).map_err(|err| err.to_string())
}

/// Reveals a file in its container (Explorer / Finder / file manager).
#[tauri::command(async)]
pub fn reveal_target(path: String) -> Result<(), String> {
    reveal_path(&path).map_err(|err| err.to_string())
}

/// Opens a URL with the platform handler (default browser). The shell only
/// ever builds this from its own search URL, never from user-typed text, so a
/// malformed URL fails as an error instead of reaching a shell.
#[tauri::command(async)]
pub fn open_url(url: String) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(format!("refusing to open non-http URL: {url}"));
    }
    open_path(&url).map_err(|err| err.to_string())
}

#[tauri::command]
pub fn db_path(state: State<'_, AppState>) -> Result<String, String> {
    Ok(state.db_path.to_string_lossy().into_owned())
}

/// The resolved `~/.zeidex/config` path, so the UI can offer "open config".
#[tauri::command]
pub fn config_path() -> Result<String, String> {
    zeidex_engine::config_path::current()
        .map(|resolved| resolved.path.to_string_lossy().into_owned())
        .ok_or_else(|| "no home directory".to_string())
}

#[tauri::command]
pub fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Builds the search-engine URL for a query, honouring the user's chosen engine
/// and the "web search disabled" setting. The URL is then opened by `open_url`.
#[tauri::command(async)]
pub fn web_search_url(state: State<'_, AppState>, query: String) -> Result<String, String> {
    let settings = {
        let store = state
            .store
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        store
            .load_search_settings()
            .map_err(|err| err.to_string())?
    };
    zeidex_engine::QueryEngine::build_web_search_url(&query, settings)
        .ok_or_else(|| "web search is disabled or the query is empty".to_string())
}

#[cfg(target_os = "windows")]
fn open_path(path: &str) -> std::io::Result<()> {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::{HSTRING, PCWSTR};

    let operation = HSTRING::from("open");
    let target = HSTRING::from(path);
    // SAFETY: both `HSTRING`s own valid NUL-terminated UTF-16 for the whole
    // call, and the two unused parameters are explicitly null.
    let result = unsafe {
        ShellExecuteW(
            None,
            &operation,
            &target,
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };

    // ShellExecuteW reports values above 32 as success; anything at or below is
    // an error code from the shell.
    let code = result.0 as isize;
    if code > 32 {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "the shell could not open {path} (code {code})"
        )))
    }
}

#[cfg(target_os = "macos")]
fn open_path(path: &str) -> std::io::Result<()> {
    spawn_detached(Command::new("open").arg(path))
}

#[cfg(target_os = "linux")]
fn open_path(path: &str) -> std::io::Result<()> {
    spawn_detached(Command::new("xdg-open").arg(path))
}

/// Starts the platform handler and reaps it in the background.
///
/// `Child` does not reap on drop, so a bare `spawn()` leaves one zombie per
/// opened file for the rest of the session - `open` and `xdg-open` exit in
/// milliseconds and nobody collects them. `status()` would reap, but it blocks
/// the IPC thread on a handler that may stay alive as long as the application
/// it launched; a detached waiter costs one short-lived thread instead.
fn spawn_detached(command: &mut Command) -> std::io::Result<()> {
    let child = command.spawn()?;
    // If the thread itself cannot be created the child is still started and
    // dropped unreaped, which is exactly the old behaviour - not worth failing
    // the open over.
    let _ = std::thread::Builder::new()
        .name("zeidex-opener-reaper".to_string())
        .spawn(move || {
            let mut child = child;
            let _ = child.wait();
        });
    Ok(())
}

#[cfg(target_os = "windows")]
fn reveal_path(path: &str) -> std::io::Result<()> {
    spawn_detached(
        Command::new("explorer")
            .args(["/select,", path])
            .creation_flags(CREATE_NO_WINDOW),
    )
}

#[cfg(target_os = "macos")]
fn reveal_path(path: &str) -> std::io::Result<()> {
    spawn_detached(Command::new("open").args(["-R", path]))
}

#[cfg(target_os = "linux")]
fn reveal_path(path: &str) -> std::io::Result<()> {
    // Most Linux file managers have no "reveal" protocol; opening the parent
    // folder is the closest portable behaviour.
    let parent = Path::new(path)
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    let target = parent.unwrap_or(Path::new(path));
    spawn_detached(Command::new("xdg-open").arg(target))
}
