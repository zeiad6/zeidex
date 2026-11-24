//! User-defined index exclusions.
//!
//! The walker already had three coarse knobs (`file_exclude_paths`,
//! `skip_dir_names`, `ignored_file_patterns`). They are process settings, not a
//! feature: the user cannot name a single file, cannot turn one rule off
//! without deleting it, and cannot see what is excluded. This module is the
//! model behind the Index Exclusions screen — one list of typed, named,
//! individually switchable rules that the walker consults before it stats an
//! entry.
//!
//! A rule is one of four kinds:
//!
//! * `Folder` — a directory and everything below it (prefix match);
//! * `File` — one exact path;
//! * `Extension` — every file with that suffix (`tmp`, `.tmp` both accepted);
//! * `Pattern` — a safe glob. A pattern with a path separator matches the whole
//!   path (`**/node_modules/**`); a bare name pattern matches the file or
//!   directory name (`*.tmp`, `node_modules`).
//!
//! Matching is delegated to [`PathPolicy`], so a Windows rule is compared
//! case-insensitively with `/` separators and a POSIX rule stays exact. Nothing
//! here touches the filesystem: the walker asks, this answers.

use crate::platform::paths::{PathPolicy, path_is_same_or_child};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Separator inside the one-line config encoding. A Windows path cannot
/// contain `|`, and the value is the last field, so a pattern that does contain
/// one survives the round trip.
const FIELD_SEPARATOR: char = '|';

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExclusionKind {
    Folder,
    File,
    Extension,
    Pattern,
}

impl ExclusionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Folder => "folder",
            Self::File => "file",
            Self::Extension => "extension",
            Self::Pattern => "pattern",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "folder" => Some(Self::Folder),
            "file" => Some(Self::File),
            "extension" => Some(Self::Extension),
            "pattern" => Some(Self::Pattern),
            _ => None,
        }
    }
}

/// One persisted rule. `id` is stable across edits so the UI can update a row
/// without rebuilding the list, and `added_at` is unix seconds (0 when the
/// origin is unknown, e.g. a rule written by hand into the config file).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exclusion {
    pub id: String,
    pub kind: ExclusionKind,
    pub value: String,
    pub enabled: bool,
    pub added_at: i64,
}

impl Exclusion {
    /// A stable id for a new rule: seconds since the epoch in hex plus a
    /// process-wide counter. Collisions need two rules created in the same
    /// second by the same process, which the counter rules out.
    pub fn new_id() -> String {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or(0);
        let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
        format!("{seconds:x}{sequence:x}")
    }

    pub fn now() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs() as i64)
            .unwrap_or(0)
    }

    pub fn new(
        id: impl Into<String>,
        kind: ExclusionKind,
        value: impl Into<String>,
        added_at: i64,
    ) -> Self {
        Self {
            id: id.into(),
            kind,
            value: value.into(),
            enabled: true,
            added_at,
        }
    }

    /// One config line: `exclusion=<id>|<kind>|<0|1>|<added>|<value>`.
    pub fn encode(&self) -> String {
        format!(
            "{}{FIELD_SEPARATOR}{}{FIELD_SEPARATOR}{}{FIELD_SEPARATOR}{}{FIELD_SEPARATOR}{}",
            self.id,
            self.kind.as_str(),
            if self.enabled { 1 } else { 0 },
            self.added_at,
            self.value
        )
    }

    /// Returns `None` for a malformed line instead of dropping the whole file:
    /// a hand-edited config should lose the broken rule, not every setting.
    pub fn decode(line: &str) -> Option<Self> {
        let mut parts = line.splitn(5, FIELD_SEPARATOR);
        let id = parts.next()?.trim();
        let kind = ExclusionKind::parse(parts.next()?.trim())?;
        let enabled = match parts.next()?.trim() {
            "1" | "true" => true,
            "0" | "false" => false,
            _ => return None,
        };
        let added_at = parts.next()?.trim().parse::<i64>().unwrap_or(0);
        let value = parts.next().unwrap_or("").trim();
        if id.is_empty() || value.is_empty() {
            return None;
        }
        Some(Self {
            id: id.to_string(),
            kind,
            value: value.to_string(),
            enabled,
            added_at,
        })
    }

    /// A value the walker can actually use. Rejects an empty extension and a
    /// pattern that does not compile, so the UI can show a real error instead
    /// of saving a rule that never matches.
    pub fn validate(kind: ExclusionKind, value: &str) -> Result<String, String> {
        let value = value.trim();
        if value.is_empty() {
            return Err("an exclusion needs a value".to_string());
        }
        match kind {
            ExclusionKind::Folder | ExclusionKind::File => {
                if value.contains('\0') {
                    return Err("the path contains an invalid character".to_string());
                }
                Ok(value.to_string())
            }
            ExclusionKind::Extension => {
                let extension = value.trim_start_matches('.');
                if extension.is_empty() {
                    return Err("an extension needs at least one character".to_string());
                }
                if extension.len() > 24
                    || !extension
                        .chars()
                        .all(|character| character.is_ascii_alphanumeric() || character == '-')
                {
                    return Err(
                        "an extension is letters, digits or '-' (no dots, spaces or slashes)"
                            .to_string(),
                    );
                }
                Ok(extension.to_ascii_lowercase())
            }
            ExclusionKind::Pattern => {
                if value.contains('\0') {
                    return Err("the pattern contains an invalid character".to_string());
                }
                if value.chars().filter(|c| *c == '*').count() > 8 {
                    return Err("the pattern has too many wildcards".to_string());
                }
                // A bare name pattern is compiled against the file name, so it
                // needs no separator handling; a path pattern goes through the
                // same compiler as `ignored_file_patterns`.
                if value.contains('/') || value.contains('\\') {
                    compile_path_glob(value)
                        .ok_or_else(|| "the pattern is not a valid glob".to_string())?;
                } else {
                    GlobBuilder::new(value)
                        .literal_separator(true)
                        .build()
                        .map_err(|_| "the pattern is not a valid glob".to_string())?;
                }
                Ok(value.to_string())
            }
        }
    }
}

