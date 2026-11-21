mod apps;
mod settings_catalog;
#[cfg(test)]
mod test_support;

use crate::platform::SettingsCatalogEntry;
use std::collections::HashMap;
use std::env;
use std::path::Path;

use plist::Value;

pub(crate) const APP_SCAN_ROOTS: &[&str] = &[
    "/Applications",
    "/System/Applications",
    "/System/Applications/Utilities",
    "/System/Library/CoreServices/Applications",
    "/System/Library/CoreServices/Finder.app/Contents/Applications",
];

pub(crate) const REQUIRED_APP_SCAN_ROOTS: &[&str] = &[
    "/System/Library/CoreServices/Applications",
    "/System/Library/CoreServices/Finder.app/Contents/Applications",
];

pub(crate) const FILE_SCAN_ROOT_SUFFIXES: &[&str] = &["Desktop", "Documents", "Downloads"];

pub(crate) const SETTINGS_URL_SCHEME_PREFIX: &str = "x-apple.systempreferences:";
pub(crate) const SETTINGS_SUBTITLE_PREFIX: &str = "System Settings ";
pub(crate) const APP_BUNDLE_EXTENSION: &str = ".app";
const SETTINGS_BUNDLE_EXTENSION: &str = ".appex";
const INFO_PLIST_FILE: &str = "Contents/Info.plist";
const BUNDLE_RESOURCES_DIR: &str = "Contents/Resources";
const INFO_PLIST_STRINGS_FILE: &str = "InfoPlist.strings";
const DISPLAY_NAME_KEYS: [&str; 2] = ["CFBundleDisplayName", "CFBundleName"];
const BUNDLE_ID_KEY: &str = "CFBundleIdentifier";
/// The user's language list lives here. `NSLocale.preferredLanguages` is the
/// Foundation equivalent; reading the plist keeps the whole module free of
/// Objective-C runtime linkage.
const GLOBAL_PREFERENCES_PLIST: &str = "Library/Preferences/.GlobalPreferences.plist";
const APPLE_LANGUAGES_KEY: &str = "AppleLanguages";

/// Reading a bundle's name costs one Info.plist read plus a few `.lproj`
/// lookups, which dominates both scans and is independent per bundle, so the
/// reads are spread over a small pool. Measured cold on 92 apps: ~69ms on one
/// thread, ~23ms on four. Capped well under the core count because indexing is
/// background work.
const MAX_BUNDLE_READ_THREADS: usize = 4;
/// Below this many bundles per thread, spawning costs more than it saves.
const MIN_BUNDLES_PER_READ_THREAD: usize = 8;

pub(crate) use apps::discover_installed_apps;
pub(crate) use settings_catalog::SETTINGS_CATALOG;

/// Settings panes ship as `.appex` bundles here on macOS 13+. On macOS 12 and
/// earlier they are `.prefPane` bundles in `/System/Library/PreferencePanes/`,
/// which this never reads.
const SETTINGS_EXTENSIONS_DIR: &str = "/System/Library/ExtensionKit/Extensions";

pub(crate) fn additional_app_scan_roots() -> Vec<String> {
    env::var("HOME")
        .ok()
        .filter(|home| !home.trim().is_empty())
        .map(|home| vec![format!("{home}/Applications")])
        .unwrap_or_default()
}

/// Bundle names localized for the user's preferred languages, one entry per
/// input path and in the same order. `None` means the bundle has no usable
/// name and the caller should keep whatever it already had.
pub(crate) fn read_localized_display_names(
    paths: &[String],
    strip_suffix: &str,
) -> Vec<Option<String>> {
    let languages = preferred_languages();
    read_localized_display_names_with(&languages, paths, strip_suffix)
}

/// The testable core of [`read_localized_display_names`]: the language list is
/// supplied, so a fixture does not depend on the machine's Locale settings.
fn read_localized_display_names_with(
    languages: &[String],
    paths: &[String],
    strip_suffix: &str,
) -> Vec<Option<String>> {
    read_bundles(paths, |path| {
        read_bundle_name(path, strip_suffix, languages)
    })
}

