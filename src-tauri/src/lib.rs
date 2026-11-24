#![deny(clippy::expect_used, clippy::unwrap_used)]

pub mod commands;
pub mod error;
mod hardware;
mod index_service;
mod index_watch;
pub mod state;

use anyhow::anyhow;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager};

use crate::index_service::IndexService;
use crate::state::AppState;

/// Set once the tray icon exists. Closing the window hides it to the tray only
/// when there is a tray to restore it from: on a desktop where the tray icon
/// could not be created, hiding would leave a running process with no visible
/// surface, so Close is allowed to end the process there.
struct TrayAvailable(bool);

/// Writes a startup failure where a GUI build can still be diagnosed.
///
/// A release build has no console, so `eprintln!` from `main` goes nowhere. The
/// log sits next to the database (same directory the user's data lives in) and
/// is append-only and size-capped: a crash loop must not fill the disk.
pub fn report_startup_failure(message: &str) {
    use std::io::Write as _;

    const MAX_LOG_BYTES: u64 = 1 << 20;
    let db_path = state::resolve_db_path();
    let Some(dir) = db_path.parent() else {
        return;
    };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let log = dir.join("zeidex-startup.log");
    if std::fs::metadata(&log).is_ok_and(|meta| meta.len() > MAX_LOG_BYTES)
        && std::fs::remove_file(&log).is_err()
    {
        return;
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default();
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
    {
        let _ = writeln!(file, "[{stamp}] {message}");
    }
}

pub fn run() -> anyhow::Result<()> {
    let app = tauri::Builder::default()
        // Registered first so a second launch exits before any other plugin
        // starts. The running instance raises its window instead: the common
        // case is a user clicking the Start Menu entry while Zeidex sits in the
        // tray, where a second process would add a second tray icon, a second
        // clipboard monitor and a second index watcher over the same database.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            show_main(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            // A fresh install launched from the desktop entry has no window and
            // no tray icon yet, so it looks like nothing happened. The config is
            // created by AppState below, so existence has to be sampled first.
            let first_run = zeidex_engine::config_path::current()
                .is_none_or(|resolved| !resolved.path.exists());
            let db_path = state::resolve_db_path();
            state::prepare_onnx_runtime(app.handle());
            let models_dir = state::resolve_models_dir(app.handle());
            let models_root = state::resolve_models_root();
            let pdfium_library = state::resolve_pdfium_library(app.handle());
            app.manage(AppState::new(
                db_path,
                models_dir,
                models_root,
                pdfium_library,
            )?);
            // Every index write goes through this worker: the startup walk,
            // the filesystem watcher, and the Reindex button. Managing it here,
            // before any command can run, means no caller ever sees a missing
            // state.
            let index_service = IndexService::start(app.handle().clone());
            app.manage(index_service.clone());

            // Text indexed for a file that was later edited or deleted would
            // surface a hit that opens the wrong content. One pass at startup is
            // enough: this index is written by OCR runs the user starts, not by
            // the live file watcher.
            //
            // It holds the store lock for the stat pass, which is why it runs
            // here and not in a command: the window may be up, but the pass is
            // bounded by `MAX_DOCUMENT_ROWS` stats and finishes in milliseconds
            // on local storage.
            let pruner = app.handle().clone();
            let prune_started = std::thread::Builder::new()
                .name("document-index-prune".to_string())
                .spawn(move || {
                    let Some(state) = pruner.try_state::<AppState>() else {
                        return;
                    };
                    let mut store = state
                        .store
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    match store.prune_stale_document_text() {
                        Ok(pruned) if pruned.total() > 0 => eprintln!(
                            "zeidex: dropped {} stale document index entries ({} missing, {} changed)",
                            pruned.total(),
                            pruned.missing,
                            pruned.changed
                        ),
                        Ok(_) => {}
                        Err(err) => eprintln!("zeidex: document index prune failed: {err}"),
                    }
                });
            if let Err(err) = prune_started {
                eprintln!("zeidex: document index prune did not start: {err}");
            }

            // Clipboard capture must not depend on the webview being visible;
            // see `run_monitor`. Failing to start it costs history, not the
            // launcher: refuse to launch over a clipboard thread and a transient
            // resource shortage takes the whole app down.
            let monitor = app.handle().clone();
            let spawn_result = std::thread::Builder::new()
                .name("clipboard-monitor".to_string())
                .spawn(move || commands::clipboard::run_monitor(monitor));
            if let Err(err) = spawn_result {
                eprintln!("zeidex: clipboard monitor did not start: {err}");
            }

            // The saved combination is read before registration so a custom
            // shortcut survives a restart. A saved value the system no longer
            // allows falls back to the default rather than leaving no way in.
            let preferred = {
                let state = app.state::<AppState>();
                commands::shortcut::saved(&state)
                    .unwrap_or_else(|| commands::shortcut::DEFAULT_HOTKEY.to_string())
            };
            let hotkey_status = commands::shortcut::install(app.handle(), &preferred);
            if let Some(err) = hotkey_status.error.as_deref() {
                eprintln!("zeidex: {err}");
            }
            let hotkey_registered = hotkey_status.registered;
            app.manage(commands::shortcut::HotkeyState(std::sync::Mutex::new(
                hotkey_status,
            )));

            let tray_ready = match build_tray(app.handle()) {
                Ok(()) => true,
                Err(err) => {
                    eprintln!("zeidex: tray icon not available: {err}");
                    false
                }
            };
            app.manage(TrayAvailable(tray_ready));

            // One quiet release check, then every six hours. It sends nothing
            // about the user and only emits when a newer release exists.
            commands::update::spawn_background_check(app.handle().clone());

            // Both are ways IN to a window that starts hidden. Losing the tray
            // (GNOME without AppIndicator) or the hotkey (Wayland denies the
            // grab) would otherwise leave a running process with no visible
            // surface and no obvious way to reach it. First launch is shown for
            // the same reason: a desktop entry that appears to do nothing is
            // indistinguishable from one that crashed.
            //
            // A login launch passes `--autostart` (see `commands::autostart`),
            // which is what lets "Start minimized" mean something: the entry
            // stays in the tray and the hotkey brings it up. Any other launch
            // shows the window - a normal double-click that produces nothing
            // reads as a broken app, not as a quiet one.
            let launched_at_login = std::env::args().any(|argument| argument == "--autostart");
            let config = zeidex_engine::config::RuntimeConfig::load_cached();
            // The first-run guide outranks "Start minimized": a fresh install
            // that stays silent looks like a failed install, and the guide is
            // the only place the shortcut is ever explained.
            let stays_hidden = launched_at_login && config.start_minimized && config.onboarding_seen;
            if first_run || !hotkey_registered || !tray_ready || !stays_hidden {
                show_main(app.handle());
            }
            // Eager mode starts the walk now; lazy mode waits for the first
            // show, which has just happened if either reachability fallback
            // fired or this is a fresh install.
            index_service.start_initial_index();
            Ok(())
        })
        .on_window_event(|window, event| match event {
            // Close means "put it away", not "kill the launcher": the hotkey and
            // the tray are how it comes back, and quitting is an explicit tray
            // action. Without a tray icon there would be no way back, so on such
            // a desktop Close keeps its ordinary meaning and ends the process.
            tauri::WindowEvent::CloseRequested { api, .. } => {
                let has_tray = window
                    .app_handle()
                    .try_state::<TrayAvailable>()
                    .is_some_and(|tray| tray.0);
                if has_tray {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
            // A file dropped on the window is chosen by the user, with the OS as
            // the picker. Minting it here is what lets the Studio open it: the
            // webview only ever sees the path it was already told about.
            tauri::WindowEvent::DragDrop(tauri::DragDropEvent::Drop { paths, .. }) => {
                if let Some(state) = window.try_state::<AppState>() {
                    for path in paths {
                        state.paths.mint(path);
                    }
                }
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            commands::autostart::autostart_status,
            commands::autostart::autostart_set,
            commands::calc::calc_eval,
            commands::batch::batch_parse,
            commands::batch::batch_import,
            commands::batch::batch_search,
            commands::batch::batch_export,
            commands::search::search,
            commands::search::bootstrap_index,
            commands::search::index_pause,
            commands::search::index_resume,
            commands::search::index_cancel,
            commands::search::index_clear,
            commands::search::search_history_clear,
            commands::search::index_control_state,
            commands::search::candidate_count,
            commands::documents::document_index_status,
            commands::documents::document_set_indexing,
            commands::documents::document_text_store,
            commands::documents::document_text_remove,
            commands::documents::document_text_clear,
            commands::exclusions::exclusions_snapshot,
            commands::exclusions::exclusions_add,
            commands::exclusions::exclusions_update,
            commands::exclusions::exclusions_remove,
            commands::exclusions::exclusions_clear,
            commands::exclusions::exclusions_apply_preset,
            commands::exclusions::exclusions_preview,
            commands::search::record_usage,
            commands::diagnostics::diagnostics_snapshot,
            commands::onboarding::onboarding_status,
            commands::onboarding::onboarding_complete,
            commands::drives::system_drives,
            commands::update::check_for_updates,
            commands::clipboard::clipboard_list,
            commands::clipboard::clipboard_record_text,
            commands::clipboard::clipboard_poll,
            commands::clipboard::clipboard_paused,
            commands::clipboard::clipboard_set_paused,
            commands::clipboard::clipboard_delete,
            commands::clipboard::clipboard_set_pinned,
            commands::clipboard::clipboard_clear,
            commands::ocr::ocr_models_status,
            commands::ocr::ocr_pick_file,
            commands::ocr::document_extract_text,
            commands::ocr::ocr_page_preview,
            commands::ocr::ocr_process_image,
            commands::ocr::ocr_process_pdf,
            commands::ocr::ocr_process_pdf_range,
            commands::ocr::ocr_cancel,
            commands::ocr::export_docx_pages,
            commands::ocr::export_xlsx_pages,
            commands::organize::organize_pages,
            commands::organize::extract_document,
            commands::organize::extract_document_batch,
            commands::organize::preview_export,
            commands::organize::export_extraction,
            commands::organize::export_organized,
            commands::organize::export_batch,
            commands::system::open_target,
            commands::system::reveal_target,
            commands::system::open_url,
            commands::system::web_search_url,
            commands::system::db_path,
            commands::system::config_path,
            commands::system::app_version,
            commands::models::models_catalog,
            commands::models::models_advisor,
            commands::models::models_download,
            commands::models::models_cancel_download,
            commands::models::models_activate,
            commands::models::models_remove,
            commands::models::models_verify,
            commands::models::models_benchmark,
            commands::settings::settings_snapshot,
            commands::settings::settings_update,
            commands::settings::settings_pick_folder,
            commands::settings::settings_pick_file,
            commands::shortcut::hotkey_status,
            commands::shortcut::hotkey_set,
            commands::translate::translate_text,
        ])
        .build(tauri::generate_context!())?;

    app.run(|handle, event| {
        // Release the global grab as the process goes away. The OS would drop
        // it on exit anyway, but doing it here makes a restart deterministic:
        // the next instance never races a shortcut the old one still owns.
        if matches!(&event, tauri::RunEvent::Exit) {
            use tauri_plugin_global_shortcut::GlobalShortcutExt;
            let _ = handle.global_shortcut().unregister_all();
        }
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Reopen {
            has_visible_windows: false,
            ..
        } = event
        {
            // Clicking the Dock icon of an app whose window is hidden asks the
            // app to reopen; without this the launcher appears to be dead.
            show_main(handle);
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (handle, event);
        }
    });
    Ok(())
}

/// Brings the launcher up and focuses it, creating nothing and failing quietly:
/// every caller runs where there is no one left to report a failure to.
fn show_main(handle: &AppHandle) {
    let Some(window) = handle.get_webview_window("main") else {
        return;
    };
    // A minimized window that is only `show()`n stays minimized on Windows, so
    // the hotkey would appear to do nothing. Restore first, then raise.
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
    // Lazy indexing walks on first use; "use" is the window becoming visible,
    // not a timer. This is the one choke point every route goes through.
    if let Some(service) = handle.try_state::<IndexService>() {
        service.on_window_shown();
    }
}

/// Toggles the launcher: focused means hide, visible-but-unfocused means bring
/// it forward.
///
/// Hiding on every press would throw the panel away when the user reaches for it
/// from another application - which is the common case for a global launcher.
pub(crate) fn toggle_window(handle: &AppHandle) {
    let Some(window) = handle.get_webview_window("main") else {
        return;
    };
    let visible = window.is_visible().unwrap_or(false);
    let focused = window.is_focused().unwrap_or(false);
    if visible && focused {
        let _ = window.hide();
    } else {
        show_main(handle);
    }
}

/// Ends the process from the tray.
///
/// The shortcut is released before the exit request so the next launch can
/// register it even if the OS is slow to reap this process. Everything else is
/// synchronous or crash-safe (SQLite journals, no unsaved buffers), so `exit`
/// is the whole shutdown.
fn quit_app(handle: &AppHandle) {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    let _ = handle.global_shortcut().unregister_all();
    handle.exit(0);
}

fn build_tray(handle: &AppHandle) -> anyhow::Result<()> {
    let show = MenuItemBuilder::with_id("show", "Show Window").build(handle)?;
    let search = MenuItemBuilder::with_id("search", "Open Search").build(handle)?;
    let ocr = MenuItemBuilder::with_id("ocr", "OCR Studio").build(handle)?;
    let settings = MenuItemBuilder::with_id("settings", "Settings").build(handle)?;
    let quit = MenuItemBuilder::with_id("quit", "Exit").build(handle)?;
    let menu = MenuBuilder::new(handle)
        .item(&show)
        .item(&search)
        .item(&ocr)
        .item(&settings)
        .separator()
        .item(&quit)
        .build()?;

    let icon = handle
        .default_window_icon()
        .cloned()
        .ok_or_else(|| anyhow!("no default window icon"))?;

    TrayIconBuilder::with_id("main-tray")
        .icon(icon)
        .tooltip("Zeidex Search")
        .menu(&menu)
        // Windows users left-click a tray icon to open its application; the
        // menu is the right-click convention. Without this the only way in from
        // the tray is to discover the menu, which reads as a dead icon.
        .on_tray_icon_event(|tray, event| {
            if let tauri::tray::TrayIconEvent::Click {
                button: tauri::tray::MouseButton::Left,
                button_state: tauri::tray::MouseButtonState::Up,
                ..
            } = event
            {
                toggle_window(tray.app_handle());
            }
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => toggle_window(app),
            // The tray's entries are explicit: show the window and switch the
            // screen, rather than toggling it away because it happened to be
            // focused when the menu was opened.
            "search" => {
                let _ = app.emit("zeidex-mode", "launcher");
                show_main(app);
            }
            "ocr" => {
                let _ = app.emit("zeidex-mode", "ocr");
                show_main(app);
            }
            "settings" => {
                let _ = app.emit("zeidex-mode", "settings");
                show_main(app);
            }
            "quit" => quit_app(app),
            _ => {}
        })
        .build(handle)?;
    Ok(())
}