/// Compiled, read-only view of the enabled rules. Cloned with the runtime
/// config, so the walk pays no rebuild cost per entry.
#[derive(Clone, Debug, Default)]
pub struct ExclusionSet {
    /// `(policy, normalized path, is_folder)`.
    paths: Vec<(PathPolicy, String, bool)>,
    /// Lowercase extensions without the dot.
    extensions: Vec<String>,
    /// `(policy, set)` for bare-name globs.
    name_globs: Vec<(PathPolicy, GlobSet)>,
    /// `(policy, set)` for globs that contain a separator.
    path_globs: Vec<(PathPolicy, GlobSet)>,
    /// Number of enabled rules, for the UI counter and status text.
    active: usize,
}

impl ExclusionSet {
    pub fn compile(entries: &[Exclusion]) -> Self {
        let mut set = Self {
            active: entries.iter().filter(|entry| entry.enabled).count(),
            ..Self::default()
        };
        let mut name_builders: Vec<(PathPolicy, GlobSetBuilder)> = Vec::new();
        let mut path_builders: Vec<(PathPolicy, GlobSetBuilder)> = Vec::new();

        for entry in entries.iter().filter(|entry| entry.enabled) {
            let value = entry.value.trim();
            if value.is_empty() {
                continue;
            }
            match entry.kind {
                ExclusionKind::Folder | ExclusionKind::File => {
                    let policy = PathPolicy::for_base(value);
                    let normalized = policy.normalize_for_matching(value).into_owned();
                    set.paths
                        .push((policy, normalized, entry.kind == ExclusionKind::Folder));
                }
                ExclusionKind::Extension => {
                    let extension = value.trim_start_matches('.').to_lowercase();
                    if !extension.is_empty() && !set.extensions.contains(&extension) {
                        set.extensions.push(extension);
                    }
                }
                ExclusionKind::Pattern => {
                    if value.contains('/') || value.contains('\\') {
                        let Some((policy, glob)) = compile_path_glob(value) else {
                            continue;
                        };
                        push_builder(&mut path_builders, policy, glob);
                        // `dir/**` names the contents, but the directory row
                        // itself is also excluded; without this the empty
                        // folder still appears in search after its files are
                        // pruned.
                        if let Some(base) = value
                            .strip_suffix("/**")
                            .or_else(|| value.strip_suffix("\\**"))
                            && !base.is_empty()
                            && let Some((base_policy, base_glob)) = compile_path_glob(base)
                        {
                            push_builder(&mut path_builders, base_policy, base_glob);
                        }
                    } else {
                        let policy = PathPolicy::current();
                        // `matches_name` folds the name through the policy, so
                        // the pattern must be folded the same way or a Windows
                        // rule `*.TMP` would never match `a.tmp`.
                        let pattern = if cfg!(target_os = "windows") {
                            value.to_lowercase()
                        } else {
                            value.to_string()
                        };
                        let Ok(glob) = GlobBuilder::new(&pattern).literal_separator(true).build()
                        else {
                            continue;
                        };
                        push_builder(&mut name_builders, policy, glob);
                    }
                }
            }
        }

        set.name_globs = finish_builders(name_builders);
        set.path_globs = finish_builders(path_builders);
        set
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
            && self.extensions.is_empty()
            && self.name_globs.is_empty()
            && self.path_globs.is_empty()
    }