/// Loads each path's bundle metadata and applies `read`, spreading the work
/// over a small thread pool. Returns one entry per input path, in input order.
fn read_bundles<T, F>(paths: &[String], read: F) -> Vec<Option<T>>
where
    T: Send,
    F: Fn(&str) -> Option<T> + Sync,
{
    let read_chunk =
        |chunk: &[String]| -> Vec<Option<T>> { chunk.iter().map(|path| read(path)).collect() };

    let threads = bundle_read_threads(paths.len());
    if threads == 1 {
        return read_chunk(paths);
    }

    let mut names = Vec::with_capacity(paths.len());
    std::thread::scope(|scope| {
        let workers: Vec<_> = paths
            .chunks(paths.len().div_ceil(threads))
            .map(|chunk| {
                let read_chunk = &read_chunk;
                scope.spawn(move || read_chunk(chunk))
            })
            .collect();
        for worker in workers {
            let Ok(chunk_names) = worker.join() else {
                eprintln!("zeidex index: bundle metadata worker panicked");
                return;
            };
            names.extend(chunk_names);
        }
    });

    // Callers zip this against `paths`, so a short result would shift every
    // name onto the wrong bundle. A panicked worker costs one serial retry.
    if names.len() == paths.len() {
        names
    } else {
        read_chunk(paths)
    }
}

fn bundle_read_threads(bundle_count: usize) -> usize {
    let cores = std::thread::available_parallelism()
        .map(|cores| cores.get())
        .unwrap_or(1);
    bundle_count
        .div_ceil(MIN_BUNDLES_PER_READ_THREAD)
        .min(cores)
        .clamp(1, MAX_BUNDLE_READ_THREADS)
}

/// Maps every settings catalog target that ships a matching `.appex` bundle to
/// its localized pane name, keyed by `SettingsCatalogEntry::target`. Targets
/// whose localized name equals the catalog title (or the bundle's own file
/// name) are left out, so callers can treat a hit as "this differs from the
/// English title".
///
/// The whole scan lives here rather than in `index::settings` so bundle
/// metadata parsing stays behind the platform boundary, and so each bundle is
/// loaded once instead of once per property read.
pub(crate) fn localized_settings_titles(
    catalog: &'static [SettingsCatalogEntry],
) -> HashMap<&'static str, String> {
    let languages = preferred_languages();
    localized_settings_titles_with(catalog, SETTINGS_EXTENSIONS_DIR, &languages)
}

/// Testable core of [`localized_settings_titles`]: the language list is
/// supplied so a fixture does not depend on the machine's Locale settings.
fn localized_settings_titles_with(
    catalog: &'static [SettingsCatalogEntry],
    extensions_dir: &str,
    languages: &[String],
) -> HashMap<&'static str, String> {
    let Ok(entries) = std::fs::read_dir(extensions_dir) else {
        return HashMap::new();
    };

    let paths: Vec<String> = entries
        .flatten()
        .filter_map(|entry| entry.path().to_str().map(str::to_string))
        .filter(|path| path.ends_with(SETTINGS_BUNDLE_EXTENSION))
        .collect();

    let targets: HashMap<&str, &SettingsCatalogEntry> =
        catalog.iter().map(|entry| (entry.target, entry)).collect();

    read_bundles(&paths, |path| {
        let info = read_info_plist(path)?;
        let identifier = info_plist_string(&info, BUNDLE_ID_KEY)?;
        // Only catalog panes are worth the localized-name lookup: the directory
        // holds hundreds of extensions, the catalog around thirty.
        let entry = targets.get(identifier.as_str())?;
        let localized = read_bundle_name(path, SETTINGS_BUNDLE_EXTENSION, &languages)?;

        let stem = Path::new(path)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default();
        (localized != entry.title && localized != stem).then_some((entry.target, localized))
    })
    .into_iter()
    .flatten()
    .collect()
}

