//! Incremental index refresh, the path the filesystem watcher uses.
//!
//! A full walk is correct but O(total). The watcher already knows which paths
//! changed, so these tests pin the contract that matters: a delta adds exactly
//! the rows a full walk would have produced, removes exactly the rows under the
//! removed paths (never a sibling whose name shares a prefix), and leaves every
//! other row alone.

// Tests are allowed to fail loudly: the crate deny-list applies to shipped code.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::{Path, PathBuf};

use zeidex_engine::config::RuntimeConfig;
use zeidex_engine::index::discover_candidates_stream_scoped;
use zeidex_engine::{BootstrapScope, QueryEngine, RefreshOutcome};
use zeidex_storage::SqliteStore;

struct Fixture {
    base: PathBuf,
    root: PathBuf,
    db: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let base = std::env::temp_dir().join(format!(
            "zeidex-incremental-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time should be after the epoch")
                .as_nanos()
        ));
        let root = base.join("root");
        std::fs::create_dir_all(&root).expect("create scan root");
        Self {
            db: base.join("index.db"),
            base,
            root,
        }
    }

    fn config(&self) -> RuntimeConfig {
        RuntimeConfig {
            file_scan_roots: vec![self.root.to_string_lossy().into_owned()],
            file_scan_extra_roots: Vec::new(),
            file_scan_depth: 4,
            file_scan_limit: 100,
            ignored_file_patterns: vec![self.root.join("*.log").to_string_lossy().into_owned()],
            ..Default::default()
        }
    }

    /// Seeds the database through the real full-walk path, not by hand-built
    /// rows, so the tests compare the watcher against the walker.
    fn seed(&self, config: &RuntimeConfig) {
        let (rx, handle) =
            discover_candidates_stream_scoped(config, BootstrapScope::FILES_ONLY).into_parts();
        let candidates: Vec<_> = rx.iter().collect();
        handle.join().expect("discovery thread should not panic");
        let mut store = SqliteStore::open(&self.db).expect("open index");
        store
            .upsert_candidates(&candidates)
            .expect("seed candidates");
    }

    fn paths(&self) -> Vec<String> {
        let store = SqliteStore::open(&self.db).expect("open index");
        let mut paths: Vec<String> = store
            .candidate_paths()
            .expect("candidate paths")
            .into_iter()
            .map(|(_, path)| path)
            .collect();
        paths.sort();
        paths
    }

    fn titles(&self) -> Vec<String> {
        let store = SqliteStore::open(&self.db).expect("open index");
        let mut titles: Vec<String> = store
            .load_candidates(None)
            .expect("load candidates")
            .into_iter()
            .map(|candidate| candidate.title.to_string())
            .collect();
        titles.sort();
        titles
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

fn refresh(
    fixture: &Fixture,
    config: &RuntimeConfig,
    removed: &[PathBuf],
    added: &[PathBuf],
) -> RefreshOutcome {
    QueryEngine::refresh_paths(&fixture.db, config, removed, added).expect("refresh")
}

#[test]
fn a_created_file_is_indexed_without_a_full_walk() {
    let fixture = Fixture::new("created-file");
    let config = fixture.config();
    std::fs::write(fixture.root.join("existing.txt"), "x").expect("seed file");
    fixture.seed(&config);

    let created = fixture.root.join("new-notes.txt");
    std::fs::write(&created, "hello").expect("create file");

    let outcome = refresh(&fixture, &config, &[], std::slice::from_ref(&created));
    assert_eq!(outcome.removed, 0);
    assert_eq!(outcome.added, 1, "one file, one row");
    assert!(
        fixture
            .titles()
            .iter()
            .any(|title| title == "new-notes.txt"),
        "the created file must be searchable: {:?}",
        fixture.titles()
    );
    assert!(
        fixture
            .paths()
            .iter()
            .any(|path| path == &created.to_string_lossy()),
        "the row must carry the created path"
    );
}

#[test]
fn removing_a_folder_removes_its_children_but_not_a_sibling_sharing_a_prefix() {
    let fixture = Fixture::new("remove-folder");
    let config = fixture.config();
    std::fs::create_dir_all(fixture.root.join("reports")).expect("create folder");
    std::fs::write(fixture.root.join("reports/child.txt"), "x").expect("child");
    std::fs::write(fixture.root.join("keep.txt"), "x").expect("keep");
    std::fs::write(fixture.root.join("reports-old.txt"), "x").expect("prefix sibling");
    fixture.seed(&config);

    let removed_folder = fixture.root.join("reports");
    std::fs::remove_dir_all(&removed_folder).expect("delete folder");
    let outcome = refresh(&fixture, &config, &[removed_folder], &[]);

    assert_eq!(outcome.removed, 2, "the folder row and its child row");
    let paths = fixture.paths();
    assert!(paths.iter().any(|path| path.ends_with("keep.txt")));
    assert!(
        paths.iter().any(|path| path.ends_with("reports-old.txt")),
        "`reports` must not match `reports-old.txt`: {paths:?}"
    );
    assert!(!paths.iter().any(|path| path.ends_with("child.txt")));
    assert!(
        !paths
            .iter()
            .any(|path| Path::new(path).ends_with("reports"))
    );
}

#[test]
fn renaming_a_file_replaces_the_row_in_one_refresh() {
    let fixture = Fixture::new("rename");
    let config = fixture.config();
    let old = fixture.root.join("before.txt");
    std::fs::write(&old, "x").expect("seed file");
    fixture.seed(&config);

    let new = fixture.root.join("after.txt");
    std::fs::rename(&old, &new).expect("rename");

    let outcome = refresh(&fixture, &config, &[old], &[new]);
    assert_eq!(outcome.removed, 1);
    assert_eq!(outcome.added, 1);

    let paths = fixture.paths();
    assert_eq!(paths.len(), 1, "rename must not duplicate: {paths:?}");
    assert!(paths[0].ends_with("after.txt"));
}

#[test]
fn filters_apply_to_the_watcher_too() {
    let fixture = Fixture::new("filters");
    let config = fixture.config();
    fixture.seed(&config);

    let ignored = fixture.root.join("debug.log");
    let hidden = fixture.root.join(".secret");
    let too_deep = fixture.root.join("a/b/c/d/e/deep.txt");
    std::fs::create_dir_all(too_deep.parent().expect("parent")).expect("deep dirs");
    std::fs::write(&ignored, "x").expect("ignored file");
    std::fs::write(&hidden, "x").expect("hidden file");
    std::fs::write(&too_deep, "x").expect("deep file");

    let outcome = refresh(
        &fixture,
        &config,
        &[],
        &[
            ignored.clone(),
            hidden.clone(),
            too_deep.clone(),
            fixture.root.join("a/b/c/d/e"),
        ],
    );

    assert_eq!(outcome.added, 0, "no filter may be bypassed by an event");
    let paths = fixture.paths();
    assert!(!paths.iter().any(|path| path.ends_with("debug.log")));
    assert!(!paths.iter().any(|path| path.ends_with(".secret")));
    assert!(!paths.iter().any(|path| path.ends_with("deep.txt")));
}

#[test]
fn a_moved_in_folder_indexes_its_subtree_once() {
    let fixture = Fixture::new("moved-folder");
    let config = fixture.config();
    fixture.seed(&config);

    let moved = fixture.root.join("moved");
    std::fs::create_dir_all(moved.join("sub")).expect("moved subtree");
    std::fs::write(moved.join("sub/data.txt"), "x").expect("moved file");

    let outcome = refresh(&fixture, &config, &[], &[moved]);

    assert_eq!(outcome.added, 3, "folder, sub-folder and file");
    let paths = fixture.paths();
    assert_eq!(paths.len(), 3, "{paths:?}");
    // Component-wise, not a string suffix: Windows stores `moved\sub`, and a
    // `/moved` suffix test passed on Unix while failing there.
    assert!(paths.iter().any(|path| Path::new(path).ends_with("moved")));
    assert!(
        paths
            .iter()
            .any(|path| Path::new(path).ends_with("moved/sub"))
    );
    assert!(
        paths
            .iter()
            .any(|path| Path::new(path).ends_with("moved/sub/data.txt"))
    );
}

#[test]
fn an_event_for_a_missing_path_is_a_noop() {
    let fixture = Fixture::new("missing");
    let config = fixture.config();
    std::fs::write(fixture.root.join("keep.txt"), "x").expect("seed file");
    fixture.seed(&config);

    let outcome = refresh(
        &fixture,
        &config,
        &[],
        &[fixture.root.join("never-created.txt")],
    );
    assert_eq!(outcome, RefreshOutcome::default());
    assert_eq!(
        fixture.paths(),
        vec![fixture.root.join("keep.txt").to_string_lossy().into_owned()]
    );
}

#[test]
fn a_refresh_outside_the_configured_roots_changes_nothing() {
    let fixture = Fixture::new("outside");
    let config = fixture.config();
    std::fs::write(fixture.root.join("keep.txt"), "x").expect("seed file");
    fixture.seed(&config);

    let outside = fixture.base.join("outside");
    std::fs::create_dir_all(&outside).expect("outside dir");
    let outside_file = outside.join("stranger.txt");
    std::fs::write(&outside_file, "x").expect("outside file");

    let outcome = refresh(&fixture, &config, &[], &[outside_file]);
    assert_eq!(outcome.added, 0);
    assert_eq!(fixture.paths().len(), 1);
}

#[allow(dead_code)]
fn assert_paths_contain(paths: &[String], expected: &Path) {
    assert!(
        paths.iter().any(|path| Path::new(path) == expected),
        "{expected:?} missing from {paths:?}"
    );
}