    /// Enabled rules behind this set. The status line reports it so "0 items
    /// excluded" can be distinguished from "no rules".
    pub fn active(&self) -> usize {
        self.active
    }

    /// True when the walk must not stat, index or descend into `path`.
    ///
    /// `is_dir` is passed in rather than read from the filesystem: the walker
    /// already has the entry type, and a second `metadata` call per candidate
    /// is the cost this check exists to avoid.
    pub fn excludes(&self, path: &str, name: &str, is_dir: bool) -> bool {
        if self.is_empty() {
            return false;
        }
        for (policy, excluded, folder) in &self.paths {
            if *folder {
                if path_is_same_or_child(path, excluded) {
                    return true;
                }
            } else if policy.is_same_or_child(path, excluded) {
                return true;
            }
        }

        // A glob can name a directory. Matching it here prunes the whole
        // subtree in `filter_entry` instead of visiting every child.
        if matches_name(&self.name_globs, name) || matches_path(&self.path_globs, path) {
            return true;
        }
        if is_dir {
            return false;
        }
        if !self.extensions.is_empty()
            && let Some(extension) = Path::new(name).extension().and_then(|value| value.to_str())
        {
            let extension = extension.to_lowercase();
            if self.extensions.iter().any(|known| known == &extension) {
                return true;
            }
        }
        false
    }
}

fn push_builder(
    builders: &mut Vec<(PathPolicy, GlobSetBuilder)>,
    policy: PathPolicy,
    glob: globset::Glob,
) {
    match builders
        .iter_mut()
        .find(|(existing, _)| *existing == policy)
    {
        Some((_, builder)) => {
            builder.add(glob);
        }
        None => {
            let mut builder = GlobSetBuilder::new();
            builder.add(glob);
            builders.push((policy, builder));
        }
    }
}

fn finish_builders(builders: Vec<(PathPolicy, GlobSetBuilder)>) -> Vec<(PathPolicy, GlobSet)> {
    builders
        .into_iter()
        .filter_map(|(policy, builder)| builder.build().ok().map(|set| (policy, set)))
        .collect()
}

fn matches_name(sets: &[(PathPolicy, GlobSet)], name: &str) -> bool {
    if sets.is_empty() {
        return false;
    }
    sets.iter()
        .any(|(policy, set)| set.is_match(policy.normalize_for_matching(name).as_ref()))
}

fn matches_path(sets: &[(PathPolicy, GlobSet)], path: &str) -> bool {
    if sets.is_empty() {
        return false;
    }
    sets.iter()
        .any(|(policy, set)| set.is_match(policy.normalize_for_matching(path).as_ref()))
}

/// Compiles a pattern that contains a separator.
///
/// `compile_ignore_glob` infers the path style from the pattern text, which is
/// right for an absolute rule (`C:\...`, `/home/...`) and wrong for a relative
/// one: `**/node_modules/**` contains `/`, so it would be compiled with POSIX
/// rules (case preserved) while a Windows candidate is lowercased, and the rule
/// would silently never match. A relative pattern follows the current platform.
fn compile_path_glob(value: &str) -> Option<(PathPolicy, globset::Glob)> {
    let absolute = value.starts_with('/')
        || value.starts_with("\\\\")
        || value.as_bytes().get(1).is_some_and(|colon| *colon == b':');
    let policy = if absolute {
        PathPolicy::for_base(value)
    } else {
        PathPolicy::current()
    };
    let normalized = policy.normalize_for_matching(value);
    GlobBuilder::new(&normalized)
        .literal_separator(true)
        .build()
        .ok()
        .map(|glob| (policy, glob))
}

#[cfg(test)]
mod tests {
    use super::{Exclusion, ExclusionKind, ExclusionSet};

    fn rule(kind: ExclusionKind, value: &str) -> Exclusion {
        Exclusion::new("test-id", kind, value, 0)
    }

