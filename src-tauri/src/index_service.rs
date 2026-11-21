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
use zeidex_engine::{BootstrapScope, QueryEngine};

use crate::index_watch::WatchHandle;
use crate::state::AppState;

/// Emitted after every refresh. The UI adopts `count` instead of guessing.
#[derive(Clone, Debug, Serialize)]
pub struct IndexStatus {
    pub indexing: bool,
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

        let worker_tx = tx.clone();
        let worker_built = built.clone();
        let worker_pending = full_pending.clone();
        let worker_rerun = rerun.clone();
        if let Err(err) = thread::Builder::new()
            .name("zeidex-index".to_string())
            .spawn(move || {
                run_worker(
                    app,
                    rx,
                    worker_tx,
                    worker_built,
                    worker_pending,
                    worker_rerun,
                );
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
    built: Arc<AtomicBool>,
    full_pending: Arc<AtomicBool>,
    rerun: Arc<AtomicBool>,
) {
    let mut watch: Option<WatchHandle> = None;
    while let Ok(request) = rx.recv() {
        match request {
            Request::Full { reply } => {
                emit(
                    &app,
                    IndexStatus {
                        indexing: true,
                        count: current_count(&app),
                        error: None,
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
                watch = restart_watch(watch.take(), request_tx.clone(), &config);
                let db_path = db_path(&app);
                let result = QueryEngine::bootstrap_sqlite(&db_path);
                match result {
                    Ok(()) => {
                        built.store(true, Ordering::Release);
                        let count = reload_count(&app);
                        emit(
                            &app,
                            IndexStatus {
                                indexing: false,
                                count,
                                error: None,
                            },
                        );
                        if let Some(reply) = reply {
                            let _ = reply.send(Ok(count));
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
                                count: current_count(&app),
                                error: Some(message.clone()),
                            },
                        );
                        if let Some(reply) = reply {
                            let _ = reply.send(Err(message));
                        }
                    }
                }
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
                                count,
                                error: None,
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
                                count: current_count(&app),
                                error: Some(message),
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
                                    count,
                                    error: None,
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
                                count: current_count(&app),
                                error: Some(message),
                            },
                        );
                    }
                }
            }
        }
    }
}

fn restart_watch(
    old: Option<WatchHandle>,
    requests: Sender<Request>,
    config: &RuntimeConfig,
) -> Option<WatchHandle> {
    drop(old);
    crate::index_watch::start(requests, config)
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
    use super::{claim_full, finish_full};
    use std::sync::atomic::{AtomicBool, Ordering};

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
