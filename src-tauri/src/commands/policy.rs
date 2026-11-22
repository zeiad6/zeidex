//! Path capabilities: the answer to "may this command touch that path?".
//!
//! The webview is a page. A command that accepts a path from it and then hands
//! that path to `xdg-open`, `std::fs::read` or the OCR engine is trusting the
//! page to choose what the process touches. Today the page is our own bundle,
//! but a compromised npm dependency, a rendering bug, or any future feature that
//! shows remote content would turn that trust into arbitrary file access with
//! the user's privileges.
//!
//! So a path is readable only when it is one of:
//!
//! * inside a configured scan root — the folders the user asked Zeidex to
//!   index;
//! * a path a native picker or an OS drag-and-drop just handed us;
//! * a file Zeidex itself wrote (an export);
//! * an application path Zeidex owns (its database, config and model folders);
//! * an entry in the index.
//!
//! The last rule is what keeps the launcher working: a search result is by
//! definition a row the walker wrote, and application entries live in
//! `/usr/share/applications`, outside every scan root.
//!
//! Containment is checked on canonical paths, so `..` and a symlink pointing out
//! of a root both resolve to where they really lead and are refused. A path that
//! does not exist is resolved through its parent: if the leaf existed, the
//! canonical branch would have taken it, so a missing leaf cannot be a symlink.

use std::collections::VecDeque;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

/// How many picked paths are remembered. A session opens a handful; the cap
/// exists so a drop of a thousand files cannot grow this without bound. The
/// oldest entries are forgotten first, which is the right trade: the file the
/// user just chose is the one the next command reads.
const MAX_PICKED: usize = 512;

/// Why a path was allowed. Returned for diagnostics and tests, not for the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permit {
    /// Inside a configured scan root.
    Root,
    /// Minted by a native picker, an OS drop, or an export this process wrote.
    Picked,
    /// A path Zeidex owns: database, config, model folders.
    Owned,
}

#[derive(Debug, Default)]
struct Inner {
    /// Scan roots as configured, made absolute. Canonicalised at check time so
    /// a root that is created after the setting is saved starts working without
    /// a restart.
    roots: Vec<PathBuf>,
    /// Paths the user chose in a native dialog, dropped on the window, or that
    /// an export just wrote.
    picked: VecDeque<PathBuf>,
    /// Paths the application owns. Containment applies: the database's WAL and
    /// the model folders under the data directory are covered by their parent.
    owned: Vec<PathBuf>,
}

/// The policy shared by every command. One instance lives in `AppState`.
#[derive(Debug, Default)]
pub struct PathPolicy {
    inner: Mutex<Inner>,
}

impl PathPolicy {
    /// `owned` are the application's own files and folders. They are resolved
    /// once: the paths are computed by the shell, not chosen by the page.
    pub fn new(owned: impl IntoIterator<Item = PathBuf>) -> Self {
        let owned = owned
            .into_iter()
            .filter_map(|path| resolve(&path))
            .collect::<Vec<_>>();
        Self {
            inner: Mutex::new(Inner {
                roots: Vec::new(),
                picked: VecDeque::new(),
                owned,
            }),
        }
    }

    /// Replaces the scan roots. Called at startup and after a settings save.
    ///
    /// A root that cannot be canonicalised yet is kept as an absolute path: it
    /// grants nothing until it exists, and then the check canonicalises it. A
    /// root that is a relative path is resolved against the working directory,
    /// which is what the walker does with it too.
    pub fn set_roots(&self, roots: &[String]) {
        let resolved = roots
            .iter()
            .map(|root| root.trim())
            .filter(|root| !root.is_empty())
            .map(|root| lexical_absolute(Path::new(root)))
            .collect();
        self.lock().roots = resolved;
    }

    /// Records a path the user just chose outside the app's own UI.
    ///
    /// Called from the native pickers, the window drag-drop event and the
    /// exporters. A path that cannot be resolved is not recorded: every read of
    /// it would fail anyway, and a capability for a path that does not exist is
    /// a capability for whatever is created there later.
    pub fn mint(&self, path: &Path) {
        let Some(resolved) = resolve(path) else {
            return;
        };
        let mut inner = self.lock();
        if inner.owned.iter().any(|owned| resolved.starts_with(owned)) {
            return;
        }
        if let Some(index) = inner.picked.iter().position(|picked| picked == &resolved) {
            // Re-insert at the back so the freshest choice is the last evicted.
            inner.picked.remove(index);
        }
        inner.picked.push_back(resolved);
        while inner.picked.len() > MAX_PICKED {
            inner.picked.pop_front();
        }
    }

