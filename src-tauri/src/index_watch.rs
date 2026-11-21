//! Filesystem watching for the two sources that change while the app runs.
//!
//! File roots are addressable, so an event becomes an incremental
//! `Request::Paths`. App discovery is not addressable by path (a menu entry is
//! parsed, not walked), so an app-root event becomes the bounded
//! `Request::AppsOnly` walk. Settings entries are static and are not watched.
//!
//! Events are debounced for two seconds: a save-then-rename or an editor's
//! temp-file dance arrives as a burst, and the index should see the settled
//! tree once, not every intermediate state. The first event of a burst sets the
//! deadline; later events never extend it, so a busy directory cannot starve
//! updates forever.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use notify::event::{ModifyKind, RemoveKind};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use zeidex_engine::config::RuntimeConfig;
use zeidex_engine::path_is_same_or_child;

use crate::index_service::Request;

/// Let a burst settle before touching the index.
const DEBOUNCE: Duration = Duration::from_secs(2);
/// A wake-up with no events; long enough to cost nothing, short enough that a
/// dropped control sender is noticed without a busy loop. The `Stop` message
/// wakes the thread immediately; this is the fallback.
const IDLE_WAIT: Duration = Duration::from_secs(3600);

enum Message {
    Event(notify::Result<Event>),
    Stop,
}

/// Keeps the watcher thread alive. Dropping it stops the watch.
pub(crate) struct WatchHandle {
    control: Sender<Message>,
    join: Option<thread::JoinHandle<()>>,
}

