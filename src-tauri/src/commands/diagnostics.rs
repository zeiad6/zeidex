//! About & Diagnostics: the facts a user (or a support conversation) needs, read
//! from the running process rather than from a version string in the UI.
//!
//! Everything here is local and read-only. It exists so "which build is this,
//! where is my index, how big is it, what is it scanning" has an answer inside
//! the app instead of a guess. Nothing is uploaded: the About screen shows it,
//! the user copies it if they choose to.

use serde::Serialize;
use tauri::State;

use crate::state::AppState;
use zeidex_engine::config::RuntimeConfig;

/// The public project facts. Constants, not settings: a user cannot edit the
/// publisher of the build they are running.
const DEVELOPER: &str = "Zeidexsoft";
/// The person behind the project. A user who needs to reach a human should not
/// have to infer a handle from a repository URL.
const MAINTAINER: &str = "@zeiad6";
const WEBSITE: &str = "https://zeidexsoft.com";
const REPOSITORY: &str = "https://github.com/zeiad6/zeidex-search";
/// The project's own developer card, linked from the README.
const DEVELOPER_CARD: &str = "https://zeidexlink.zeidexsoft.com/zeidex";
const LICENSE: &str = "GPL-3.0-or-later";
const COPYRIGHT: &str = "Copyright © 2026 Zeidexsoft";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppFacts {
    pub name: &'static str,
    pub version: &'static str,
    /// `release` or `debug`. A debug build points autostart at `target/debug`,
    /// which is why the Startup screen warns about it.
    pub build: &'static str,
    pub tauri: &'static str,
    pub developer: &'static str,
    pub maintainer: &'static str,
    pub website: &'static str,
    pub repository: &'static str,
    pub developer_card: &'static str,
    pub license: &'static str,
    pub copyright: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformFacts {
    pub os: String,
    pub arch: &'static str,
    pub family: &'static str,
    pub executable: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathFacts {
    pub config: Option<String>,
    pub database: String,
    pub models_bundled: String,
    pub models_user: String,
    pub onnx_runtime: Option<String>,
    pub pdfium: Option<String>,
    pub startup_log: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexFacts {
    pub candidates: usize,
    pub documents: usize,
    pub database_bytes: u64,
    pub scan_roots: Vec<String>,
    pub exclusion_rules: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsSnapshot {
    pub app: AppFacts,
    pub platform: PlatformFacts,
    pub paths: PathFacts,
    pub index: IndexFacts,
}

#[tauri::command(async)]
pub fn diagnostics_snapshot(state: State<'_, AppState>) -> DiagnosticsSnapshot {
    let config = RuntimeConfig::load_cached();

    let candidates = state
        .engine
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .candidate_count();
    let documents = {
        let store = state
            .store
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        store.document_text_count().unwrap_or(0)
    };
    let database_bytes = std::fs::metadata(&state.db_path)
        .map(|meta| meta.len())
        .unwrap_or(0);

    let startup_log = state
        .db_path
        .parent()
        .map(|dir| dir.join("zeidex-startup.log"))
        .filter(|path| path.is_file())
        .map(|path| path.display().to_string());

    DiagnosticsSnapshot {
        app: AppFacts {
            name: "Zeidex Search",
            version: env!("CARGO_PKG_VERSION"),
            build: if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            },
            tauri: tauri::VERSION,
            developer: DEVELOPER,
            maintainer: MAINTAINER,
            website: WEBSITE,
            repository: REPOSITORY,
            developer_card: DEVELOPER_CARD,
            license: LICENSE,
            copyright: COPYRIGHT,
        },
        platform: PlatformFacts {
            // Both `long_os_version` and `name` are `Result`s in sysinfo 0.40,
            // so the fallback chain stays in `Result` and ends in `std::env`.
            os: sysinfo::System::long_os_version()
                .or_else(|_| sysinfo::System::name())
                .unwrap_or_else(|_| std::env::consts::OS.to_string()),
            arch: std::env::consts::ARCH,
            family: std::env::consts::FAMILY,
            executable: std::env::current_exe()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
        },
        paths: PathFacts {
            config: zeidex_engine::config_path::current()
                .map(|resolved| resolved.path.display().to_string()),
            database: state.db_path.display().to_string(),
            models_bundled: state.models_dir.display().to_string(),
            models_user: state.models_root.display().to_string(),
            onnx_runtime: std::env::var("ORT_DYLIB_PATH")
                .ok()
                .filter(|value| !value.trim().is_empty()),
            pdfium: state
                .pdfium_library
                .as_ref()
                .map(|path| path.display().to_string()),
            startup_log,
        },
        index: IndexFacts {
            candidates,
            documents,
            database_bytes,
            scan_roots: config.file_scan_root_list(),
            exclusion_rules: config
                .exclusions
                .iter()
                .filter(|entry| entry.enabled)
                .count(),
        },
    }
}
