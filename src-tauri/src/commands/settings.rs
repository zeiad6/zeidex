//! The Settings Center's backend.
//!
//! Two storage locations meet here, and the split is deliberate:
//!
//! * `~/.zeidex/config` holds the values the engine reads at startup (scan
//!   depth, history limit, retention, …). It is hand-editable, so writes are
//!   surgical: `config::write_settings` rewrites only the lines it owns.
//! * The `settings` table holds values that already had a runtime path (web
//!   search). Those are applied through the store, no file involved.
//!
//! Every range is enforced on this side. The UI mirrors it for inline errors,
//! but the check that matters is `config::canonical_setting`, which uses the
//! same constants as the parser.

use std::path::Path;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use zeidex_engine::config::{self, RuntimeConfig, SettingValue};
use zeidex_storage::{SearchEngine, SearchSettings};

use super::dialogs;
use crate::index_service::IndexService;
use crate::state::AppState;

/// Everything the Settings Center renders, in one round trip. Ranges travel
/// with the values so the UI never invents its own bounds.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSnapshot {
    pub version: String,
    pub platform: &'static str,
    /// `None` when the home directory cannot be resolved: saving is then
    /// impossible, and the UI says so instead of offering a dead button.
    pub config_path: Option<String>,
    pub db_path: String,
    pub candidate_count: usize,
    pub file_scan_depth: u32,
    pub file_scan_limit: u32,
    pub file_scan_extra_roots: Vec<String>,
    pub lazy_indexing_enabled: bool,
    pub localized_app_names: bool,
    pub clipboard_history_limit: u32,
    pub clipboard_filter_secrets: bool,
    pub clipboard_retention_days: u32,
    pub clipboard_paused: bool,
    pub web_search_enabled: bool,
    pub web_search_engine: String,
    pub search_engines: Vec<&'static str>,
    pub limits: SettingLimits,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingLimits {
    pub file_scan_depth: (u32, u32),
    pub file_scan_limit: (u32, u32),
    pub clipboard_history_limit: (u32, u32),
    pub clipboard_retention_days: (u32, u32),
}

/// Only the fields the user actually changed. `deny_unknown_fields` turns a
/// frontend typo into a loud error instead of a field that is silently ignored.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SettingsPatch {
    pub file_scan_depth: Option<u32>,
    pub file_scan_limit: Option<u32>,
    pub file_scan_extra_roots: Option<Vec<String>>,
    pub lazy_indexing_enabled: Option<bool>,
    pub localized_app_names: Option<bool>,
    pub clipboard_history_limit: Option<u32>,
    pub clipboard_filter_secrets: Option<bool>,
    pub clipboard_retention_days: Option<u32>,
    pub web_search: Option<WebSearchPatch>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WebSearchPatch {
    pub enabled: bool,
    pub engine: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsUpdate {
    pub settings: SettingsSnapshot,
    /// True when the patch changed something the indexer reads: the new values
    /// are saved, but existing rows were built with the old ones.
    pub reindex_required: bool,
    /// A live effect that failed after the value was saved. The file is the
    /// record, so this is a "takes effect on restart" notice, not a failure.
    pub warning: Option<String>,
}

#[tauri::command(async)]
pub fn settings_snapshot(state: State<'_, AppState>) -> Result<SettingsSnapshot, String> {
    // A fresh read, not the process cache: if the user edited the file by hand,
    // showing the cached value and then saving it back would undo that edit.
    let resolved = zeidex_engine::config_path::current();
    build_snapshot(
        &state,
        &RuntimeConfig::load(),
        resolved.as_ref().map(|path| path.path.as_path()),
    )
}

/// Asks the user for a folder to add as a scan root.
///
/// The dialog runs in Rust for the same reason every other picker does: the
/// page asks for a folder, the user chooses one, and the path becomes readable
/// only because they did. Returns `None` when the dialog was dismissed.
#[tauri::command(async)]
pub fn settings_pick_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    title: Option<String>,
) -> Result<Option<String>, String> {
    let picked = dialogs::pick_folder(&app, &state.paths, title)?;
    Ok(picked.map(|path| path.display().to_string()))
}

#[tauri::command(async)]
pub fn settings_update(
    state: State<'_, AppState>,
    index: State<'_, IndexService>,
    patch: SettingsPatch,
) -> Result<SettingsUpdate, String> {
    let resolved = zeidex_engine::config_path::current();
    let update = apply_patch(
        &state,
        &patch,
        resolved.as_ref().map(|path| path.path.as_path()),
    )?;
    // The value is saved either way; a changed root or depth cannot be applied
    // by rewriting rows, so the shell rebuilds the index now instead of asking
    // the user to press a second button. The event carries the new count.
    if update.reindex_required {
        index.index_settings_changed();
    }
    Ok(update)
}

