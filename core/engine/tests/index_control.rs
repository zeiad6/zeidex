//! Pause and cancel for a full index walk.
//!
//! The dangerous part of stopping a walk is not stopping: it is the stale-row
//! sweep that runs at the end. Rows the walk never visited are not gone, and
//! pruning them would delete a large part of a working index. These tests pin
//! that rule from both sides — a cancelled walk keeps everything, and the next
//! complete walk still removes what really disappeared.

// Tests are allowed to fail loudly: the crate deny-list applies to shipped code.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use zeidex_engine::config::RuntimeConfig;
use zeidex_engine::{BootstrapOutcome, BootstrapScope, QueryEngine};
use zeidex_indexing::IndexControl;
use zeidex_storage::SqliteStore;

struct Fixture {
    base: PathBuf,
    root: PathBuf,
    db: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let base = std::env::temp_dir().join(format!(
            "zeidex-index-control-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
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
            file_scan_depth: 3,
            file_scan_limit: 500,
            ignored_file_patterns: Vec::new(),
            ..Default::default()
        }
    }

    fn write(&self, name: &str) {
        std::fs::write(self.root.join(name), "x").expect("write fixture file");
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

    fn run(&self, config: &RuntimeConfig, control: &Arc<IndexControl>) -> BootstrapOutcome {
        QueryEngine::bootstrap_sqlite_controlled_with(
            &self.db,
            config,
            BootstrapScope::FILES_ONLY,
            control,
        )
        .expect("controlled walk")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

#[test]
fn a_cancelled_walk_keeps_the_rows_it_never_reached() {
    let fixture = Fixture::new("cancel-keeps-rows");
    let config = fixture.config();
    fixture.write("keep.txt");
    fixture.write("gone.txt");
    fixture.write("also-keep.txt");

    let first = fixture.run(&config, &IndexControl::shared());
    assert_eq!(
        first,
        BootstrapOutcome::Complete {
            discovered: 3,
            skipped: 0
        },
        "the first walk indexes the three files"
    );
    assert_eq!(fixture.paths().len(), 3);

    // One file disappears from disk, and the next walk is cancelled before it
    // starts. The sweep must not run: it would delete `gone.txt` (correctly, in
    // a complete walk) and would also delete any row it never visited.
    std::fs::remove_file(fixture.root.join("gone.txt")).expect("remove fixture file");
    let cancelled = IndexControl::shared();
    cancelled.cancel();

    let outcome = fixture.run(&config, &cancelled);
    assert_eq!(
        outcome,
        BootstrapOutcome::Cancelled {
            discovered: 0,
            skipped: 0
        }
    );
    assert!(
        outcome.was_cancelled(),
        "the caller must be able to tell a stopped walk from a finished one"
    );
    assert_eq!(
        fixture.paths().len(),
        3,
        "a cancelled walk prunes nothing: {:?}",
        fixture.paths()
    );

    // The next complete walk is allowed to sweep, and still does. The sweep
    // only removes rows older than the run's start second (a crash-safety
    // guard), so let the clock cross one boundary before asking it to.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let outcome = fixture.run(&config, &IndexControl::shared());
    assert_eq!(
        outcome,
        BootstrapOutcome::Complete {
            discovered: 2,
            skipped: 0
        }
    );
    let paths = fixture.paths();
    assert_eq!(paths.len(), 2, "the deleted file is swept: {paths:?}");
    assert!(
        paths.iter().all(|path| !path.ends_with("gone.txt")),
        "only the deleted row is removed"
    );
}

#[test]
fn a_paused_walk_resumes_where_it_stopped() {
    let fixture = Fixture::new("pause-resume");
    let config = fixture.config();
    for index in 0..4 {
        fixture.write(&format!("file-{index}.txt"));
    }

    let control = IndexControl::shared();
    control.pause();

    let worker_control = control.clone();
    let db = fixture.db.clone();
    let worker_config = config.clone();
    let handle = std::thread::spawn(move || {
        QueryEngine::bootstrap_sqlite_controlled_with(
            &db,
            &worker_config,
            BootstrapScope::FILES_ONLY,
            &worker_control,
        )
        .expect("controlled walk")
    });

    // The walk is parked before its first entry. Nothing may be written while
    // paused: a pause that still hammers the disk is not a pause.
    std::thread::sleep(std::time::Duration::from_millis(150));
    assert!(
        !handle.is_finished(),
        "the walk must still be waiting on the control"
    );
    assert_eq!(fixture.paths().len(), 0, "a parked walk writes nothing");

    control.resume();
    let outcome = handle.join().expect("walk thread");
    assert_eq!(
        outcome,
        BootstrapOutcome::Complete {
            discovered: 4,
            skipped: 0
        }
    );
    assert_eq!(fixture.paths().len(), 4, "and finishes the whole walk");
}

#[test]
fn a_folder_rule_prunes_before_descending_and_is_counted() {
    let fixture = Fixture::new("exclusion-count");
    let mut config = fixture.config();
    fixture.write("keep.txt");
    let pruned = fixture.root.join("skipme");
    std::fs::create_dir_all(&pruned).expect("create pruned folder");
    std::fs::write(pruned.join("child.txt"), "x").expect("write child");
    std::fs::write(pruned.join("grandchild.txt"), "x").expect("write grandchild");

    config.set_exclusions(vec![zeidex_engine::exclusions::Exclusion::new(
        "test-folder",
        zeidex_engine::exclusions::ExclusionKind::Folder,
        pruned.to_string_lossy().into_owned(),
        0,
    )]);

    let outcome = fixture.run(&config, &IndexControl::shared());

    assert_eq!(
        outcome.skipped(),
        1,
        "the pruned folder counts once, not once per descendant"
    );
    let paths = fixture.paths();
    assert!(
        paths.iter().any(|path| path.ends_with("keep.txt")),
        "the sibling is still indexed: {paths:?}"
    );
    assert!(
        paths.iter().all(|path| !path.contains("skipme")),
        "nothing under the excluded folder reaches the database: {paths:?}"
    );
}
