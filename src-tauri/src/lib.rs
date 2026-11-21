#![deny(clippy::expect_used, clippy::unwrap_used)]

pub mod commands;
pub mod error;
pub mod state;

use anyhow::anyhow;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use crate::state::AppState;

pub fn run() -> anyhow::Result<()> {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            let db_path = state::resolve_db_path();
            state::prepare_onnx_runtime(app.handle());
            let models_dir = state::resolve_models_dir(app.handle());
            let pdfium_library = state::resolve_pdfium_library(app.handle());
            app.manage(AppState::new(db_path, models_dir, pdfium_library)?);

            let handle = app.handle().clone();
            app.global_shortcut()
                .on_shortcut("Alt+Space", move |_app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        toggle_window(&handle);
                    }
                })
                .map_err(|err| anyhow!("register Alt+Space hotkey: {err}"))?;

            build_tray(app.handle())?;
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
            commands::clipboard::clipboard_delete,
            commands::clipboard::clipboard_set_pinned,
            commands::clipboard::clipboard_clear,
            commands::ocr::ocr_models_status,
            commands::ocr::ocr_page_preview,
            commands::ocr::ocr_process_image,
            commands::ocr::ocr_process_pdf,
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
            commands::translate::translate_text,
        ])
        .run(tauri::generate_context!())
        .map_err(Into::into)
}

fn toggle_window(handle: &AppHandle) {
    let Some(window) = handle.get_webview_window("main") else {
        return;
    };
    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
    } else {
        let _ = window.show();
        let _ = window.set_focus();
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
