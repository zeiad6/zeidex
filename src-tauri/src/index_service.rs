//! The one writer of the search index.
//!
//! Everything that can change the index - the startup walk, the filesystem
//! watcher and the manual Reindex button - funnels through this worker thread,
//! so two full walks cannot race and the SQLite writes stay serialized. The
//! worker reloads the in-memory engine and emits `zeidex://index-status` after
//! every change, which is how the UI shows the real count instead of a number
//! it hoped for.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use zeidex_engine::config::RuntimeConfig;
use zeidex_engine::{BootstrapOutcome, BootstrapScope, QueryEngine};
use zeidex_indexing::{IndexControl, IndexRun};

use crate::index_watch::WatchHandle;
use crate::state::AppState;

/// Emitted after every refresh. The UI adopts `count` instead of guessing.
#[derive(Clone, Debug, Serialize)]
pub struct IndexStatus {
    pub indexing: bool,
    /// True while the running walk is parked on the user's request. The UI uses
    /// it to show Resume instead of Pause, and the footer to say why the count
    /// stopped moving.
    pub paused: bool,
    pub count: usize,
    pub error: Option<String>,
}

pub(crate) enum Request {
    /// Re-walk every source and prune what is gone.
    Full {
        reply: Option<Sender<Result<usize, String>>>,
    },
    /// Re-walk only installed apps after a shortcut changed. App discovery is
    /// not addressable by path, so this is the bounded alternative to a full
    /// walk when one menu entry appears or disappears.
    AppsOnly,
    /// Apply exactly the paths the watcher reported.
    Paths {
        removed: Vec<PathBuf>,
        added: Vec<PathBuf>,
    },
    /// The watcher's live state changed: a directory stopped being watchable
    /// or a previously failed one recovered. `None` means live updates are
    /// complete again. `generation` identifies the watcher that sent it, so a
    /// message queued by a replaced watcher cannot overwrite the new state.
    WatchWarning {
        generation: u64,
        warning: Option<String>,
    },
}

/// Flags the worker and the command layer share. Grouped so the worker takes
/// one argument instead of five that always travel together.
#[derive(Clone)]
struct WorkerShared {
    built: Arc<AtomicBool>,
    full_pending: Arc<AtomicBool>,
    rerun: Arc<AtomicBool>,
    /// True only between the start and the end of a full walk. A pause or a
    /// cancel asked while nothing is walking must report "no walk", not flip a
    /// switch that would park the next one.
    walking: Arc<AtomicBool>,
    control: Arc<IndexControl>,
}

/// A clonable handle to the worker. Cloning shares the same channel and flags.
#[derive(Clone)]
pub struct IndexService {
    tx: Sender<Request>,
    /// A full walk has completed in this session, so a lazy startup has
    /// something to search. Cleared when index settings change.
    built: Arc<AtomicBool>,
    /// `lazy_indexing_enabled`: walk on first use instead of at startup.
    lazy: Arc<AtomicBool>,
    /// A full walk is queued or running; `on_window_shown` must not enqueue a
    /// second one for the same open.
    full_pending: Arc<AtomicBool>,
    /// A full walk was requested while one was queued or running. The worker
    /// runs again when it finishes, because a settings save during a walk
    /// changed the config that walk had already read.
    rerun: Arc<AtomicBool>,
    /// The pause/cancel switch shared with the walker and its writer.
    control: Arc<IndexControl>,
    /// True while a full walk is actually running (see `WorkerShared`).
    walking: Arc<AtomicBool>,
}

impl IndexService {
    pub fn start(app: AppHandle) -> Self {
        let (tx, rx) = mpsc::channel();
        let built = Arc::new(AtomicBool::new(false));
        let lazy = Arc::new(AtomicBool::new(
            RuntimeConfig::load_cached().lazy_indexing_enabled,
        ));
        let full_pending = Arc::new(AtomicBool::new(false));
        let rerun = Arc::new(AtomicBool::new(false));
        let control = IndexControl::shared();
        let walking = Arc::new(AtomicBool::new(false));

        let worker_tx = tx.clone();
        let shared = WorkerShared {
            built: built.clone(),
            full_pending: full_pending.clone(),
            rerun: rerun.clone(),
            walking: walking.clone(),
            control: control.clone(),
        };
        if let Err(err) = thread::Builder::new()
            .name("zeidex-index".to_string())
            .spawn(move || {
                run_worker(app, rx, worker_tx, shared);
            })
        {
            // The launcher still works from the rows already in the database;
            // only refreshing is lost. Reported, never silent.
            eprintln!("zeidex: could not start the index worker: {err}");
        }

        Self {
            tx,
            built,
            lazy,
            full_pending,
            rerun,
            control,
            walking,
        }
    }