    fn set(rules: &[(ExclusionKind, &str)]) -> ExclusionSet {
        ExclusionSet::compile(
            &rules
                .iter()
                .map(|(kind, value)| rule(*kind, value))
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn a_folder_rule_excludes_the_folder_and_everything_below_it() {
        let set = set(&[(ExclusionKind::Folder, "/home/me/projects/node_modules")]);
        assert!(set.excludes("/home/me/projects/node_modules", "node_modules", true));
        assert!(set.excludes(
            "/home/me/projects/node_modules/react/index.js",
            "index.js",
            false
        ));
        assert!(!set.excludes("/home/me/projects/src/main.rs", "main.rs", false));
        // A sibling whose name shares the prefix is not a child.
        assert!(!set.excludes("/home/me/projects/node_modules-old/x.js", "x.js", false));
    }

    #[test]
    fn a_file_rule_excludes_only_that_file() {
        let set = set(&[(ExclusionKind::File, "/home/me/notes/secret.txt")]);
        assert!(set.excludes("/home/me/notes/secret.txt", "secret.txt", false));
        assert!(!set.excludes("/home/me/notes/secret.txt.bak", "secret.txt.bak", false));
        assert!(!set.excludes("/home/me/notes", "notes", true));
    }

    #[test]
    fn an_extension_rule_matches_the_suffix_and_nothing_else() {
        let set = set(&[(ExclusionKind::Extension, "tmp")]);
        assert!(set.excludes("/home/me/a.tmp", "a.tmp", false));
        assert!(set.excludes("/home/me/a.TMP", "a.TMP", false));
        assert!(!set.excludes("/home/me/a.tmpl", "a.tmpl", false));
        assert!(!set.excludes("/home/me/tmp", "tmp", true));
        assert!(!set.excludes("/home/me/a.tmp", "a.tmp", true));
    }

    #[test]
    fn a_bare_name_pattern_matches_files_and_directories_at_any_depth() {
        let dirs = set(&[(ExclusionKind::Pattern, "node_modules")]);
        assert!(dirs.excludes("/home/me/app/node_modules", "node_modules", true));
        assert!(dirs.excludes("/home/me/app/node_modules", "node_modules", false));
        assert!(!dirs.excludes("/home/me/app/src", "src", true));

        let temp = set(&[(ExclusionKind::Pattern, "*.tmp")]);
        assert!(temp.excludes("/home/me/a.tmp", "a.tmp", false));
        assert!(!temp.excludes("/home/me/a.txt", "a.txt", false));
    }

    #[test]
    fn a_path_pattern_matches_the_whole_path() {
        let set = set(&[(ExclusionKind::Pattern, "**/build/**")]);
        assert!(set.excludes("/home/me/app/build/out.js", "out.js", false));
        assert!(set.excludes("/home/me/app/build", "build", true));
        assert!(!set.excludes("/home/me/app/src/out.js", "out.js", false));
    }

    #[test]
    fn a_disabled_rule_is_ignored() {
        let mut disabled = rule(ExclusionKind::Folder, "/home/me/tmp");
        disabled.enabled = false;
        let set = ExclusionSet::compile(&[disabled]);
        assert!(set.is_empty());
        assert_eq!(set.active(), 0);
        assert!(!set.excludes("/home/me/tmp/a.txt", "a.txt", false));
    }

    #[test]
    fn encode_and_decode_round_trip_a_value_containing_the_separator() {
        let entry = Exclusion::new("abc123", ExclusionKind::Pattern, "a|b/**", 1_700_000_000);
        let decoded = Exclusion::decode(&entry.encode()).expect("round trip");
        assert_eq!(decoded, entry);
    }

    #[test]
    fn a_malformed_line_decodes_to_none_instead_of_panicking() {
        assert!(Exclusion::decode("").is_none());
        assert!(Exclusion::decode("id|not-a-kind|1|0|value").is_none());
        assert!(Exclusion::decode("id|folder|maybe|0|value").is_none());
        assert!(Exclusion::decode("id|folder|1|0|").is_none());
        assert!(Exclusion::decode("id|folder|1|0").is_none());
    }

    #[test]
    fn validation_normalises_extensions_and_rejects_unsafe_values() {
        assert_eq!(
            Exclusion::validate(ExclusionKind::Extension, ".TMP").unwrap(),
            "tmp"
        );
        assert!(Exclusion::validate(ExclusionKind::Extension, ".").is_err());
        assert!(Exclusion::validate(ExclusionKind::Extension, "a/b").is_err());
        assert!(Exclusion::validate(ExclusionKind::Folder, "   ").is_err());
        assert!(Exclusion::validate(ExclusionKind::Pattern, "*.tmp").is_ok());
        assert!(Exclusion::validate(ExclusionKind::Pattern, "[").is_err());
    }
}
