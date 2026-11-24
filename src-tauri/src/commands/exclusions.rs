//! Index Exclusions: the list the user maintains, its persistence, and what
//! changing it does to the index.
//!
//! The walker reads `RuntimeConfig::exclusion_set`, so a saved rule takes
//! effect on the next traversal without a restart. This module owns the other
//! half of the contract: a rule the user just added must also remove the rows
//! the previous walk already wrote, or the excluded file keeps answering
//! searches until some later full walk. Removing a rule does the opposite, and
//! only for the scope that rule covered.

use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, Manager};
use zeidex_engine::QueryEngine;
use zeidex_engine::config::{RuntimeConfig, write_exclusions};
use zeidex_engine::exclusions::{Exclusion, ExclusionKind};

use crate::index_service::IndexService;
use crate::state::AppState;

/// One preset rule. Kept as strings so the UI renders exactly what would be
/// saved; nothing is generated on the frontend.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetRule {
    pub kind: ExclusionKind,
    pub value: &'static str,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetView {
    pub id: &'static str,
    pub name: &'static str,
    pub name_ar: &'static str,
    pub description: &'static str,
    pub description_ar: &'static str,
    pub rules: Vec<PresetRule>,
    /// How many of this preset's rules are already in the list, so "Apply"
    /// never silently duplicates.
    pub already_applied: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExclusionsSnapshot {
    pub entries: Vec<Exclusion>,
    /// Enabled rules. The walker reports `0` excluded items when this is `0`.
    pub active: usize,
    pub presets: Vec<PresetView>,
    pub config_path: Option<String>,
    pub index_path: String,
}

/// The result of any mutation: the new list plus what it did to the index.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExclusionsChange {
    pub entries: Vec<Exclusion>,
    pub active: usize,
    /// Index rows deleted because the new list rejects them.
    pub removed: usize,
    /// True when a full walk was requested instead of a scoped one.
    pub reindexing: bool,
}

struct Preset {
    id: &'static str,
    name: &'static str,
    name_ar: &'static str,
    description: &'static str,
    description_ar: &'static str,
    rules: &'static [(ExclusionKind, &'static str)],
}

/// Opt-in groups. Nothing here is active until the user presses Apply, and the
/// default skip list deliberately contains none of these names: a source tree
/// the user searches for must not disappear because a preset was convenient.
const PRESETS: &[Preset] = &[
    Preset {
        id: "development",
        name: "Development files",
        name_ar: "ملفات التطوير",
        description: "Dependency and build-output folders created by npm, Cargo, Python, Gradle and friends. Your source files stay indexed.",
        description_ar: "مجلدات الاعتماديات ومخرجات البناء التي تنشئها npm وCargo وPython وGradle وغيرها. ملفاتك المصدرية تبقى مفهرسة.",
        rules: &[
            (ExclusionKind::Pattern, "node_modules"),
            (ExclusionKind::Pattern, "target"),
            (ExclusionKind::Pattern, "dist"),
            (ExclusionKind::Pattern, "build"),
            (ExclusionKind::Pattern, "venv"),
            (ExclusionKind::Pattern, "vendor"),
            (ExclusionKind::Pattern, "__pycache__"),
            (ExclusionKind::Pattern, "coverage"),
            (ExclusionKind::Pattern, "out"),
        ],
    },
    Preset {
        id: "temporary",
        name: "Temporary files",
        name_ar: "الملفات المؤقتة",
        description: "Scratch folders and editor/download leftovers. Partially written files are rarely worth a search hit.",
        description_ar: "مجلدات العمل المؤقت وبقايا المحررات والتنزيلات. الملفات نصف المكتوبة نادراً ما تستحق ظهوراً في نتائج البحث.",
        rules: &[
            (ExclusionKind::Pattern, "tmp"),
            (ExclusionKind::Pattern, "temp"),
            (ExclusionKind::Pattern, "cache"),
            (ExclusionKind::Pattern, "logs"),
            (ExclusionKind::Extension, "tmp"),
            (ExclusionKind::Extension, "log"),
            (ExclusionKind::Extension, "cache"),
            (ExclusionKind::Extension, "bak"),
            (ExclusionKind::Extension, "part"),
            (ExclusionKind::Extension, "crdownload"),
        ],
    },
    Preset {
        id: "system",
        name: "System noise",
        name_ar: "ضجيج النظام",
        description: "Windows maintenance folders that only appear if a drive root was added as a scan folder.",
        description_ar: "مجلدات صيانة Windows التي لا تظهر إلا إذا أُضيف جذر قرص كمجلد فحص.",
        rules: &[
            (ExclusionKind::Pattern, "**/$RECYCLE.BIN/**"),
            (ExclusionKind::Pattern, "**/System Volume Information/**"),
            (ExclusionKind::Pattern, "**/Windows.old/**"),
            (ExclusionKind::Pattern, "**/WinSxS/**"),
            (ExclusionKind::Pattern, "**/SoftwareDistribution/**"),
        ],
    },
];