impl Drop for WatchHandle {
    fn drop(&mut self) {
        let _ = self.control.send(Message::Stop);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

/// Paths one debounce window accumulated.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Delta {
    pub apps: bool,
    pub removed: Vec<PathBuf>,
    pub added: Vec<PathBuf>,
}

/// The configured roots and the scan depth that applies to each, in one value.
/// Both the event classifier and the watch-set maintenance need all four; as
/// separate arguments they made every caller's signature unreadable.
struct Roots {
    apps: Vec<String>,
    files: Vec<String>,
    app_depth: usize,
    file_depth: usize,
}

/// Result of trying to start the watcher.
///
/// `warning` is user-facing: it explains why live updates are off or partial.
/// The caller puts it in every index-status event, so a desktop launch does not
/// lose the reason to a stderr nobody sees.
pub(crate) struct WatchStart {
    pub handle: Option<WatchHandle>,
    pub warning: Option<String>,
}

pub(crate) fn start(requests: Sender<Request>, config: &RuntimeConfig) -> WatchStart {
    let mut app_roots = config.app_scan_roots.clone();
    app_roots.sort();
    app_roots.dedup();
    let mut file_roots = config.file_scan_roots.clone();
    file_roots.extend(config.file_scan_extra_roots.iter().cloned());
    file_roots.sort();
    file_roots.dedup();

    // Watch exactly the directories the scan can reach, not the whole tree.
    // `RecursiveMode::Recursive` on an app root would descend into every `.app`
    // bundle on macOS (hundreds of thousands of directories), and on Linux a
    // home-directory file root can exceed the inotify watch limit. Depth is
    // bounded by the same settings the walk uses, so the watch set can never be
    // an order of magnitude larger than the index it serves.
    let app_depth = config.app_scan_depth;
    let file_depth = config.file_scan_depth;
    let mut targets: Vec<PathBuf> = Vec::new();
    for root in app_roots.iter().filter(|root| !root.trim().is_empty()) {
        targets.extend(watch_targets(root, app_depth, true));
    }
    for root in file_roots.iter().filter(|root| !root.trim().is_empty()) {
        targets.extend(watch_targets(root, file_depth, false));
    }
    targets.sort();
    targets.dedup();
    if targets.is_empty() {
        return WatchStart {
            handle: None,
            warning: None,
        };
    }

    let (control, rx) = mpsc::channel();
    let event_tx = control.clone();
    let mut watcher = match RecommendedWatcher::new(
        move |result| {
            let _ = event_tx.send(Message::Event(result));
        },
        notify::Config::default(),
    ) {
        Ok(watcher) => watcher,
        Err(err) => {
            let warning =
                format!("live index updates are off: the file watcher could not start: {err}");
            eprintln!("zeidex: {warning}");
            return WatchStart {
                handle: None,
                warning: Some(warning),
            };
        }
    };

    // Registration runs here, on the caller's thread, so a backend that cannot
    // watch a root is reported before the worker starts emitting status.
    let mut watched: HashSet<PathBuf> = HashSet::new();
    let mut failed = 0usize;
    let mut first_error: Option<String> = None;
    for target in &targets {
        match watcher.watch(target, RecursiveMode::NonRecursive) {
            Ok(()) => {
                watched.insert(target.clone());
            }
            Err(err) => {
                failed += 1;
                if first_error.is_none() {
                    first_error = Some(format!("{}: {err}", target.display()));
                }
                if failed <= 5 {
                    eprintln!("zeidex: could not watch {}: {err}", target.display());
                }
            }
        }
    }
    if failed > 5 {
        eprintln!("zeidex: {failed} watch registrations failed in total");
    }
    let warning = registration_warning(watched.len(), failed, first_error.as_deref());
    if let Some(message) = &warning {
        eprintln!("zeidex: {message}");
    }
    if watched.is_empty() {
        return WatchStart {
            handle: None,
            warning,
        };
    }

    let roots = Roots {
        apps: app_roots,
        files: file_roots,
        app_depth,
        file_depth,
    };
    let join = thread::Builder::new()
        .name("zeidex-watch".to_string())
        .spawn(move || run(rx, watcher, watched, roots, requests));
    match join {
        Ok(join) => WatchStart {
            handle: Some(WatchHandle {
                control,
                join: Some(join),
            }),
            warning,
        },
        Err(err) => {
            let message =
                format!("live index updates are off: the watcher thread could not start: {err}");
            eprintln!("zeidex: {message}");
            let warning = match warning {
                Some(partial) => Some(format!("{partial}; {message}")),
                None => Some(message),
            };
            WatchStart {
                handle: None,
                warning,
            }
        }
    }
}

/// One sentence for the UI when some or all watch registrations failed.
///
/// A partial set is not fatal — the un-watched directories fall back to the
/// manual Reindex — but it is the difference between "live" and "sometimes
/// live", so it is reported rather than hidden behind a successful startup.
fn registration_warning(
    watched: usize,
    failed: usize,
    first_error: Option<&str>,
) -> Option<String> {
    if failed == 0 {
        return None;
    }
    if watched == 0 {
        return Some(match first_error {
            Some(err) => {
                format!("live index updates are off: no index root could be watched ({err})")
            }
            None => "live index updates are off: no index root could be watched".to_string(),
        });
    }
    let directories = if failed == 1 {
        "1 directory could not be watched".to_string()
    } else {
        format!("{failed} directories could not be watched")
    };
    Some(match first_error {
        Some(err) => format!("live index updates are partial: {directories} ({err})"),
        None => format!("live index updates are partial: {directories}"),
    })
}

/// Directories to register, each watched non-recursively.
///
/// `max_depth` counts in scan-depth units: a directory at depth `d` is watched
/// when `d < max_depth`, because an event for an entry at depth `max_depth`
/// arrives from its parent. Hidden directories are skipped (the walk skips
/// them), as are `.app` bundles when watching an app root. Symlinked
/// directories are not followed, which rules out cycles; a file reached only
/// through a symlink is still indexed by the full walk, it just does not get
/// live updates.
fn watch_targets(root: &str, max_depth: usize, skip_app_bundles: bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![(PathBuf::from(root), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        if !dir.is_dir() {
            continue;
        }
        out.push(dir.clone());
        if depth + 1 >= max_depth {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            // `file_type` does not follow symlinks, which is what keeps the
            // traversal finite.
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if name.starts_with('.') || (skip_app_bundles && name.ends_with(".app")) {
                continue;
            }
            stack.push((entry.path(), depth + 1));
        }
    }
    out
}

fn run(
    rx: Receiver<Message>,
    mut watcher: RecommendedWatcher,
    mut watched: HashSet<PathBuf>,
    roots: Roots,
    requests: Sender<Request>,
) {
    let mut pending = Delta::default();
    let mut deadline: Option<Instant> = None;
    loop {
        let timeout = deadline
            .map(|at| at.saturating_duration_since(Instant::now()))
            .unwrap_or(IDLE_WAIT);
        match rx.recv_timeout(timeout) {
            Ok(Message::Stop) => break,
            Ok(Message::Event(Ok(event))) => {
                // Non-recursive watches mean a directory created after start
                // has no watch of its own. Register it (and its reachable
                // children) before its own events can be missed; a path that
                // disappeared releases its entries instead.
                sync_watches(&mut watcher, &mut watched, &event.paths, &roots);
                if apply_event(&event, &roots.apps, &roots.files, &mut pending)
                    && deadline.is_none()
                {
                    deadline = Some(Instant::now() + DEBOUNCE);
                }
            }
            Ok(Message::Event(Err(err))) => {
                eprintln!("zeidex: file watcher event error: {err}");
            }
            Err(RecvTimeoutError::Timeout) => {
                if deadline.is_some() {
                    dispatch(&requests, &mut pending);
                    deadline = None;
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

/// Brings the watch set in line with one event batch.
///
/// A directory that exists inside a root has its reachable subtree registered;
/// the depth budget is what the scan still spends below it, so the watch set
/// stays bounded. A path that disappeared releases itself and everything under
/// it, which is what lets a delete-then-recreate be registered again.
fn sync_watches(
    watcher: &mut RecommendedWatcher,
    watched: &mut HashSet<PathBuf>,
    paths: &[PathBuf],
    roots: &Roots,
) {
    for path in paths {
        let Some(path_str) = path.to_str() else {
            continue;
        };
        if !path.exists() {
            watched.retain(|entry| {
                entry
                    .to_str()
                    .is_some_and(|entry_str| !path_is_same_or_child(entry_str, path_str))
            });
            continue;
        }
        if !path.is_dir() {
            continue;
        }
        let (candidates, depth, skip_bundles) = if roots
            .apps
            .iter()
            .any(|root| path_is_same_or_child(path_str, root))
        {
            (&roots.apps, roots.app_depth, true)
        } else {
            (&roots.files, roots.file_depth, false)
        };
        let Some(dir_depth) = candidates
            .iter()
            .filter(|root| path_is_same_or_child(path_str, root))
            .filter_map(|root| relative_depth(path_str, root))
            .min()
        else {
            continue;
        };
        // A directory at scan depth D would only contain entries past the
        // limit; there is nothing under it to watch.
        if dir_depth == 0 || dir_depth >= depth {
            continue;
        }
        for dir in watch_targets(path_str, depth - dir_depth, skip_bundles) {
            if !watched.insert(dir.clone()) {
                continue;
            }
            if let Err(err) = watcher.watch(&dir, RecursiveMode::NonRecursive) {
                watched.remove(&dir);
                eprintln!("zeidex: could not watch {}: {err}", dir.display());
            }
        }
    }
}

/// Components of `path` below `root`, or `None` when it is not below it.
fn relative_depth(path: &str, root: &str) -> Option<usize> {
    let path = Path::new(path);
    let root = Path::new(root);
    path.strip_prefix(root)
        .ok()
        .map(|tail| tail.components().count())
        .or_else(|| {
            let path_len = path.components().count();
            let root_len = root.components().count();
            (path_len > root_len).then_some(path_len - root_len)
        })
}

fn dispatch(requests: &Sender<Request>, pending: &mut Delta) {
    pending.removed.sort();
    pending.removed.dedup();
    pending.added.sort();
    pending.added.dedup();
    if pending.apps {
        let _ = requests.send(Request::AppsOnly);
    }
    if !pending.removed.is_empty() || !pending.added.is_empty() {
        let _ = requests.send(Request::Paths {
            removed: std::mem::take(&mut pending.removed),
            added: std::mem::take(&mut pending.added),
        });
    }
    *pending = Delta::default();
}

/// Folds one event into `delta`. Returns whether it contained anything the
/// index owns, which is what starts (or extends) the debounce window.
pub(crate) fn apply_event(
    event: &Event,
    app_roots: &[String],
    file_roots: &[String],
    delta: &mut Delta,
) -> bool {
    if matches!(event.kind, EventKind::Access(_)) {
        return false;
    }

    let mut changed = false;
    for path in &event.paths {
        let Some(path_str) = path.to_str() else {
            continue;
        };
        if app_roots
            .iter()
            .any(|root| path_is_same_or_child(path_str, root))
        {
            delta.apps = true;
            changed = true;
            continue;
        }
        if !file_roots
            .iter()
            .any(|root| path_is_same_or_child(path_str, root))
        {
            continue;
        }

        match &event.kind {
            EventKind::Remove(RemoveKind::Any)
            | EventKind::Remove(RemoveKind::File)
            | EventKind::Remove(RemoveKind::Folder)
            | EventKind::Remove(RemoveKind::Other) => delta.removed.push(path.clone()),
            EventKind::Create(_) => delta.added.push(path.clone()),
            // Rename reports the old and new path, but some backends report
            // only one side. The tree is the authority: a path that exists is
            // an addition, one that does not is a removal.
            EventKind::Modify(ModifyKind::Name(_)) | EventKind::Modify(_) => {
                if path.exists() {
                    delta.added.push(path.clone());
                } else {
                    delta.removed.push(path.clone());
                }
            }
            EventKind::Any | EventKind::Other => {
                if path.exists() {
                    delta.added.push(path.clone());
                } else {
                    delta.removed.push(path.clone());
                }
            }
            _ => continue,
        }
        changed = true;
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::{Delta, apply_event, registration_warning};
    use notify::event::{CreateKind, ModifyKind, RemoveKind};
    use notify::{Event, EventKind};

    fn event(kind: EventKind, paths: &[&str]) -> Event {
        let mut event = Event::new(kind);
        for path in paths {
            event = event.add_path(std::path::PathBuf::from(path));
        }
        event
    }

    fn roots() -> (Vec<String>, Vec<String>) {
        (
            vec!["/apps".to_string()],
            vec!["/home/u/Documents".to_string()],
        )
    }

    #[test]
    fn a_created_file_under_a_file_root_is_an_addition() {
        let (app_roots, file_roots) = roots();
        let mut delta = Delta::default();
        let changed = apply_event(
            &event(
                EventKind::Create(CreateKind::File),
                &["/home/u/Documents/report.pdf"],
            ),
            &app_roots,
            &file_roots,
            &mut delta,
        );
        assert!(changed);
        assert!(!delta.apps);
        assert_eq!(
            delta.added,
            vec![std::path::PathBuf::from("/home/u/Documents/report.pdf")]
        );
        assert!(delta.removed.is_empty());
    }

    #[test]
    fn a_removed_folder_under_a_file_root_is_a_removal() {
        let (app_roots, file_roots) = roots();
        let mut delta = Delta::default();
        let changed = apply_event(
            &event(
                EventKind::Remove(RemoveKind::Folder),
                &["/home/u/Documents/old"],
            ),
            &app_roots,
            &file_roots,
            &mut delta,
        );
        assert!(changed);
        assert_eq!(
            delta.removed,
            vec![std::path::PathBuf::from("/home/u/Documents/old")]
        );
    }

    #[test]
    fn an_app_root_event_requests_the_scoped_apps_walk() {
        let (app_roots, file_roots) = roots();
        let mut delta = Delta::default();
        let changed = apply_event(
            &event(
                EventKind::Create(CreateKind::File),
                &["/apps/zeidex.desktop"],
            ),
            &app_roots,
            &file_roots,
            &mut delta,
        );
        assert!(changed);
        assert!(delta.apps, "app discovery must re-walk its own source");
        assert!(
            delta.added.is_empty(),
            "an .desktop file is not a file candidate"
        );
    }

    #[test]
    fn paths_outside_every_root_are_ignored() {
        let (app_roots, file_roots) = roots();
        let mut delta = Delta::default();
        let changed = apply_event(
            &event(EventKind::Create(CreateKind::File), &["/var/log/syslog"]),
            &app_roots,
            &file_roots,
            &mut delta,
        );
        assert!(!changed);
        assert_eq!(delta, Delta::default());
    }

    #[test]
    fn access_events_never_touch_the_index() {
        let (app_roots, file_roots) = roots();
        let mut delta = Delta::default();
        let changed = apply_event(
            &event(
                EventKind::Access(notify::event::AccessKind::Read),
                &["/home/u/Documents/report.pdf"],
            ),
            &app_roots,
            &file_roots,
            &mut delta,
        );
        assert!(!changed);
        assert_eq!(delta, Delta::default());
    }

    #[test]
    fn a_partial_watch_is_reported_with_the_failure_count() {
        let warning = registration_warning(3, 2, Some("/root: permission denied"))
            .expect("partial registration must warn");
        assert!(warning.starts_with("live index updates are partial"));
        assert!(warning.contains("2 directories could not be watched"));
        assert!(warning.contains("/root: permission denied"));

        let one = registration_warning(3, 1, None).expect("one failure must warn");
        assert!(one.contains("1 directory could not be watched"), "{one}");
    }

    #[test]
    fn no_watchable_root_reports_live_updates_are_off() {
        let warning = registration_warning(0, 4, Some("/root: no such file"))
            .expect("zero watched roots must warn");
        assert!(warning.starts_with("live index updates are off"));
        assert!(warning.contains("no index root could be watched"));
    }

    #[test]
    fn a_clean_watch_has_no_warning() {
        assert_eq!(registration_warning(12, 0, None), None);
    }

    #[test]
    fn watch_targets_stop_at_the_scan_depth() {
        let dir = std::env::temp_dir().join(format!(
            "zeidex-watch-depth-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(dir.join("one/two/three")).expect("create tree");

        let root = dir.to_string_lossy().into_owned();
        let shallow = super::watch_targets(&root, 1, false);
        assert_eq!(shallow, vec![dir.clone()], "depth 1 watches only the root");

        let deep = super::watch_targets(&root, 4, false);
        assert!(deep.contains(&dir));
        assert!(deep.contains(&dir.join("one")));
        assert!(deep.contains(&dir.join("one/two")));
        assert!(deep.contains(&dir.join("one/two/three")));
        assert_eq!(deep.len(), 4, "depth 4 watches four levels: {deep:?}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn watch_targets_skip_hidden_dirs_and_app_bundles() {
        let dir = std::env::temp_dir().join(format!(
            "zeidex-watch-skip-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(dir.join(".git/objects")).expect("create hidden dir");
        std::fs::create_dir_all(dir.join("Safari.app/Contents/MacOS")).expect("create bundle");
        std::fs::create_dir_all(dir.join("Utilities")).expect("create plain dir");

        let root = dir.to_string_lossy().into_owned();
        let app = super::watch_targets(&root, 4, true);
        assert!(app.contains(&dir.join("Utilities")));
        assert!(
            !app.iter()
                .any(|path| path.to_string_lossy().contains(".app")),
            "an app root must not watch inside bundles: {app:?}"
        );
        assert!(
            !app.iter()
                .any(|path| path.to_string_lossy().contains(".git"))
        );

        // File roots may legitimately contain folders that end in `.app`, so the
        // bundle skip is only applied to app roots.
        let files = super::watch_targets(&root, 4, false);
        assert!(
            files
                .iter()
                .any(|path| path.to_string_lossy().contains(".app"))
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_rename_reports_an_addition_when_the_new_path_exists() {
        let dir = std::env::temp_dir().join(format!(
            "zeidex-watch-rename-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let file_root = dir.to_string_lossy().into_owned();
        let new_path = dir.join("after.txt");
        std::fs::write(&new_path, "x").expect("create renamed file");
        let new_path_str = new_path.to_string_lossy().into_owned();

        let mut delta = Delta::default();
        let changed = apply_event(
            &event(
                EventKind::Modify(ModifyKind::Name(notify::event::RenameMode::Any)),
                &[&new_path_str],
            ),
            &[],
            std::slice::from_ref(&file_root),
            &mut delta,
        );
        assert!(changed);
        assert_eq!(delta.added, vec![new_path]);
        assert!(delta.removed.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
