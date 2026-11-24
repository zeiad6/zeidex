use crate::config::RuntimeConfig;
use crate::index::{FILE_CANDIDATE_ID_PREFIX, FOLDER_CANDIDATE_ID_PREFIX};
use crate::platform::paths::{
    PathPolicy, candidate_id_path_component, compile_ignore_glob, path_is_same_or_child,
};
use globset::{GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::UNIX_EPOCH;
use zeidex_indexing::{Candidate, CandidateKind, IndexControl};

// One `GlobSet` per path policy: a candidate is normalized once per distinct
// policy and matched against all patterns of that policy in a single pass.
pub(crate) type IgnoredFileMatchers = Vec<(PathPolicy, GlobSet)>;

fn modified_unix_s(metadata: Option<&std::fs::Metadata>) -> Option<i64> {
    metadata?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs() as i64)
}

const FOLDER_BUDGET_FACTOR: usize = 4;

/// How far a walk may go and the switch that can stop it. Bundled so the
/// recursive walk keeps a readable signature: depth and control are one
/// decision ("how much of this tree may I spend"), not two unrelated flags.
#[derive(Clone, Copy)]
struct WalkLimit<'a> {
    max_depth: usize,
    control: Option<&'a IndexControl>,
}

/// The two budgets a walk spends. Bundled so the recursive call keeps a
/// readable signature and the file limit is checked in one place.
#[derive(Default)]
struct WalkTally {
    files: usize,
    folders: usize,
}

/// Walks every configured root and streams the candidates it finds.
///
/// `skipped` counts one per entry the user's exclusion rules rejected at the
/// traversal boundary: a pruned directory counts once, not once per descendant,
/// because the walker never entered it. That is the number the Indexing screen
/// can honestly show as "entries skipped before they cost a stat".
pub fn discover_local_files_and_folders(
    config: &RuntimeConfig,
    tx: mpsc::SyncSender<Candidate>,
    control: Option<&IndexControl>,
    skipped: Arc<AtomicUsize>,
) {
    let roots = config.file_scan_root_list();

    // Compile the ignore matchers once, not once per root.
    let ignored_file_matchers = build_ignored_matchers(&config.ignored_file_patterns);

    let mut tally = WalkTally::default();
    for root in &roots {
        // A pause between roots parks the walk instead of spinning: the
        // checkpoint blocks on the control's condition variable.
        if control.is_some_and(|control| !control.checkpoint()) {
            return;
        }
        walk_files(
            root,
            config,
            &ignored_file_matchers,
            &tx,
            &mut tally,
            control,
            &skipped,
        );
        if tally.files >= config.file_scan_limit {
            break;
        }
    }
}

fn walk_files(
    path: &str,
    config: &RuntimeConfig,
    ignored_file_matchers: &IgnoredFileMatchers,
    tx: &mpsc::SyncSender<Candidate>,
    tally: &mut WalkTally,
    control: Option<&IndexControl>,
    skipped: &Arc<AtomicUsize>,
) {
    walk_files_with_depth(
        path,
        config,
        ignored_file_matchers,
        tx,
        tally,
        WalkLimit {
            max_depth: config.file_scan_depth,
            control,
        },
        skipped,
    );
}