fn config_file() -> Result<PathBuf, String> {
    zeidex_engine::config_path::current()
        .map(|resolved| resolved.path)
        .ok_or_else(|| "the Zeidex config folder is not available".to_string())
}

fn persist(entries: &[Exclusion]) -> Result<(), String> {
    let path = config_file()?;
    write_exclusions(&path, entries)?;
    // The walker holds a cached clone; without this the next traversal would
    // use the previous list and re-index what the user just excluded.
    RuntimeConfig::invalidate_cache();
    Ok(())
}

fn same_rule(left: &Exclusion, kind: ExclusionKind, value: &str) -> bool {
    left.kind == kind && left.value.eq_ignore_ascii_case(value)
}

fn active_count(entries: &[Exclusion]) -> usize {
    entries.iter().filter(|entry| entry.enabled).count()
}

fn preset_views(entries: &[Exclusion]) -> Vec<PresetView> {
    PRESETS
        .iter()
        .map(|preset| PresetView {
            id: preset.id,
            name: preset.name,
            name_ar: preset.name_ar,
            description: preset.description,
            description_ar: preset.description_ar,
            rules: preset
                .rules
                .iter()
                .map(|(kind, value)| PresetRule { kind: *kind, value })
                .collect(),
            already_applied: preset
                .rules
                .iter()
                .filter(|(kind, value)| entries.iter().any(|entry| same_rule(entry, *kind, value)))
                .count(),
        })
        .collect()
}

fn snapshot(entries: Vec<Exclusion>, state: &AppState) -> ExclusionsSnapshot {
    ExclusionsSnapshot {
        active: active_count(&entries),
        presets: preset_views(&entries),
        config_path: config_file().ok().map(|path| path.display().to_string()),
        index_path: state.db_path.display().to_string(),
        entries,
    }
}

/// Deletes the rows the new list rejects and refreshes the in-memory engine.
/// Returns the number of rows removed.
fn purge(app: &AppHandle) -> Result<usize, String> {
    let config = RuntimeConfig::load();
    let state = app.state::<AppState>();
    let removed =
        QueryEngine::purge_excluded(&state.db_path, &config).map_err(|err| err.to_string())?;
    app.state::<IndexService>().reload();
    Ok(removed)
}

/// What re-opening a scope costs. A folder or file is walked directly; an
/// extension or glob has no location, so the only honest way to find what it
/// was hiding is a full walk. The UI says which one is happening.
fn reindex_after_removal(app: &AppHandle, removed: &[Exclusion]) -> bool {
    let index = app.state::<IndexService>();
    let mut scoped: Vec<PathBuf> = Vec::new();
    let mut needs_full = false;
    for entry in removed {
        match entry.kind {
            ExclusionKind::Folder | ExclusionKind::File => {
                let path = PathBuf::from(&entry.value);
                if path.exists() {
                    scoped.push(path);
                }
            }
            ExclusionKind::Extension | ExclusionKind::Pattern => needs_full = true,
        }
    }
    if needs_full {
        index.index_settings_changed();
        return true;
    }
    index.reindex_paths(scoped);
    false
}

#[tauri::command(async)]
pub fn exclusions_snapshot(app: AppHandle) -> Result<ExclusionsSnapshot, String> {
    let state = app.state::<AppState>();
    let config = RuntimeConfig::load();
    Ok(snapshot(config.exclusions, &state))
}

#[tauri::command(async)]
pub fn exclusions_add(
    app: AppHandle,
    kind: ExclusionKind,
    value: String,
) -> Result<ExclusionsChange, String> {
    let value = Exclusion::validate(kind, &value)?;
    let mut entries = RuntimeConfig::load().exclusions;
    if entries.iter().any(|entry| same_rule(entry, kind, &value)) {
        return Err("that rule is already in the list".to_string());
    }
    entries.push(Exclusion::new(
        Exclusion::new_id(),
        kind,
        value,
        Exclusion::now(),
    ));
    persist(&entries)?;
    let removed = purge(&app)?;
    Ok(ExclusionsChange {
        active: active_count(&entries),
        entries,
        removed,
        reindexing: false,
    })
}

#[tauri::command(async)]
pub fn exclusions_update(
    app: AppHandle,
    id: String,
    value: Option<String>,
    enabled: Option<bool>,
) -> Result<ExclusionsChange, String> {
    let mut entries = RuntimeConfig::load().exclusions;
    let index = entries
        .iter()
        .position(|entry| entry.id == id)
        .ok_or_else(|| "that exclusion no longer exists".to_string())?;
    let previous = entries[index].clone();

    if let Some(value) = value {
        let normalized = Exclusion::validate(previous.kind, &value)?;
        if entries.iter().enumerate().any(|(position, entry)| {
            position != index && same_rule(entry, previous.kind, &normalized)
        }) {
            return Err("that rule is already in the list".to_string());
        }
        entries[index].value = normalized;
    }
    if let Some(enabled) = enabled {
        entries[index].enabled = enabled;
    }
    persist(&entries)?;

    // Enabling or widening a rule hides rows: purge. Disabling or narrowing it
    // reveals them: walk the scope that just came back.
    let was_active = previous.enabled;
    let is_active = entries[index].enabled;
    let value_changed = entries[index].value != previous.value;
    let mut removed = 0usize;
    let mut reindexing = false;
    if is_active && (!was_active || value_changed) {
        removed = purge(&app)?;
    }
    if was_active && (!is_active || value_changed) {
        reindexing = reindex_after_removal(&app, &[previous]);
    }

    Ok(ExclusionsChange {
        active: active_count(&entries),
        entries,
        removed,
        reindexing,
    })
}