    /// Returns why `path` is allowed, or `None`.
    ///
    /// `None` is not a verdict on its own: the caller still checks the index,
    /// which lives in SQLite and is not this module's business.
    pub fn permits(&self, path: &Path) -> Option<Permit> {
        let resolved = resolve(path)?;
        let inner = self.lock();
        if inner.picked.iter().any(|picked| picked == &resolved) {
            return Some(Permit::Picked);
        }
        if inner.owned.iter().any(|owned| resolved.starts_with(owned)) {
            return Some(Permit::Owned);
        }
        for root in &inner.roots {
            // Canonicalised here, not when the setting was saved: a root can be
            // a symlink that is re-pointed, and a stale cached target would
            // authorise the old one forever.
            let root = root.canonicalize().unwrap_or_else(|_| root.clone());
            if resolved.starts_with(&root) {
                return Some(Permit::Root);
            }
        }
        None
    }

    /// How many paths the pickers have minted. Exposed for tests and for a
    /// future diagnostics screen; nothing in the app depends on the number.
    #[cfg(test)]
    fn picked_len(&self) -> usize {
        self.lock().picked.len()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// The real location of `path`, whether or not it exists.
///
/// An existing path is canonicalised, which resolves `..` and every symlink. A
/// missing path is resolved through its parent and keeps its final component:
/// the leaf cannot be a symlink (if it were, it would exist and the first branch
/// would have run), so the parent's real location decides where it points.
fn resolve(path: &Path) -> Option<PathBuf> {
    if let Ok(canonical) = path.canonicalize() {
        return Some(canonical);
    }
    let name = path.file_name()?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    let parent = parent.map_or_else(|| Path::new(".").to_path_buf(), Path::to_path_buf);
    parent.canonicalize().ok().map(|parent| parent.join(name))
}

/// `path` made absolute and with `.`/`..` removed lexically.
///
/// This is only used for a scan root before it is canonicalised. Resolving `..`
/// without touching the filesystem can disagree with the kernel when a symlink
/// is involved, which is exactly why the containment check never trusts this
/// form for the candidate: it canonicalises both sides first.
fn lexical_absolute(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::{PathPolicy, Permit, lexical_absolute};
    use std::path::{Path, PathBuf};

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "zeidex-policy-{name}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|elapsed| elapsed.as_nanos())
                    .unwrap_or_default()
            ));
            std::fs::create_dir_all(root.join("scan")).expect("scan root");
            std::fs::create_dir_all(root.join("outside")).expect("outside dir");
            std::fs::write(root.join("scan/report.pdf"), b"pdf").expect("inside file");
            std::fs::write(root.join("outside/secret.txt"), b"secret").expect("outside file");
            Self { root }
        }

        fn policy(&self) -> PathPolicy {
            PathPolicy::new([self.root.join("app-data")])
        }

        fn scan(&self) -> String {
            self.root.join("scan").to_string_lossy().into_owned()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn a_file_inside_a_scan_root_is_readable_and_one_beside_it_is_not() {
        let fixture = Fixture::new("roots");
        let policy = fixture.policy();
        policy.set_roots(&[fixture.scan()]);

        assert_eq!(
            policy.permits(&fixture.root.join("scan/report.pdf")),
            Some(Permit::Root)
        );
        assert_eq!(
            policy.permits(&fixture.root.join("scan")),
            Some(Permit::Root)
        );
        assert_eq!(
            policy.permits(&fixture.root.join("outside/secret.txt")),
            None
        );
        assert_eq!(policy.permits(&fixture.root), None);
    }

    #[test]
    fn a_path_that_climbs_out_of_a_root_with_dot_dot_is_refused() {
        let fixture = Fixture::new("traversal");
        let policy = fixture.policy();
        policy.set_roots(&[fixture.scan()]);

        // Lexically this starts with the root, but it resolves beside it.
        let escape = fixture.root.join("scan/../outside/secret.txt");
        assert_eq!(policy.permits(&escape), None);
        assert_eq!(policy.permits(Path::new("/etc/passwd")), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_pointing_out_of_a_root_is_refused() {
        let fixture = Fixture::new("symlink");
        let policy = fixture.policy();
        policy.set_roots(&[fixture.scan()]);
        let link = fixture.root.join("scan/shortcut");
        std::os::unix::fs::symlink(fixture.root.join("outside"), &link).expect("symlink");

        assert_eq!(
            policy.permits(&link.join("secret.txt")),
            None,
            "a symlink must not lend its target the root's authority"
        );
        assert_eq!(policy.permits(&link), None);
    }

    #[test]
    fn a_picked_path_is_readable_even_outside_every_root() {
        let fixture = Fixture::new("picked");
        let policy = fixture.policy();
        policy.set_roots(&[fixture.scan()]);
        let chosen = fixture.root.join("outside/secret.txt");
        assert_eq!(policy.permits(&chosen), None);

        policy.mint(&chosen);
        assert_eq!(policy.permits(&chosen), Some(Permit::Picked));
        // The capability is for that path, not for its neighbours.
        assert_eq!(policy.permits(&fixture.root.join("outside")), None);
    }

    #[test]
    fn minting_the_same_path_twice_does_not_grow_the_set() {
        let fixture = Fixture::new("dedupe");
        let policy = fixture.policy();
        let chosen = fixture.root.join("outside/secret.txt");
        policy.mint(&chosen);
        policy.mint(&chosen);
        policy.mint(&fixture.root.join("outside/../outside/secret.txt"));
        assert_eq!(policy.picked_len(), 1);
    }

    #[test]
    fn the_oldest_pick_is_forgotten_before_the_newest() {
        let fixture = Fixture::new("eviction");
        let policy = fixture.policy();
        for index in 0..(super::MAX_PICKED + 5) {
            let path = fixture.root.join(format!("outside/file-{index}.txt"));
            std::fs::write(&path, b"x").expect("file");
            policy.mint(&path);
        }
        assert_eq!(policy.picked_len(), super::MAX_PICKED);
        let oldest = fixture.root.join("outside/file-0.txt");
        assert_eq!(
            policy.permits(&oldest),
            None,
            "the oldest capability is the one to drop"
        );
        let newest = fixture
            .root
            .join(format!("outside/file-{}.txt", super::MAX_PICKED + 4));
        assert_eq!(policy.permits(&newest), Some(Permit::Picked));
    }

    #[test]
    fn an_application_owned_path_needs_no_pick_and_no_root() {
        let fixture = Fixture::new("owned");
        std::fs::create_dir_all(fixture.root.join("app-data")).expect("app data");
        let policy = fixture.policy();
        let database = fixture.root.join("app-data/zeidex.db");
        std::fs::write(&database, b"db").expect("database");

        assert_eq!(policy.permits(&database), Some(Permit::Owned));
        // Owned containment covers siblings that appear later (WAL, models).
        let wal = fixture.root.join("app-data/zeidex.db-wal");
        std::fs::write(&wal, b"wal").expect("wal");
        assert_eq!(policy.permits(&wal), Some(Permit::Owned));
        assert_eq!(
            policy.permits(&fixture.root.join("outside/secret.txt")),
            None
        );
    }

    #[test]
    fn a_missing_path_is_resolved_through_its_parent() {
        let fixture = Fixture::new("missing");
        let policy = fixture.policy();
        policy.set_roots(&[fixture.scan()]);
        let not_yet = fixture.root.join("scan/new-export.xlsx");
        assert!(
            !not_yet.exists(),
            "the fixture must not create the file it is testing"
        );
        assert_eq!(policy.permits(&not_yet), Some(Permit::Root));
        // And a missing path outside every root is still refused.
        assert_eq!(
            policy.permits(&fixture.root.join("outside/new-file.txt")),
            None
        );
    }

    #[test]
    fn a_root_that_does_not_exist_yet_grants_nothing() {
        let fixture = Fixture::new("late-root");
        let policy = fixture.policy();
        let late = fixture.root.join("created-later");
        policy.set_roots(&[late.to_string_lossy().into_owned()]);
        assert_eq!(policy.permits(&fixture.root.join("scan/report.pdf")), None);

        std::fs::create_dir_all(&late).expect("late root");
        std::fs::write(late.join("note.txt"), b"note").expect("file");
        assert_eq!(
            policy.permits(&late.join("note.txt")),
            Some(Permit::Root),
            "a root created after the setting was saved must start working"
        );
    }

    #[test]
    fn a_relative_root_is_made_absolute() {
        let cwd = std::env::current_dir().expect("cwd");
        assert_eq!(lexical_absolute(Path::new("docs")), cwd.join("docs"));
        assert_eq!(
            lexical_absolute(Path::new("docs/../notes")),
            cwd.join("notes")
        );
        assert_eq!(lexical_absolute(&cwd.join("a/b/../../c")), cwd.join("c"));
    }
}