/// The apply path, free of Tauri state so it can be tested against a temporary
/// config file and database.
fn apply_patch(
    state: &AppState,
    patch: &SettingsPatch,
    config_path: Option<&Path>,
) -> Result<SettingsUpdate, String> {
    // Validate everything before the first write: a rejected engine must not
    // leave half of the patch on disk.
    let changes = config_changes(patch)?;
    let search = validated_web_search(patch)?;
    let reindex_required = affects_index(patch);

    let config = match config_path {
        Some(path) => {
            if !changes.is_empty() {
                config::write_settings(path, &changes)?;
                // The running process reads the file through a cache; without
                // this the next clipboard poll would keep the previous limit.
                RuntimeConfig::invalidate_cache();
            }
            RuntimeConfig::load_from(path)
        }
        None => {
            if !changes.is_empty() {
                return Err(
                    "Cannot save settings: the home directory could not be resolved.".to_string(),
                );
            }
            RuntimeConfig::load()
        }
    };

    // A saved root is a read capability: apply the file's values, not the
    // patch's, so a root the parser rejected grants nothing.
    state.paths.set_roots(&config.file_scan_extra_roots);

    let mut warning = None;
    {
        let mut store = state
            .store
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        // Apply the values the parser read back, not the numbers in the patch:
        // what runs is then guaranteed to be what a restart would load.
        if patch.clipboard_history_limit.is_some()
            && let Err(err) = store.set_clipboard_text_cap(config.clipboard_history_limit)
        {
            warning = Some(format!(
                "The history limit was saved but could not be applied now: {err}"
            ));
        }
        if patch.clipboard_retention_days.is_some()
            && let Err(err) = store.set_clipboard_retention_days(config.clipboard_retention_days)
        {
            warning.get_or_insert_with(|| {
                format!("The retention limit was saved but could not be applied now: {err}")
            });
        }
        if patch.clipboard_filter_secrets.is_some() {
            store.set_clipboard_filter_secrets(config.clipboard_filter_secrets);
        }
        if let Some(search) = search {
            store
                .save_search_settings(search)
                .map_err(|err| err.to_string())?;
        }
    }

    Ok(SettingsUpdate {
        settings: build_snapshot(state, &config, config_path)?,
        reindex_required,
        warning,
    })
}

fn build_snapshot(
    state: &AppState,
    config: &RuntimeConfig,
    config_path: Option<&Path>,
) -> Result<SettingsSnapshot, String> {
    let (clipboard_paused, search_settings) = {
        let store = state
            .store
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        (
            store.clipboard_paused().map_err(|err| err.to_string())?,
            store
                .load_search_settings()
                .map_err(|err| err.to_string())?,
        )
    };
    let candidate_count = {
        let engine = state
            .engine
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        engine.candidate_count()
    };

    Ok(SettingsSnapshot {
        version: env!("CARGO_PKG_VERSION").to_string(),
        platform: std::env::consts::OS,
        config_path: config_path.map(|path| path.to_string_lossy().into_owned()),
        db_path: state.db_path.to_string_lossy().into_owned(),
        candidate_count,
        file_scan_depth: config.file_scan_depth as u32,
        file_scan_limit: config.file_scan_limit as u32,
        file_scan_extra_roots: config.file_scan_extra_roots.clone(),
        lazy_indexing_enabled: config.lazy_indexing_enabled,
        localized_app_names: config.localized_app_names,
        clipboard_history_limit: config.clipboard_history_limit as u32,
        clipboard_filter_secrets: config.clipboard_filter_secrets,
        clipboard_retention_days: config.clipboard_retention_days,
        clipboard_paused,
        web_search_enabled: search_settings.web_search_enabled,
        web_search_engine: search_settings.web_search_engine.key().to_string(),
        search_engines: SearchEngine::all()
            .iter()
            .map(|engine| engine.key())
            .collect(),
        limits: SettingLimits {
            file_scan_depth: bounds("file_scan_depth"),
            file_scan_limit: bounds("file_scan_limit"),
            clipboard_history_limit: bounds("clipboard_history_limit"),
            clipboard_retention_days: bounds("clipboard_retention_days"),
        },
    })
}

/// `(0, 0)` for a key that has no range: the UI then refuses every value rather
/// than sending one unchecked. These four keys are compiled in, so this branch
/// is unreachable in practice and loud if it ever is not.
fn bounds(key: &str) -> (u32, u32) {
    config::setting_bounds(key).unwrap_or_default()
}

