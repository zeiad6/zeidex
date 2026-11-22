use crate::BootstrapScope;
use crate::config::RuntimeConfig;
mod apps;
mod files;
mod settings;

pub(crate) use files::{candidate_for_path, compile_ignored_matchers, walk_children};

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::mpsc;
use std::thread;
use zeidex_indexing::{Candidate, CandidateIdKind, IndexControl};

const INDEX_CHANNEL_CAPACITY: usize = 2048;

pub(super) const APP_CANDIDATE_ID_PREFIX: &str = CandidateIdKind::PREFIX_APP;
pub(super) const FILE_CANDIDATE_ID_PREFIX: &str = CandidateIdKind::PREFIX_FILE;
pub(super) const FOLDER_CANDIDATE_ID_PREFIX: &str = CandidateIdKind::PREFIX_FOLDER;
pub(super) const SETTINGS_CANDIDATE_ID_PREFIX: &str = CandidateIdKind::PREFIX_SETTING;

pub struct CandidateDiscoveryStream {
    pub rx: mpsc::Receiver<Candidate>,
    producer_handle: thread::JoinHandle<()>,
}

pub fn discover_candidates_stream(config: &RuntimeConfig) -> CandidateDiscoveryStream {
    discover_candidates_stream_scoped(config, BootstrapScope::ALL)
}

/// Like `discover_candidates_stream`, but only spawns the sub-discoveries selected
/// by `scope`. Sources whose flag is `false` are skipped entirely - no walker is
/// spawned and no candidates are emitted for them.
pub fn discover_candidates_stream_scoped(
    config: &RuntimeConfig,
    scope: BootstrapScope,
) -> CandidateDiscoveryStream {
    discover_candidates_stream_controlled(config, scope, None)
}

/// The same stream, with a shared switch the caller can pause or cancel. The
/// switch is checked by the file walker between directory entries and by the
/// apps/settings discovery between sources, so a stop is seen within one entry
/// rather than at the end of the walk.
pub fn discover_candidates_stream_controlled(
    config: &RuntimeConfig,
    scope: BootstrapScope,
    control: Option<Arc<IndexControl>>,
) -> CandidateDiscoveryStream {
    let (tx, rx) = mpsc::sync_channel(INDEX_CHANNEL_CAPACITY);
    let config = config.clone();
    let producer_handle = thread::spawn(move || {
        let files_handle = if scope.files {
            let files_config = config.clone();
            let files_tx = tx.clone();
            let files_control = control.clone();
            Some(thread::spawn(move || {
                files::discover_local_files_and_folders(
                    &files_config,
                    files_tx,
                    files_control.as_deref(),
                );
            }))
        } else {
            None
        };

        if control
            .as_deref()
            .is_some_and(|control| !control.checkpoint())
        {
            drop(tx);
            if let Some(handle) = files_handle
                && let Err(err) = handle.join()
            {
                eprintln!("zeidex index: file worker panicked: {err:?}");
            }
            return;
        }

        if scope.apps {
            apps::discover_installed_apps(&config, tx.clone());
        }
        if scope.settings {
            settings::discover_system_settings_entries(config.localized_app_names, tx.clone());
        }
        drop(tx);

        if let Some(handle) = files_handle
            && let Err(err) = handle.join()
        {
            eprintln!("zeidex index: file worker panicked: {err:?}");
        }
    });

    CandidateDiscoveryStream {
        rx,
        producer_handle,
    }
}

impl CandidateDiscoveryStream {
    pub fn into_parts(self) -> (mpsc::Receiver<Candidate>, thread::JoinHandle<()>) {
        (self.rx, self.producer_handle)
    }

    pub fn finish(self) {
        if let Err(err) = self.producer_handle.join() {
            eprintln!("zeidex index: producer worker panicked: {err:?}");
        }
    }
}

pub fn discover_candidates(config: &RuntimeConfig) -> Vec<Candidate> {
    let (rx, producer_handle) = discover_candidates_stream(config).into_parts();
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    // Deduplicate centrally while receiving streamed candidates from all sources.
    for candidate in rx {
        if seen.insert(candidate.id.clone()) {
            out.push(candidate);
        }
    }

    if let Err(err) = producer_handle.join() {
        eprintln!("zeidex index: producer worker panicked: {err:?}");
    }

    out
}