/// Resolves a bundle's display name: the first preferred language that has an
/// `InfoPlist.strings` entry wins, falling back to the Info.plist value.
fn read_bundle_name(path: &str, strip_suffix: &str, languages: &[String]) -> Option<String> {
    let info = read_info_plist(path)?;
    let fallbacks = [
        (
            DISPLAY_NAME_KEYS[0],
            info_plist_string(&info, DISPLAY_NAME_KEYS[0]),
        ),
        (
            DISPLAY_NAME_KEYS[1],
            info_plist_string(&info, DISPLAY_NAME_KEYS[1]),
        ),
    ];

    for (key, fallback) in fallbacks {
        for language in language_candidates(languages) {
            if let Some(strings) = read_bundle_strings(path, &language)
                && let Some(value) = info_plist_string(&strings, key)
            {
                return normalize_display_name(&value, strip_suffix);
            }
        }
        // `localizedStringForKey:value:table:localizations:` returns the
        // fallback when the key has no entry, so the first key with a usable
        // Info.plist value settles the name.
        if let Some(fallback) = fallback {
            return normalize_display_name(&fallback, strip_suffix);
        }
    }
    None
}

fn read_info_plist(bundle_path: &str) -> Option<plist::Dictionary> {
    let path = Path::new(bundle_path).join(INFO_PLIST_FILE);
    let value: Value = plist::from_file(path).ok()?;
    value.into_dictionary()
}

/// Reads `Contents/Resources/<language>.lproj/InfoPlist.strings`. Returns
/// `None` when the bundle has no such table, which is the common case.
fn read_bundle_strings(bundle_path: &str, language: &str) -> Option<plist::Dictionary> {
    let path = Path::new(bundle_path)
        .join(BUNDLE_RESOURCES_DIR)
        .join(format!("{language}.lproj"))
        .join(INFO_PLIST_STRINGS_FILE);
    let value: Value = plist::from_file(path).ok()?;
    value.into_dictionary()
}

fn info_plist_string(dict: &plist::Dictionary, key: &str) -> Option<String> {
    dict.get(key)
        .and_then(Value::as_string)
        .map(str::to_string)
        .filter(|value| !value.trim().is_empty())
}

/// The user's preferred languages, most preferred first. `AppleLanguages` is
/// the Foundation source `NSLocale.preferredLanguages` reads; the `LANG`
/// fallback covers a bare shell with no global preferences file.
fn preferred_languages() -> Vec<String> {
    if let Some(languages) = apple_languages() {
        return languages;
    }
    for key in ["LANG", "LC_ALL"] {
        if let Ok(value) = env::var(key) {
            let language = value.trim();
            if !language.is_empty() && language != "C" && language != "POSIX" {
                return vec![language.to_string()];
            }
        }
    }
    vec!["en".to_string()]
}

fn apple_languages() -> Option<Vec<String>> {
    let home = crate::config::user_home_dir()?;
    let path = Path::new(&home).join(GLOBAL_PREFERENCES_PLIST);
    let dict = read_plist_dict(&path)?;
    let array = dict.get(APPLE_LANGUAGES_KEY)?.as_array()?;
    let languages: Vec<String> = array
        .iter()
        .filter_map(Value::as_string)
        .map(str::to_string)
        .filter(|language| !language.trim().is_empty())
        .collect();
    if languages.is_empty() {
        None
    } else {
        Some(languages)
    }
}

fn read_plist_dict(path: &Path) -> Option<plist::Dictionary> {
    let value: Value = plist::from_file(path).ok()?;
    value.into_dictionary()
}

/// Expands `zh-Hans-CN` to `zh-Hans-CN`, `zh_Hans_CN`, `zh-Hans`, `zh_Hans`,
/// `zh`, so both bundle-localized directory conventions are covered. Longest
/// (most specific) first, matching how `NSBundle` resolves localizations.
fn language_candidates(languages: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for raw in languages {
        let normalized = raw.trim().replace('_', "-");
        let normalized = normalized.split('.').next().unwrap_or(normalized.as_str());
        if normalized.is_empty() {
            continue;
        }
        let parts: Vec<&str> = normalized.split('-').collect();
        for end in (1..=parts.len()).rev() {
            for candidate in [parts[..end].join("-"), parts[..end].join("_")] {
                if !out.contains(&candidate) {
                    out.push(candidate);
                }
            }
        }
    }
    out
}