fn config_changes(patch: &SettingsPatch) -> Result<Vec<(String, String)>, String> {
    let mut changes: Vec<(String, String)> = Vec::new();
    let mut push = |key: &str, value: SettingValue<'_>| -> Result<(), String> {
        changes.push((key.to_string(), config::canonical_setting(key, value)?));
        Ok(())
    };

    if let Some(value) = patch.file_scan_depth {
        push("file_scan_depth", SettingValue::Integer(value))?;
    }
    if let Some(value) = patch.file_scan_limit {
        push("file_scan_limit", SettingValue::Integer(value))?;
    }
    if let Some(roots) = patch.file_scan_extra_roots.as_deref() {
        push("file_scan_extra_roots", SettingValue::PathList(roots))?;
    }
    if let Some(value) = patch.lazy_indexing_enabled {
        push("lazy_indexing_enabled", SettingValue::Bool(value))?;
    }
    if let Some(value) = patch.localized_app_names {
        push("localized_app_names", SettingValue::Bool(value))?;
    }
    if let Some(value) = patch.clipboard_history_limit {
        push("clipboard_history_limit", SettingValue::Integer(value))?;
    }
    if let Some(value) = patch.clipboard_filter_secrets {
        push("clipboard_filter_secrets", SettingValue::Bool(value))?;
    }
    if let Some(value) = patch.clipboard_retention_days {
        push("clipboard_retention_days", SettingValue::Integer(value))?;
    }
    Ok(changes)
}

fn validated_web_search(patch: &SettingsPatch) -> Result<Option<SearchSettings>, String> {
    let Some(search) = &patch.web_search else {
        return Ok(None);
    };
    // Strict on purpose: `SearchEngine::from_key` falls back to Google so an
    // unreadable stored value still boots, but a value the user just chose must
    // be saved as chosen or refused.
    let engine = SearchEngine::from_key_checked(&search.engine)
        .ok_or_else(|| format!("Unknown search engine: {}", search.engine))?;
    Ok(Some(SearchSettings {
        web_search_enabled: search.enabled,
        web_search_engine: engine,
    }))
}