    /// Startup policy. Eager mode walks now; lazy mode waits for the window,
    /// which is what the Settings toggle promises.
    pub fn start_initial_index(&self) {
        if !self.lazy.load(Ordering::Acquire) {
            self.request_full();
        }
    }

    /// Called by `show_main`, the single place every route (hotkey, tray,
    /// first launch, macOS reopen) funnels through.
    pub fn on_window_shown(&self) {
        if self.lazy.load(Ordering::Acquire) && !self.built.load(Ordering::Acquire) {
            self.request_full();
        }
    }

    /// Index-affecting settings were saved. The caller has already invalidated
    /// the config cache, so the worker's `load_cached` sees the new roots and
    /// depth. A full walk is the honest response: a changed depth or root can
    /// add and remove rows that no single event mentions.
    pub fn index_settings_changed(&self) {
        let lazy = RuntimeConfig::load_cached().lazy_indexing_enabled;
        self.lazy.store(lazy, Ordering::Release);
        self.built.store(false, Ordering::Release);
        self.request_full();
    }

    /// Park the running walk after its current entry. Returns whether a walk
    /// was actually running, so the UI can say "already paused" instead of
    /// pretending the click did something.
    pub fn pause(&self) -> bool {
        self.walking.load(Ordering::Acquire) && self.control.pause()
    }

    pub fn resume(&self) -> bool {
        self.control.resume()
    }

    /// Stop the walk. The database writer skips its stale-row sweep, so the
    /// index keeps every row the walk did not reach.
    pub fn cancel(&self) -> bool {
        if !self.walking.load(Ordering::Acquire) {
            return false;
        }
        // A cancel must not be immediately followed by the queued rerun: that
        // would restart the walk the user just stopped.
        self.rerun.store(false, Ordering::Release);
        self.control.cancel()
    }

    /// The live switch state. `None` means no walk is running, which is not the
    /// same as "running": a UI that mounts while idle must not offer a pause.
    pub fn control_state(&self) -> Option<IndexRun> {
        self.walking
            .load(Ordering::Acquire)
            .then(|| self.control.state())
    }

    fn request_full(&self) {
        if !claim_full(&self.full_pending, &self.rerun) {
            return;
        }
        if self.tx.send(Request::Full { reply: None }).is_err() {
            self.full_pending.store(false, Ordering::Release);
        }
    }

    /// Blocks the calling (blocking-pool) thread until the walk completes. Used
    /// by the `bootstrap_index` command, which must report the real count or the
    /// real error.
    pub fn full_index_blocking(&self) -> Result<usize, String> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.full_pending.store(true, Ordering::Release);
        self.tx
            .send(Request::Full {
                reply: Some(reply_tx),
            })
            .map_err(|_| "the index worker is not running".to_string())?;
        reply_rx
            .recv()
            .map_err(|_| "the index worker stopped before replying".to_string())?
    }
}

/// Claims the right to enqueue a full walk. Returns false when one is already
/// queued or running, in which case the caller sets `rerun`: the running walk
/// may have read the config before the caller changed it.
fn claim_full(pending: &AtomicBool, rerun: &AtomicBool) -> bool {
    if pending.swap(true, Ordering::AcqRel) {
        rerun.store(true, Ordering::Release);
        return false;
    }
    true
}

/// Ends a full walk. Returns whether another one is owed; when it is, the
/// in-flight marker stays set so the follow-up is not duplicated.
fn finish_full(pending: &AtomicBool, rerun: &AtomicBool) -> bool {
    pending.store(false, Ordering::Release);
    if rerun.swap(false, Ordering::AcqRel) {
        pending.store(true, Ordering::Release);
        true
    } else {
        false
    }
}