fn normalize_display_name(display_name: &str, strip_suffix: &str) -> Option<String> {
    let name = display_name.trim();
    let name = name.strip_suffix(strip_suffix).unwrap_or(name).trim();
    (!name.is_empty()).then(|| name.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        APP_BUNDLE_EXTENSION, MIN_BUNDLES_PER_READ_THREAD, SETTINGS_BUNDLE_EXTENSION,
        SettingsCatalogEntry, bundle_read_threads, language_candidates, read_bundle_name,
        read_localized_display_names_with,
    };
    use crate::platform::macos::test_support::TempDir;
    use plist::Value;
    use std::fs;
    use std::path::Path;

    /// "Localization test" in Simplified Chinese, to prove the name really came
    /// from the zh-Hans strings file rather than the Info.plist fallback.
    const LOCALIZED_FIXTURE_NAME: &str = "本地化测试";
    const FIXTURE_LANGUAGE: &str = "zh-Hans";

    /// Writes a bundle whose Info.plist carries `info_plist_entries`, plus a
    /// `zh-Hans` InfoPlist.strings when `localized_name` is set.
    fn write_bundle(
        path: &Path,
        info_plist_entries: &[(&str, &str)],
        localized_name: Option<&str>,
    ) {
        fs::create_dir_all(path.join("Contents")).expect("create test bundle contents");
        write_plist(&path.join("Contents/Info.plist"), info_plist_entries);
        if let Some(localized_name) = localized_name {
            let resources = path.join(format!("Contents/Resources/{FIXTURE_LANGUAGE}.lproj"));
            fs::create_dir_all(&resources).expect("create test bundle resources");
            write_plist(
                &resources.join("InfoPlist.strings"),
                &[("CFBundleDisplayName", localized_name)],
            );
        }
    }

    fn write_plist(path: &Path, entries: &[(&str, &str)]) {
        let mut dict = plist::Dictionary::new();
        for (key, value) in entries {
            dict.insert((*key).to_string(), Value::String((*value).to_string()));
        }
        let mut file = fs::File::create(path).expect("write plist");
        plist::to_writer_xml(&mut file, &dict).expect("serialize plist");
    }

    fn read_fixture_name(path: &Path, strip_suffix: &str) -> Option<String> {
        let path = path.to_str().expect("UTF-8 test bundle path");
        read_bundle_name(path, strip_suffix, &[FIXTURE_LANGUAGE.to_string()])
    }

    #[test]
    fn bundle_name_uses_explicit_localization_for_infoplist_strings() {
        let dir = TempDir::new("localized-name");
        let app = dir.path().join(format!("Fixture{APP_BUNDLE_EXTENSION}"));
        write_bundle(
            &app,
            &[
                ("CFBundleIdentifier", "test.zeidex.localized-name"),
                ("CFBundleDisplayName", "English Fixture"),
            ],
            Some(LOCALIZED_FIXTURE_NAME),
        );

        assert_eq!(
            read_fixture_name(&app, APP_BUNDLE_EXTENSION).as_deref(),
            Some(LOCALIZED_FIXTURE_NAME)
        );
    }

    /// A blank `CFBundleDisplayName` makes the localized lookup echo the key
    /// back; the name must fall through to `CFBundleName` instead of being
    /// indexed as "CFBundleDisplayName".
    #[test]
    fn blank_display_name_falls_through_to_bundle_name() {
        let dir = TempDir::new("blank-display-name");
        let app = dir.path().join(format!("Fixture{APP_BUNDLE_EXTENSION}"));
        write_bundle(
            &app,
            &[
                ("CFBundleIdentifier", "test.zeidex.blank-display-name"),
                ("CFBundleDisplayName", ""),
                ("CFBundleName", "Real Name"),
            ],
            None,
        );

        assert_eq!(
            read_fixture_name(&app, APP_BUNDLE_EXTENSION).as_deref(),
            Some("Real Name")
        );
    }

    #[test]
    fn settings_titles_map_only_catalog_targets_with_a_differing_localized_name() {
        static CATALOG: &[SettingsCatalogEntry] = &[SettingsCatalogEntry {
            title: "Fixture Pane",
            target: "test.zeidex.settings-fixture",
            candidate_id_suffix: "test.zeidex.settings-fixture",
            aliases: "settings fixture",
        }];

        let dir = TempDir::new("settings-extensions");
        write_bundle(
            &dir.path()
                .join(format!("Fixture{SETTINGS_BUNDLE_EXTENSION}")),
            &[
                ("CFBundleIdentifier", "test.zeidex.settings-fixture"),
                ("CFBundleDisplayName", "Fixture Pane"),
            ],
            Some(LOCALIZED_FIXTURE_NAME),
        );
        // Not in the catalog: must be skipped rather than indexed.
        write_bundle(
            &dir.path()
                .join(format!("Unlisted{SETTINGS_BUNDLE_EXTENSION}")),
            &[
                ("CFBundleIdentifier", "test.zeidex.unlisted"),
                ("CFBundleDisplayName", "Unlisted Pane"),
            ],
            Some("Unlisted Localized"),
        );

        let titles = super::localized_settings_titles_with(
            CATALOG,
            dir.path().to_str().expect("UTF-8 temp path"),
            &[FIXTURE_LANGUAGE.to_string()],
        );

        assert_eq!(titles.len(), 1);
        assert_eq!(
            titles
                .get("test.zeidex.settings-fixture")
                .map(String::as_str),
            Some(LOCALIZED_FIXTURE_NAME)
        );
    }

    /// Callers zip the result against the input, so a batch spread over several
    /// threads has to come back one-per-input and in the original order. Sized
    /// past the per-thread threshold so the parallel path is the one under test.
    #[test]
    fn batched_names_stay_aligned_with_their_input_paths() {
        let bundle_count = MIN_BUNDLES_PER_READ_THREAD * 3;
        assert!(
            bundle_read_threads(bundle_count) > 1,
            "fixture too small to exercise the threaded path"
        );

        let dir = TempDir::new("batched-names");
        let mut paths = Vec::new();
        for index in 0..bundle_count {
            let app = dir
                .path()
                .join(format!("Fixture{index}{APP_BUNDLE_EXTENSION}"));
            let identifier = format!("test.zeidex.batch{index}");
            let display_name = format!("English {index}");
            let localized_name = format!("{LOCALIZED_FIXTURE_NAME}{index}");
            write_bundle(
                &app,
                &[
                    ("CFBundleIdentifier", identifier.as_str()),
                    ("CFBundleDisplayName", display_name.as_str()),
                ],
                Some(localized_name.as_str()),
            );
            paths.push(app.to_str().expect("UTF-8 test bundle path").to_string());
        }

        let names = read_localized_display_names_with(
            &[FIXTURE_LANGUAGE.to_string()],
            &paths,
            APP_BUNDLE_EXTENSION,
        );

        assert_eq!(names.len(), paths.len());
        for (index, name) in names.iter().enumerate() {
            assert_eq!(
                name.as_deref(),
                Some(format!("{LOCALIZED_FIXTURE_NAME}{index}").as_str()),
                "bundle {index} came back out of order"
            );
        }
    }

    #[test]
    fn language_candidates_expand_from_most_to_least_specific() {
        let candidates = language_candidates(&["zh-Hans-CN".to_string(), "en-US".to_string()]);
        assert_eq!(
            candidates,
            vec![
                "zh-Hans-CN".to_string(),
                "zh_Hans_CN".to_string(),
                "zh-Hans".to_string(),
                "zh_Hans".to_string(),
                "zh".to_string(),
                "en-US".to_string(),
                "en_US".to_string(),
                "en".to_string(),
            ]
        );
    }

    #[test]
    fn language_candidates_strip_encoding_suffix() {
        let candidates = language_candidates(&["en_US.UTF-8".to_string()]);
        assert_eq!(
            candidates,
            vec!["en-US".to_string(), "en_US".to_string(), "en".to_string()]
        );
    }
}