fn affects_index(patch: &SettingsPatch) -> bool {
    patch.file_scan_depth.is_some()
        || patch.file_scan_limit.is_some()
        || patch.file_scan_extra_roots.is_some()
        || patch.lazy_indexing_enabled.is_some()
        || patch.localized_app_names.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_changes_validates_values_before_anything_is_written() {
        let too_deep = SettingsPatch {
            file_scan_depth: Some(99),
            ..SettingsPatch::default()
        };
        assert!(config_changes(&too_deep).is_err());

        let negative_is_a_parse_error: Result<SettingsPatch, _> =
            serde_json::from_str(r#"{"fileScanDepth":-1}"#);
        assert!(negative_is_a_parse_error.is_err());

        let unknown_field: Result<SettingsPatch, _> = serde_json::from_str(r#"{"nope":1}"#);
        assert!(unknown_field.is_err());
    }

    #[test]
    fn config_changes_covers_every_writable_field() {
        let patch = SettingsPatch {
            file_scan_depth: Some(6),
            file_scan_limit: Some(12_000),
            file_scan_extra_roots: Some(vec!["/tmp/zeidex-extra".to_string()]),
            lazy_indexing_enabled: Some(false),
            localized_app_names: Some(true),
            clipboard_history_limit: Some(250),
            clipboard_filter_secrets: Some(false),
            clipboard_retention_days: Some(30),
            web_search: None,
        };
        let changes = config_changes(&patch).expect("valid patch");
        let keys: Vec<&str> = changes.iter().map(|(key, _)| key.as_str()).collect();
        assert_eq!(
            keys,
            vec![
                "file_scan_depth",
                "file_scan_limit",
                "file_scan_extra_roots",
                "lazy_indexing_enabled",
                "localized_app_names",
                "clipboard_history_limit",
                "clipboard_filter_secrets",
                "clipboard_retention_days",
            ]
        );
        assert_eq!(
            changes
                .iter()
                .find(|(key, _)| key == "file_scan_extra_roots")
                .map(|(_, value)| value.as_str()),
            Some("/tmp/zeidex-extra")
        );
    }

    #[test]
    fn web_search_rejects_an_unknown_engine() {
        let patch = SettingsPatch {
            web_search: Some(WebSearchPatch {
                enabled: true,
                engine: "gogle".to_string(),
            }),
            ..SettingsPatch::default()
        };
        assert!(validated_web_search(&patch).is_err());

        let patch = SettingsPatch {
            web_search: Some(WebSearchPatch {
                enabled: false,
                engine: "duckduckgo".to_string(),
            }),
            ..SettingsPatch::default()
        };
        let search = validated_web_search(&patch)
            .expect("known engine")
            .expect("present");
        assert!(!search.web_search_enabled);
        assert_eq!(search.web_search_engine, SearchEngine::DuckDuckGo);
    }

    #[test]
    fn only_index_settings_ask_for_a_reindex() {
        let clipboard = SettingsPatch {
            clipboard_history_limit: Some(50),
            ..SettingsPatch::default()
        };
        assert!(!affects_index(&clipboard));

        let depth = SettingsPatch {
            file_scan_depth: Some(5),
            ..SettingsPatch::default()
        };
        assert!(affects_index(&depth));

        let roots = SettingsPatch {
            file_scan_extra_roots: Some(Vec::new()),
            ..SettingsPatch::default()
        };
        assert!(affects_index(&roots));
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_directory(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "zeidex-settings-apply-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time should be after epoch")
                .as_nanos()
        ))
    }

    #[test]
    fn applying_a_patch_writes_the_file_then_reads_it_back() {
        let directory = temp_directory("apply");
        std::fs::create_dir_all(&directory).expect("temp dir");
        let config_path = directory.join("config");
        std::fs::write(&config_path, "# hand-written comment\nfile_scan_depth=4\n")
            .expect("seed config");
        let state = AppState::new(
            directory.join("zeidex.db"),
            directory.join("models"),
            directory.join("packs"),
            None,
        )
        .expect("app state");

        let update = apply_patch(
            &state,
            &SettingsPatch {
                file_scan_depth: Some(7),
                clipboard_history_limit: Some(50),
                clipboard_filter_secrets: Some(false),
                web_search: Some(WebSearchPatch {
                    enabled: false,
                    engine: "duckduckgo".to_string(),
                }),
                ..SettingsPatch::default()
            },
            Some(&config_path),
        )
        .expect("apply patch");

        // The snapshot the UI receives must be the values that were just saved.
        assert_eq!(update.settings.file_scan_depth, 7);
        assert_eq!(update.settings.clipboard_history_limit, 50);
        assert!(!update.settings.clipboard_filter_secrets);
        assert!(!update.settings.web_search_enabled);
        assert_eq!(update.settings.web_search_engine, "duckduckgo");
        assert!(update.reindex_required);
        assert!(update.warning.is_none());

        let written = std::fs::read_to_string(&config_path).expect("config");
        assert!(written.contains("# hand-written comment"));
        assert!(written.contains("file_scan_depth=7"));
        assert!(written.contains("clipboard_history_limit=50"));
        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn snapshot_json_matches_the_frontend_contract() {
        let directory = temp_directory("contract");
        std::fs::create_dir_all(&directory).expect("temp dir");
        let state = AppState::new(
            directory.join("zeidex.db"),
            directory.join("models"),
            directory.join("packs"),
            None,
        )
        .expect("app state");
        let snapshot = build_snapshot(
            &state,
            &RuntimeConfig::default(),
            Some(&directory.join("config")),
        )
        .expect("snapshot");
        let json = serde_json::to_value(&snapshot).expect("json");

        // The TypeScript `AppSettings` interface is these keys, camelCased. A
        // rename here without a rename there is a silently undefined field.
        for key in [
            "version",
            "platform",
            "configPath",
            "dbPath",
            "candidateCount",
            "fileScanDepth",
            "fileScanLimit",
            "fileScanExtraRoots",
            "lazyIndexingEnabled",
            "localizedAppNames",
            "clipboardHistoryLimit",
            "clipboardFilterSecrets",
            "clipboardRetentionDays",
            "clipboardPaused",
            "webSearchEnabled",
            "webSearchEngine",
            "searchEngines",
            "limits",
        ] {
            assert!(json.get(key).is_some(), "snapshot is missing {key}");
        }
        let limits = json
            .get("limits")
            .and_then(|value| value.as_object())
            .expect("limits");
        for key in [
            "fileScanDepth",
            "fileScanLimit",
            "clipboardHistoryLimit",
            "clipboardRetentionDays",
        ] {
            assert!(limits.get(key).is_some(), "limits is missing {key}");
        }
        assert_eq!(json["limits"]["fileScanDepth"], serde_json::json!([1, 12]));
        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn an_invalid_patch_leaves_the_config_untouched() {
        let directory = temp_directory("reject");
        std::fs::create_dir_all(&directory).expect("temp dir");
        let config_path = directory.join("config");
        let original = "# untouched\nfile_scan_depth=4\n";
        std::fs::write(&config_path, original).expect("seed config");
        let state = AppState::new(
            directory.join("zeidex.db"),
            directory.join("models"),
            directory.join("packs"),
            None,
        )
        .expect("app state");

        let result = apply_patch(
            &state,
            &SettingsPatch {
                file_scan_depth: Some(99),
                ..SettingsPatch::default()
            },
            Some(&config_path),
        );
        assert!(result.is_err());
        assert_eq!(
            std::fs::read_to_string(&config_path).expect("config"),
            original,
            "a rejected patch must not leave half a write behind"
        );
        std::fs::remove_dir_all(&directory).ok();
    }
}