#[tauri::command(async)]
pub fn exclusions_remove(app: AppHandle, id: String) -> Result<ExclusionsChange, String> {
    let mut entries = RuntimeConfig::load().exclusions;
    let index = entries
        .iter()
        .position(|entry| entry.id == id)
        .ok_or_else(|| "that exclusion no longer exists".to_string())?;
    let removed_rule = entries.remove(index);
    persist(&entries)?;
    let reindexing = if removed_rule.enabled {
        reindex_after_removal(&app, &[removed_rule])
    } else {
        false
    };
    Ok(ExclusionsChange {
        active: active_count(&entries),
        entries,
        removed: 0,
        reindexing,
    })
}

#[tauri::command(async)]
pub fn exclusions_clear(app: AppHandle) -> Result<ExclusionsChange, String> {
    let entries: Vec<Exclusion> = Vec::new();
    persist(&entries)?;
    // Every scope re-opens; only a full walk can find what was hidden.
    app.state::<IndexService>().index_settings_changed();
    Ok(ExclusionsChange {
        entries,
        active: 0,
        removed: 0,
        reindexing: true,
    })
}

#[tauri::command(async)]
pub fn exclusions_apply_preset(app: AppHandle, preset: String) -> Result<ExclusionsChange, String> {
    let chosen = PRESETS
        .iter()
        .find(|candidate| candidate.id == preset)
        .ok_or_else(|| format!("unknown preset: {preset}"))?;
    let mut entries = RuntimeConfig::load().exclusions;
    let mut added = 0usize;
    for (kind, value) in chosen.rules {
        if entries.iter().any(|entry| same_rule(entry, *kind, value)) {
            continue;
        }
        entries.push(Exclusion::new(
            Exclusion::new_id(),
            *kind,
            *value,
            Exclusion::now(),
        ));
        added += 1;
    }
    if added == 0 {
        return Ok(ExclusionsChange {
            active: active_count(&entries),
            entries,
            removed: 0,
            reindexing: false,
        });
    }
    persist(&entries)?;
    let removed = purge(&app)?;
    Ok(ExclusionsChange {
        active: active_count(&entries),
        entries,
        removed,
        reindexing: false,
    })
}

/// How many indexed rows one candidate rule would reject. Shown before the
/// user commits, so a folder exclusion has a number instead of a promise.
#[tauri::command(async)]
pub fn exclusions_preview(
    app: AppHandle,
    kind: ExclusionKind,
    value: String,
) -> Result<usize, String> {
    let value = Exclusion::validate(kind, &value)?;
    let rule = Exclusion::new("preview", kind, value, 0);
    let state = app.state::<AppState>();
    QueryEngine::count_matching(&state.db_path, &[rule])
        .ok_or_else(|| "the search index could not be read".to_string())
}

#[cfg(test)]
mod tests {
    use super::{PRESETS, active_count, preset_views, same_rule};
    use zeidex_engine::exclusions::{Exclusion, ExclusionKind};

    #[test]
    fn every_preset_id_is_unique_and_rules_are_non_empty() {
        let mut ids: Vec<&str> = PRESETS.iter().map(|preset| preset.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), PRESETS.len());
        for preset in PRESETS {
            assert!(!preset.rules.is_empty());
            for (kind, value) in preset.rules {
                assert!(!value.is_empty());
                Exclusion::validate(*kind, value).expect("a preset rule must validate");
            }
        }
    }

    #[test]
    fn preset_views_count_the_rules_already_present() {
        let entries = vec![Exclusion::new(
            "id",
            ExclusionKind::Pattern,
            "node_modules",
            0,
        )];
        let views = preset_views(&entries);
        let development = views
            .iter()
            .find(|view| view.id == "development")
            .expect("development preset");
        assert_eq!(development.already_applied, 1);
    }

    #[test]
    fn rule_comparison_ignores_case_but_not_kind() {
        let entry = Exclusion::new("id", ExclusionKind::Extension, "TMP", 0);
        assert!(same_rule(&entry, ExclusionKind::Extension, "tmp"));
        assert!(!same_rule(&entry, ExclusionKind::Folder, "tmp"));
    }

    #[test]
    fn the_active_count_skips_disabled_rules() {
        let mut disabled = Exclusion::new("a", ExclusionKind::Pattern, "tmp", 0);
        disabled.enabled = false;
        let entries = vec![
            disabled,
            Exclusion::new("b", ExclusionKind::Pattern, "cache", 0),
        ];
        assert_eq!(active_count(&entries), 1);
    }
}