fn run_worker(
    app: AppHandle,
    rx: Receiver<Request>,
    request_tx: Sender<Request>,
    shared: WorkerShared,
) {
    let WorkerShared {
        built,
        full_pending,
        rerun,
        walking,
        control,
    } = shared;
    let mut watch: Option<WatchHandle> = None;
    // Why live updates are off or partial, if they are. Kept for the life of
    // the worker and repeated in every status so a warning cannot be missed by
    // a frontend that subscribed after the first event.
    let mut watch_warning: Option<String> = None;
    let mut watch_generation: u64 = 0;
    while let Ok(request) = rx.recv() {
        match request {
            Request::Full { reply } => {
                // A fresh run: whatever the previous run was told (paused or
                // cancelled) no longer applies.
                control.reset();
                walking.store(true, Ordering::Release);
                emit(
                    &app,
                    IndexStatus {
                        indexing: true,
                        paused: false,
                        count: current_count(&app),
                        error: status_error(watch_warning.as_deref(), None),
                    },
                );
                let config = RuntimeConfig::load_cached();
                // Register the watch BEFORE walking. A walk of a large tree
                // takes seconds; a file created behind the walker's cursor
                // would otherwise be invisible until some later event. Events
                // that arrive while this thread walks are queued by the
                // watcher and applied right after. Restarting also picks up
                // changed roots and depth, which is why it happens here and
                // not only when settings change.
                let started = restart_watch(watch.take(), request_tx.clone(), &config);
                watch = started.handle;
                watch_warning = started.warning;
                watch_generation = started.generation;
                let db_path = db_path(&app);
                let result = QueryEngine::bootstrap_sqlite_controlled(
                    &db_path,
                    BootstrapScope::ALL,
                    &control,
                );
                match result {
                    Ok(BootstrapOutcome::Complete { .. }) => {
                        built.store(true, Ordering::Release);
                        let count = reload_count(&app);
                        emit(
                            &app,
                            IndexStatus {
                                indexing: false,
                                paused: false,
                                count,
                                error: status_error(watch_warning.as_deref(), None),
                            },
                        );
                        if let Some(reply) = reply {
                            let _ = reply.send(Ok(count));
                        }
                    }
                    Ok(BootstrapOutcome::Cancelled { discovered }) => {
                        // Not an error: the user asked to stop. The rows already
                        // written stay, and the rows not reached are still in
                        // the database, so `built` is unchanged and the count is
                        // whatever is really there.
                        let count = reload_count(&app);
                        emit(
                            &app,
                            IndexStatus {
                                indexing: false,
                                paused: false,
                                count,
                                error: status_error(watch_warning.as_deref(), None),
                            },
                        );
                        if let Some(reply) = reply {
                            let _ = reply.send(Err(format!(
                                "cancelled after {discovered} items; the index keeps what it had"
                            )));
                        }
                    }
                    Err(err) => {
                        let message = err.to_string();
                        eprintln!("zeidex: index refresh failed: {message}");
                        built.store(false, Ordering::Release);
                        emit(
                            &app,
                            IndexStatus {
                                indexing: false,
                                paused: false,
                                count: current_count(&app),
                                error: status_error(
                                    watch_warning.as_deref(),
                                    Some(message.clone()),
                                ),
                            },
                        );
                        if let Some(reply) = reply {
                            let _ = reply.send(Err(message));
                        }
                    }
                }
                walking.store(false, Ordering::Release);
                if finish_full(&full_pending, &rerun)
                    && request_tx.send(Request::Full { reply: None }).is_err()
                {
                    full_pending.store(false, Ordering::Release);
                }
            }
            Request::AppsOnly => {
                let db_path = db_path(&app);
                let result =
                    QueryEngine::bootstrap_sqlite_scoped(&db_path, BootstrapScope::APPS_ONLY);
                match result {
                    Ok(()) => {
                        let count = reload_count(&app);
                        emit(
                            &app,
                            IndexStatus {
                                indexing: false,
                                paused: false,
                                count,
                                error: status_error(watch_warning.as_deref(), None),
                            },
                        );
                    }
                    Err(err) => {
                        let message = err.to_string();
                        eprintln!("zeidex: apps refresh failed: {message}");
                        emit(
                            &app,
                            IndexStatus {
                                indexing: false,
                                paused: false,
                                count: current_count(&app),
                                error: status_error(watch_warning.as_deref(), Some(message)),
                            },
                        );
                    }
                }
            }
            Request::Paths { removed, added } => {
                let config = RuntimeConfig::load_cached();
                let db_path = db_path(&app);
                match QueryEngine::refresh_paths(&db_path, &config, &removed, &added) {
                    Ok(outcome) => {
                        // Nothing changed: do not repaint the count for an event
                        // that only touched an ignored path.
                        if outcome.removed > 0 || outcome.added > 0 {
                            let count = reload_count(&app);
                            emit(
                                &app,
                                IndexStatus {
                                    indexing: false,
                                    paused: false,
                                    count,
                                    error: status_error(watch_warning.as_deref(), None),
                                },
                            );
                        }
                    }
                    Err(err) => {
                        let message = err.to_string();
                        eprintln!("zeidex: incremental index refresh failed: {message}");
                        emit(
                            &app,
                            IndexStatus {
                                indexing: false,
                                paused: false,
                                count: current_count(&app),
                                error: status_error(watch_warning.as_deref(), Some(message)),
                            },
                        );
                    }
                }
            }
            Request::WatchWarning {
                generation,
                warning,
            } => {
                // Drop a warning queued by a watcher that has since been
                // replaced; its state does not describe the current one.
                if generation != watch_generation {
                    continue;
                }
                // The watcher retried a directory and either failed again with
                // a different set or recovered. Emit only on a change: this
                // channel is shared with path deltas and must not spam.
                if watch_warning != warning {
                    watch_warning = warning;
                    emit(
                        &app,
                        IndexStatus {
                            indexing: false,
                            paused: control.state() == IndexRun::Paused,
                            count: current_count(&app),
                            error: status_error(watch_warning.as_deref(), None),
                        },
                    );
                }
            }
        }
    }
}

