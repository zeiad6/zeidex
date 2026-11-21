#![deny(clippy::expect_used, clippy::unwrap_used)]

pub mod commands;
pub mod error;
mod index_service;
mod index_watch;
pub mod state;

use anyhow::anyhow;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use crate::index_service::IndexService;
use crate::state::AppState;

pub fn run() -> anyhow::Result<()> {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
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
            let pdfium_library = state::resolve_pdfium_library(app.handle());
            app.manage(AppState::new(db_path, models_dir, pdfium_library)?);
            // Every index write goes through this worker: the startup walk,
            // the filesystem watcher, and the Reindex button. Managing it here,
            // before any command can run, means no caller ever sees a missing
            // state.
            let index_service = IndexService::start(app.handle().clone());
            app.manage(index_service.clone());

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

            let handle = app.handle().clone();
            let hotkey_registered = app
                .global_shortcut()
                .on_shortcut("Alt+Space", move |_app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        toggle_window(&handle);
                    }
                })
                .map_err(|err| eprintln!("zeidex: could not register Alt+Space: {err}"))
                .is_ok();

            let tray_ready = match build_tray(app.handle()) {
                Ok(()) => true,
                Err(err) => {
                    eprintln!("zeidex: tray icon not available: {err}");
                    false
                }
            };

            // Both are ways IN to a window that starts hidden. Losing the tray
            // (GNOME without AppIndicator) or the hotkey (Wayland denies the
            // grab) would otherwise leave a running process with no visible
            // surface and no obvious way to reach it. First launch is shown for
            // the same reason: a desktop entry that appears to do nothing is
            // indistinguishable from one that crashed.
            if first_run || !hotkey_registered || !tray_ready {
                show_main(app.handle());
            }
            // Eager mode starts the walk now; lazy mode waits for the first
            // show, which has just happened if either reachability fallback
            // fired or this is a fresh install.
            index_service.start_initial_index();
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::calc::calc_eval,
            commands::search::search,
            commands::search::bootstrap_index,
            commands::search::candidate_count,
            commands::search::record_usage,
            commands::clipboard::clipboard_list,
            commands::clipboard::clipboard_record_text,
            commands::clipboard::clipboard_poll,
            commands::clipboard::clipboard_paused,
            commands::clipboard::clipboard_set_paused,
            commands::clipboard::clipboard_delete,
            commands::clipboard::clipboard_set_pinned,
            commands::clipboard::clipboard_clear,
            commands::ocr::ocr_models_status,
            commands::ocr::ocr_page_preview,
            commands::ocr::ocr_process_image,
            commands::ocr::ocr_process_pdf,
            commands::ocr::ocr_process_pdf_range,
            commands::ocr::export_docx,
            commands::ocr::export_docx_pages,
            commands::ocr::export_xlsx,
            commands::ocr::export_xlsx_pages,
            commands::system::open_target,
            commands::system::reveal_target,
            commands::system::open_url,
            commands::system::web_search_url,
            commands::system::db_path,
            commands::system::config_path,
            commands::system::app_version,
            commands::settings::settings_snapshot,
            commands::settings::settings_update,
            commands::translate::translate_text,
        ])
        .build(tauri::generate_context!())?;

    app.run(|handle, event| {
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
    let _ = window.show();
    let _ = window.set_focus();
    // Lazy indexing walks on first use; "use" is the window becoming visible,
    // not a timer. This is the one choke point every route goes through.
    if let Some(service) = handle.try_state::<IndexService>() {
        service.on_window_shown();
    }
}

fn toggle_window(handle: &AppHandle) {
    let Some(window) = handle.get_webview_window("main") else {
        return;
    };
    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
    } else {
        show_main(handle);
    }
}

fn build_tray(handle: &AppHandle) -> anyhow::Result<()> {
    let show = MenuItemBuilder::with_id("show", "Show Zeidex").build(handle)?;
    let ocr = MenuItemBuilder::with_id("ocr", "OCR Studio").build(handle)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit").build(handle)?;
    let menu = MenuBuilder::new(handle)
        .item(&show)
        .item(&ocr)
        .separator()
        .item(&quit)
        .build()?;

    let icon = handle
        .default_window_icon()
        .cloned()
        .ok_or_else(|| anyhow!("no default window icon"))?;

    TrayIconBuilder::with_id("main-tray")
        .icon(icon)
        .tooltip("Zeidex Studio")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => toggle_window(app),
            "ocr" => {
                let _ = app.emit("zeidex-mode", "ocr");
                toggle_window(app);
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(handle)?;
    Ok(())
}