/// Walks `path` and emits the candidates under it, counting depth from `path`.
/// `max_depth` is overridden by the incremental path: a directory that appeared
/// under a root at depth k may only spend `file_scan_depth - k` more levels, or
/// a moved-in subtree would index deeper than a full walk ever would.
fn walk_files_with_depth(
    path: &str,
    config: &RuntimeConfig,
    ignored_file_matchers: &IgnoredFileMatchers,
    tx: &mpsc::SyncSender<Candidate>,
    tally: &mut WalkTally,
    limit: WalkLimit<'_>,
    skipped: &Arc<AtomicUsize>,
) {
    let WalkLimit { max_depth, control } = limit;
    if should_exclude_path(path, &config.file_exclude_paths) {
        return;
    }

    let mut walker = WalkBuilder::new(path);
    let root_path = path.to_string();
    let exclude_paths = config.file_exclude_paths.clone();
    let skip_dir_names = config.skip_dir_names.clone();
    let exclusions = config.exclusion_set.clone();
    let skipped = Arc::clone(skipped);
    walker
        .hidden(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .parents(false)
        .max_depth(Some(max_depth))
        .filter_entry(move |entry| {
            let path_buf = entry.path();
            let Some(path_str) = path_buf.to_str() else {
                return false;
            };

            if path_str == root_path {
                return true;
            }

            if should_exclude_path(path_str, &exclude_paths) {
                return false;
            }

            let Some(name) = path_buf.file_name().and_then(|value| value.to_str()) else {
                return false;
            };

            // The user's own rules come first: a pruned directory costs one
            // `file_type` call, while visiting its children costs a stat and a
            // candidate each.
            if exclusions.excludes(
                path_str,
                name,
                entry.file_type().is_some_and(|t| t.is_dir()),
            ) {
                // One per boundary: the walker never descends into a pruned
                // directory, so counting descendants here would require the
                // traversal this feature exists to avoid.
                skipped.fetch_add(1, Ordering::Relaxed);
                return false;
            }

            if name.starts_with('.') {
                return false;
            }

            // Name-based skip applies to anything (dir, file, junction).
            // walkdir's `file_type` doesn't follow Windows reparse points, so
            // gating on `is_dir()` here lets `Documents\My Music` (a junction
            // to `~\Music`) leak through and become a duplicate folder result.
            if name.ends_with(".app") || should_skip_dir(name, &skip_dir_names) {
                return false;
            }

            true
        });

    for entry in walker.build().flatten() {
        if tally.files >= config.file_scan_limit {
            break;
        }
        // One atomic load per entry on the normal path; a paused or cancelled
        // walk stops here, before the stat, so the cost of stopping is a single
        // directory entry.
        if control.is_some_and(|control| !control.checkpoint()) {
            break;
        }

        let path_buf = entry.into_path();
        let Some(path_str) = path_buf.to_str() else {
            continue;
        };
        if path_str == path {
            continue;
        }

        // One stat per entry, reused for both kind and mtime. `fs::metadata`
        // follows symlinks, matching the old `Path::is_dir`/`Path::is_file` calls,
        // which each did their own separate stat on top of the mtime read.
        let Ok(metadata) = std::fs::metadata(&path_buf) else {
            continue;
        };

        if metadata.is_dir() {
            // Folders don't count toward the file limit; they get their own.
            if tally.folders >= config.file_scan_limit.saturating_mul(FOLDER_BUDGET_FACTOR) {
                continue;
            }
            if let Some(candidate) =
                candidate_from_metadata(config, &path_buf, &metadata, ignored_file_matchers)
            {
                tally.folders += 1;
                let _ = tx.send(candidate);
            }
            continue;
        }

        if metadata.is_file()
            && let Some(candidate) =
                candidate_from_metadata(config, &path_buf, &metadata, ignored_file_matchers)
        {
            tally.files += 1;
            let _ = tx.send(candidate);
        }
    }
}

/// The one place an entry becomes a candidate, shared by the full walk and the
/// watcher's single-path refresh. Two builders would drift: the watcher would
/// resurrect a hidden file the walk drops, or emit a different id.
fn candidate_from_metadata(
    config: &RuntimeConfig,
    path_buf: &std::path::Path,
    metadata: &std::fs::Metadata,
    ignored_file_matchers: &IgnoredFileMatchers,
) -> Option<Candidate> {
    let path_str = path_buf.to_str()?;
    let name = path_buf.file_name().and_then(|s| s.to_str())?;
    if name.starts_with('.')
        || name.ends_with(".app")
        || should_skip_dir(name, &config.skip_dir_names)
        || should_exclude_path(path_str, &config.file_exclude_paths)
        || config
            .exclusion_set
            .excludes(path_str, name, metadata.is_dir())
    {
        return None;
    }
    let modified_at = modified_unix_s(Some(metadata));

    if metadata.is_dir() {
        let key = format!(
            "{FOLDER_CANDIDATE_ID_PREFIX}{}",
            candidate_id_path_component(path_str)
        );
        let mut candidate = Candidate::new(&key, CandidateKind::Folder, name, path_str);
        candidate.subtitle = Some(CandidateKind::Folder.as_str().into());
        candidate.fs_modified_at_unix_s = modified_at;
        return Some(candidate);
    }

    if metadata.is_file() {
        // Normalize the candidate with the same policy used for the pattern.
        // Without this, a Windows pattern like `C:\tmp\*.log` becomes lowercase
        // `c:/...`, while walker output like `C:/tmp/debug.log` can keep its
        // original case and miss the glob.
        if ignored_file_matchers
            .iter()
            .any(|(policy, glob_set)| glob_set.is_match(&*policy.normalize_for_matching(path_str)))
        {
            return None;
        }

        let key = format!(
            "{FILE_CANDIDATE_ID_PREFIX}{}",
            candidate_id_path_component(path_str)
        );
        let mut candidate = Candidate::new(&key, CandidateKind::File, name, path_str);
        candidate.subtitle = Some(CandidateKind::File.as_str().into());
        candidate.fs_modified_at_unix_s = modified_at;
        candidate.fs_size_bytes = Some(metadata.len());
        return Some(candidate);
    }

    None
}

/// Builds the candidate a full walk would have produced for one path.
///
/// `root` is the configured scan root the path belongs to; depth is measured
/// from it. Returns `None` when the walk would have skipped the path (hidden
/// name, ignored glob, `.app` bundle, excluded directory, too deep) or when it
/// no longer exists.
pub(crate) fn candidate_for_path(
    config: &RuntimeConfig,
    root: &str,
    path: &std::path::Path,
    ignored_file_matchers: &IgnoredFileMatchers,
) -> Option<Candidate> {
    let path_str = path.to_str()?;
    if !path_is_same_or_child(path_str, root) {
        return None;
    }
    let depth = std::path::Path::new(path_str)
        .strip_prefix(root)
        .ok()?
        .components()
        .count();
    if depth == 0 || depth > config.file_scan_depth {
        return None;
    }
    let metadata = std::fs::metadata(path).ok()?;
    candidate_from_metadata(config, path, &metadata, ignored_file_matchers)
}

pub(crate) fn compile_ignored_matchers(patterns: &[String]) -> IgnoredFileMatchers {
    build_ignored_matchers(patterns)
}

/// Walks the children of a directory that appeared during a watcher refresh.
/// The directory's own candidate is built by `candidate_for_path`; this covers
/// a folder moved in with its contents, where no child create events fire.
pub(crate) fn walk_children(
    path: &str,
    config: &RuntimeConfig,
    ignored_file_matchers: &IgnoredFileMatchers,
    max_depth: usize,
    out: &mut Vec<Candidate>,
) {
    let (tx, rx) = mpsc::sync_channel(1024);
    let mut tally = WalkTally::default();
    walk_files_with_depth(
        path,
        config,
        ignored_file_matchers,
        &tx,
        &mut tally,
        WalkLimit {
            max_depth,
            control: None,
        },
        &Arc::new(AtomicUsize::new(0)),
    );
    drop(tx);
    out.extend(rx);
}

fn build_ignored_matchers(patterns: &[String]) -> IgnoredFileMatchers {
    let mut groups: Vec<(PathPolicy, GlobSetBuilder)> = Vec::new();
    for (policy, glob) in patterns
        .iter()
        .filter_map(|pattern| compile_ignore_glob(pattern))
    {
        match groups.iter_mut().find(|(existing, _)| *existing == policy) {
            Some((_, builder)) => {
                builder.add(glob);
            }
            None => {
                let mut builder = GlobSetBuilder::new();
                builder.add(glob);
                groups.push((policy, builder));
            }
        }
    }
    groups
        .into_iter()
        .filter_map(|(policy, builder)| builder.build().ok().map(|set| (policy, set)))
        .collect()
}

fn should_skip_dir(name: &str, skip_dir_names: &[String]) -> bool {
    let lower = name.to_lowercase();
    skip_dir_names.iter().any(|entry| entry == &lower)
}

fn should_exclude_path(path: &str, file_exclude_paths: &[String]) -> bool {
    file_exclude_paths.iter().any(|entry| {
        let normalized_exclude = entry.trim();
        if normalized_exclude.is_empty() {
            return false;
        }
        path_is_same_or_child(path, normalized_exclude)
    })
}

#[cfg(test)]
mod tests {
    use super::{discover_local_files_and_folders, should_exclude_path};
    use crate::config::RuntimeConfig;
    use crate::platform::paths::compile_ignore_matcher;
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use std::sync::mpsc;
    use zeidex_indexing::CandidateKind;

    fn temp_path(prefix: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "zeidex-files-test-{prefix}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time should be after epoch")
                .as_nanos()
        ))
    }

    fn windows_style_ignored_pattern_matches(pattern: &str, path: &str) -> bool {
        let Some((policy, matcher)) = compile_ignore_matcher(pattern) else {
            return false;
        };
        let normalized_path = policy.normalize_for_matching(path);
        matcher.is_match(&*normalized_path)
    }

    #[test]
    fn excludes_nested_paths_and_exact_matches() {
        let excludes = vec!["/Users/demo/Downloads/tmp".to_string()];
        assert!(should_exclude_path("/Users/demo/Downloads/tmp", &excludes));
        assert!(should_exclude_path(
            "/Users/demo/Downloads/tmp/cache/file.txt",
            &excludes
        ));
    }

    #[test]
    fn does_not_exclude_unrelated_paths() {
        let excludes = vec!["/Users/demo/Downloads/tmp".to_string()];
        assert!(!should_exclude_path(
            "/Users/demo/Downloads/template",
            &excludes
        ));
        assert!(!should_exclude_path(
            "/Users/demo/Documents/report.md",
            &excludes
        ));
    }

    #[test]
    fn handles_trailing_slashes_and_blank_entries() {
        let excludes = vec!["/Users/demo/Downloads/tmp/".to_string(), " ".to_string()];
        assert!(should_exclude_path("/Users/demo/Downloads/tmp", &excludes));
        assert!(should_exclude_path(
            "/Users/demo/Downloads/tmp/cache/a.txt",
            &excludes
        ));
    }

    #[test]
    fn path_prefix_is_boundary_aware() {
        let excludes = vec!["/Users/demo/Down".to_string()];
        assert!(!should_exclude_path("/Users/demo/Downloads", &excludes));
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn exclude_path_matching_supports_windows_style_separators() {
        let excludes = vec!["C:\\Users\\demo\\Downloads".to_string()];
        assert!(should_exclude_path(
            "C:/Users/demo/Downloads/cache/a.txt",
            &excludes
        ));
    }

    #[test]
    fn a_folder_added_in_settings_is_walked_before_the_limit_is_spent() {
        // `file_scan_limit` is one budget shared by the whole walk, so the root
        // walked first spends it. The walk used to sort the roots, which put a
        // standard folder (`a-standard`) ahead of the folder the user had just
        // added (`z-added`) for no reason but its name, so the added folder was
        // never reached and its files never appeared in search.
        let standard = temp_path("a-standard");
        let added = temp_path("z-added");
        std::fs::create_dir_all(&standard).expect("create standard root");
        std::fs::create_dir_all(&added).expect("create added root");
        for index in 0..5 {
            std::fs::write(standard.join(format!("file-{index}.txt")), "x")
                .expect("write standard file");
        }
        std::fs::write(added.join("wanted.txt"), "x").expect("write added file");

        let config = RuntimeConfig {
            file_scan_roots: vec![standard.to_string_lossy().into_owned()],
            file_scan_extra_roots: vec![added.to_string_lossy().into_owned()],
            file_scan_depth: 4,
            file_scan_limit: 5,
            ..Default::default()
        };
        let (tx, rx) = mpsc::sync_channel(64);
        discover_local_files_and_folders(&config, tx, None, Arc::new(AtomicUsize::new(0)));
        let walked = rx
            .try_iter()
            .map(|candidate| candidate.path)
            .collect::<Vec<_>>();
        assert!(
            walked.iter().any(|path| path.ends_with("wanted.txt")),
            "the folder the user added must be indexed, not starved by the limit: {walked:?}"
        );

        let _ = std::fs::remove_dir_all(&standard);
        let _ = std::fs::remove_dir_all(&added);
    }

    #[test]
    fn single_path_builder_matches_the_walk() {
        let root = temp_path("single-path-builder");
        let nested = root.join("nested");
        std::fs::create_dir_all(&nested).expect("create nested directory");
        std::fs::write(root.join("notes.md"), "notes").expect("write notes");
        std::fs::write(nested.join("report.pdf"), "pdf").expect("write report");
        std::fs::write(root.join(".hidden"), "hidden").expect("write hidden");

        let config = RuntimeConfig {
            file_scan_roots: vec![root.to_string_lossy().into_owned()],
            file_scan_depth: 4,
            file_scan_limit: 100,
            ..Default::default()
        };
        let (tx, rx) = mpsc::sync_channel(64);
        discover_local_files_and_folders(&config, tx, None, Arc::new(AtomicUsize::new(0)));
        let walked = rx.try_iter().collect::<Vec<_>>();
        assert!(walked.len() >= 3, "fixture should produce rows: {walked:?}");

        let root_str = root.to_string_lossy();
        let matchers = super::build_ignored_matchers(&config.ignored_file_patterns);
        for candidate in &walked {
            let built = super::candidate_for_path(
                &config,
                &root_str,
                std::path::Path::new(&*candidate.path),
                &matchers,
            )
            .unwrap_or_else(|| panic!("walked candidate must rebuild: {:?}", candidate.path));
            assert_eq!(
                built.id, candidate.id,
                "id drifted for {:?}",
                candidate.path
            );
            assert_eq!(built.kind, candidate.kind);
            assert_eq!(built.title, candidate.title);
            assert_eq!(built.subtitle, candidate.subtitle);
            assert_eq!(built.path, candidate.path);
            assert_eq!(
                built.fs_modified_at_unix_s, candidate.fs_modified_at_unix_s,
                "mtime drifted for {:?}",
                candidate.path
            );
        }
        assert!(
            !walked
                .iter()
                .any(|candidate| candidate.title.as_ref() == ".hidden"),
            "the walk must skip hidden names"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn single_path_builder_rejects_out_of_root_entries() {
        let root = temp_path("single-path-root");
        let other = temp_path("single-path-other");
        std::fs::create_dir_all(&root).expect("create root");
        std::fs::create_dir_all(&other).expect("create other");
        std::fs::write(other.join("elsewhere.txt"), "x").expect("write outside file");

        let config = RuntimeConfig {
            file_scan_roots: vec![root.to_string_lossy().into_owned()],
            file_scan_depth: 1,
            ..Default::default()
        };
        let matchers = super::build_ignored_matchers(&config.ignored_file_patterns);
        assert!(
            super::candidate_for_path(
                &config,
                &root.to_string_lossy(),
                &other.join("elsewhere.txt"),
                &matchers
            )
            .is_none(),
            "a path outside the root must not be indexable"
        );

        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&other);
    }

    #[test]
    fn ignored_patterns_skip_matching_files_but_keep_other_entries() {
        let root = temp_path("ignored-patterns");
        let nested = root.join("nested");
        std::fs::create_dir_all(&nested).expect("should create test directories");
        std::fs::write(root.join("debug.log"), "log").expect("should write log file");
        std::fs::write(root.join("notes.md"), "notes").expect("should write notes file");
        std::fs::write(nested.join("state.db-wal"), "wal").expect("should write wal file");

        let config = RuntimeConfig {
            file_scan_roots: vec![root.to_string_lossy().into_owned()],
            file_scan_depth: 4,
            file_scan_limit: 100,
            ignored_file_patterns: vec![
                root.join("*.log").to_string_lossy().into_owned(),
                root.join("**")
                    .join("*.db-wal")
                    .to_string_lossy()
                    .into_owned(),
            ],
            ..Default::default()
        };

        let (tx, rx) = mpsc::sync_channel(128);
        discover_local_files_and_folders(&config, tx, None, Arc::new(AtomicUsize::new(0)));
        let candidates = rx.try_iter().collect::<Vec<_>>();

        assert!(candidates.iter().any(|candidate| {
            candidate.kind == CandidateKind::File && candidate.title.as_ref() == "notes.md"
        }));
        assert!(candidates.iter().any(|candidate| {
            candidate.kind == CandidateKind::Folder && candidate.title.as_ref() == "nested"
        }));
        assert!(
            !candidates
                .iter()
                .any(|candidate| candidate.title.as_ref() == "debug.log")
        );
        assert!(
            !candidates
                .iter()
                .any(|candidate| candidate.title.as_ref() == "state.db-wal")
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn ignored_patterns_do_not_consume_file_scan_limit() {
        let root = temp_path("ignored-pattern-limit");
        std::fs::create_dir_all(&root).expect("should create test directory");
        std::fs::write(root.join("debug.log"), "log").expect("should write ignored file");
        std::fs::write(root.join("notes.md"), "notes").expect("should write kept file");

        let config = RuntimeConfig {
            file_scan_roots: vec![root.to_string_lossy().into_owned()],
            file_scan_depth: 2,
            file_scan_limit: 1,
            ignored_file_patterns: vec![root.join("*.log").to_string_lossy().into_owned()],
            ..Default::default()
        };

        let (tx, rx) = mpsc::sync_channel(32);
        discover_local_files_and_folders(&config, tx, None, Arc::new(AtomicUsize::new(0)));
        let candidates = rx.try_iter().collect::<Vec<_>>();

        let file_titles = candidates
            .iter()
            .filter(|candidate| candidate.kind == CandidateKind::File)
            .map(|candidate| candidate.title.as_ref())
            .collect::<Vec<_>>();
        assert_eq!(file_titles, vec!["notes.md"]);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn ignored_patterns_folder_scoped_glob_only_matches_direct_children() {
        // Case:
        // - root/direct.tmp
        // - root/nested/child.tmp
        // - ignored_file_patterns = [root/*.tmp]
        let root = temp_path("ignored-pattern-direct-children");
        let nested = root.join("nested");
        std::fs::create_dir_all(&nested).expect("should create test directories");
        std::fs::write(root.join("direct.tmp"), "tmp").expect("should write direct file");
        std::fs::write(nested.join("child.tmp"), "tmp").expect("should write nested file");

        let config = RuntimeConfig {
            file_scan_roots: vec![root.to_string_lossy().into_owned()],
            file_scan_depth: 4,
            file_scan_limit: 10,
            ignored_file_patterns: vec![root.join("*.tmp").to_string_lossy().into_owned()],
            ..Default::default()
        };

        let (tx, rx) = mpsc::sync_channel(32);
        discover_local_files_and_folders(&config, tx, None, Arc::new(AtomicUsize::new(0)));
        let candidates = rx.try_iter().collect::<Vec<_>>();

        assert!(
            !candidates
                .iter()
                .any(|candidate| candidate.title.as_ref() == "direct.tmp")
        );
        assert!(
            candidates
                .iter()
                .any(|candidate| candidate.title.as_ref() == "child.tmp")
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn ignored_patterns_windows_style_paths_match_consistently() {
        // Case:
        // - pattern = C:\Users\me\AppData\Local\Temp\**\*.etl
        // - path = C:/Users/me/AppData/Local/Temp/nested/trace.etl
        // - unrelated = C:/Users/me/AppData/Local/Temp/nested/trace.log
        assert!(windows_style_ignored_pattern_matches(
            r"C:\Users\me\AppData\Local\Temp\**\*.etl",
            "C:/Users/me/AppData/Local/Temp/nested/trace.etl"
        ));
        assert!(!windows_style_ignored_pattern_matches(
            r"C:\Users\me\AppData\Local\Temp\**\*.etl",
            "C:/Users/me/AppData/Local/Temp/nested/trace.log"
        ));
    }

    #[test]
    fn ignored_patterns_match_windows_backslash_pattern_against_slash_candidate() {
        let pattern = r"C:\Users\me\Temp\*.log";
        let (policy, matcher) = compile_ignore_matcher(pattern).expect("pattern should compile");

        assert!(matcher.is_match(&*policy.normalize_for_matching("C:/Users/me/Temp/debug.log")));
        assert!(!matcher.is_match(&*policy.normalize_for_matching("C:/Users/me/Temp/debug.txt")));
    }
}