fn restart_watch(
    old: Option<WatchHandle>,
    requests: Sender<Request>,
    config: &RuntimeConfig,
) -> crate::index_watch::WatchStart {
    drop(old);
    crate::index_watch::start(requests, config)
}

/// One message for the status event: an operation error and a standing watcher
/// warning can both be true, and losing either is how a failure becomes silent.
fn status_error(watch_warning: Option<&str>, operation: Option<String>) -> Option<String> {
    match (watch_warning, operation) {
        (None, None) => None,
        (Some(warning), None) => Some(warning.to_string()),
        (None, Some(error)) => Some(error),
        (Some(warning), Some(error)) => Some(format!("{error} ({warning})")),
    }
}

fn db_path(app: &AppHandle) -> PathBuf {
    app.state::<AppState>().db_path.clone()
}

fn current_count(app: &AppHandle) -> usize {
    let state = app.state::<AppState>();
    let engine = state
        .engine
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    engine.candidate_count()
}

fn reload_count(app: &AppHandle) -> usize {
    let state = app.state::<AppState>();
    state.reload_engine();
    current_count(app)
}

fn emit(app: &AppHandle, status: IndexStatus) {
    if let Err(err) = app.emit("zeidex://index-status", status) {
        eprintln!("zeidex: could not emit the index status: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::{claim_full, finish_full, status_error};
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn a_standing_watch_warning_reaches_the_status() {
        assert_eq!(status_error(None, None), None);
        assert_eq!(
            status_error(Some("live index updates are off"), None).as_deref(),
            Some("live index updates are off")
        );
        assert_eq!(
            status_error(None, Some("database is busy".to_string())).as_deref(),
            Some("database is busy")
        );
        let both = status_error(Some("partial watch"), Some("database is busy".to_string()))
            .expect("both messages survive");
        assert!(both.contains("database is busy"));
        assert!(both.contains("partial watch"));
    }

    #[test]
    fn a_second_request_during_a_walk_is_remembered_not_dropped() {
        let pending = AtomicBool::new(false);
        let rerun = AtomicBool::new(false);

        assert!(claim_full(&pending, &rerun), "first request is enqueued");
        assert!(!claim_full(&pending, &rerun), "second is coalesced");
        assert!(rerun.load(Ordering::Acquire), "but not forgotten");
        assert!(
            finish_full(&pending, &rerun),
            "the walk asks for a follow-up"
        );
        assert!(
            pending.load(Ordering::Acquire),
            "the follow-up is in flight"
        );
        assert!(!finish_full(&pending, &rerun), "and is the last one");
    }

    #[test]
    fn an_uncontended_walk_does_not_rerun() {
        let pending = AtomicBool::new(false);
        let rerun = AtomicBool::new(false);
        assert!(claim_full(&pending, &rerun));
        assert!(!finish_full(&pending, &rerun));
        assert!(!pending.load(Ordering::Acquire));
    }
}
