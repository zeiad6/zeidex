use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tauri::Manager;
use zeidex_engine::QueryEngine;
use zeidex_models::ModelPack;
use zeidex_ocr::OcrEngine;
use zeidex_storage::SqliteStore;

/// One in-flight model download. Downloads are serialised: two packs writing
/// staging folders at once would compete for the same disk and the same
/// progress channel, and the UI has no use for that.
pub struct ModelDownload {
    pub pack_id: String,
    pub cancel: Arc<AtomicBool>,
}

/// Shared shell state. Both fields are behind a mutex because Tauri commands run
/// on the async runtime while the indexing engine is synchronous and CPU-bound.
pub struct AppState {
    pub engine: Mutex<QueryEngine>,
    pub store: Mutex<SqliteStore>,
    /// Last text seen by the clipboard poller. Clipboard APIs have no change
    /// event, so the shell polls; without this guard every poll would re-record
    /// the same clip and inflate its copy count.
    ///
    /// Lock order: `last_clipboard` is always taken before `store`, never after.
    /// The poller holds it from the pasteboard read through the write, so a
    /// pause, a resume or a history wipe cannot interleave with it.
    pub last_clipboard: Mutex<Option<String>>,
    /// Set when a resume could not read the pasteboard. The next clip the poller
    /// sees is taken as the watermark and not written: without the current
    /// value it cannot tell a clip copied before the resume from one copied
    /// after, and losing one clip is the safe side of that doubt.
    pub clipboard_skip_next: std::sync::atomic::AtomicBool,
    /// Lazily loaded OCR engine. `OcrEngine` is `Send + Sync`, so the `Arc` is
    /// cloned into blocking tasks.
    pub ocr: Mutex<Option<std::sync::Arc<OcrEngine>>>,
    /// Cancellation token of the recognition run that owns the Studio, if any.
    ///
    /// The Studio runs one document at a time, so a single slot is enough. The
    /// token is compared by pointer when a run ends: a command that finished
    /// late must not clear the run that replaced it.
    pub ocr_run: Mutex<Option<Arc<AtomicBool>>>,
    /// The bundled model folder: shipped with the installer, read-only in a
    /// packaged app.
    pub models_dir: PathBuf,
    /// The writable model folder: downloaded packs and the active-pack marker
    /// live here.
    pub models_root: PathBuf,
    /// The download that currently owns the progress channel, if any.
    pub model_download: Mutex<Option<ModelDownload>>,
    pub pdfium_library: Option<PathBuf>,
    pub db_path: PathBuf,
}

impl AppState {
    pub fn new(
        db_path: PathBuf,
        models_dir: PathBuf,
        models_root: PathBuf,
        pdfium_library: Option<PathBuf>,
    ) -> anyhow::Result<Self> {
        // A corrupt database file used to abort startup entirely. Recovery keeps
        // the unreadable file beside the fresh one instead of failing the app.
        let mut store = SqliteStore::open_or_recover(&db_path)?;
        // The generated config documents this limit, so honour it. Clamped again
        // inside the store, which is the layer that actually prunes. Pruning runs
        // here, at startup, so a lowered limit applies to rows from last session
        // too.
        let config = zeidex_engine::config::RuntimeConfig::load_cached();
        // A retention limit that cannot be applied must not stop the launcher
        // from starting: the new cap is stored before the prune runs, so every
        // later copy still enforces it, and the failure is reported rather than
        // hidden. Nothing has been deleted, so there is nothing to lose.
        if let Err(err) = store.set_clipboard_text_cap(config.clipboard_history_limit) {
            eprintln!("zeidex: could not apply the clipboard history limit: {err}");
        }
        // Age retention is opt-in (0 = off). Applied at startup for the same
        // reason as the size cap: a user who enables it wants yesterday's clips
        // gone now, not after their next copy. A failure is reported, not fatal.
        if let Err(err) = store.set_clipboard_retention_days(config.clipboard_retention_days) {
            eprintln!("zeidex: could not apply the clipboard retention limit: {err}");
        }
        store.set_clipboard_filter_secrets(config.clipboard_filter_secrets);
        // A failure here means the index could not be read even though the store
        // opened. Starting empty keeps the launcher usable, but the reason must
        // not vanish: without it, "no results" is the only symptom.
        let engine = QueryEngine::from_sqlite(&db_path).unwrap_or_else(|err| {
            eprintln!("zeidex: could not load the search index: {err}");
            QueryEngine::default()
        });
        Ok(Self {
            engine: Mutex::new(engine),
            store: Mutex::new(store),
            last_clipboard: Mutex::new(None),
            clipboard_skip_next: std::sync::atomic::AtomicBool::new(false),
            ocr: Mutex::new(None),
            ocr_run: Mutex::new(None),
            models_dir,
            models_root,
            model_download: Mutex::new(None),
            pdfium_library,
            db_path,
        })
    }

    /// Re-reads the candidate table after a bootstrap or watcher refresh.
    pub fn reload_engine(&self) {
        if let Ok(engine) = QueryEngine::from_sqlite(&self.db_path)
            && let Ok(mut guard) = self.engine.lock()
        {
            *guard = engine;
        }
    }

    /// Starts a recognition run and returns the token that can stop it.
    pub fn begin_ocr_run(&self) -> Arc<AtomicBool> {
        let token = Arc::new(AtomicBool::new(false));
        let mut guard = self
            .ocr_run
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = Some(token.clone());
        token
    }

    /// Releases the slot for `token`, leaving a newer run's token alone.
    pub fn end_ocr_run(&self, token: &Arc<AtomicBool>) {
        let mut guard = self
            .ocr_run
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if guard
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, token))
        {
            *guard = None;
        }
    }

    /// Asks the live run to stop between pages. Returns whether one was live.
    pub fn cancel_ocr_run(&self) -> bool {
        let guard = self
            .ocr_run
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match guard.as_ref() {
            Some(token) => {
                token.store(true, Ordering::Relaxed);
                true
            }
            None => false,
        }
    }

    /// Loads the OCR engine on first use and caches it. Loading five ONNX
    /// sessions is far too slow to do per page.
    pub fn ocr_engine(&self) -> Result<std::sync::Arc<OcrEngine>, zeidex_ocr::OcrError> {
        let mut guard = self
            .ocr
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(engine) = guard.as_ref() {
            return Ok(engine.clone());
        }
        let engine = std::sync::Arc::new(OcrEngine::with_paths(
            self.ocr_paths(),
            zeidex_ocr::OcrEngineConfig {
                models_dir: self.models_dir.clone(),
                pdfium_library: self.pdfium_library.clone(),
                dpi: 300.0,
                ..Default::default()
            },
        )?);
        *guard = Some(engine.clone());
        Ok(engine)
    }

    /// The pack the next engine load should use.
    ///
    /// A marker naming an unknown pack, or one whose files have disappeared,
    /// falls back to the bundled set: a stale preference must not turn OCR off.
    pub fn active_pack(&self) -> &'static ModelPack {
        zeidex_models::active_pack(&self.models_root)
            .and_then(|id| zeidex_models::pack(&id))
            .filter(|pack| {
                pack.bundled || zeidex_models::status(&self.models_root, pack).is_installed()
            })
            .unwrap_or_else(zeidex_models::default_pack)
    }

    /// Model paths for the active pack. Detection and recognition come from the
    /// pack; layout and table stay shared with the bundled folder.
    pub fn ocr_paths(&self) -> zeidex_ocr::onnx::ModelPaths {
        self.ocr_paths_for(self.active_pack())
    }

    /// Model paths for one specific pack, active or not. The benchmark uses
    /// this to test a pack before the user commits to it.
    pub fn ocr_paths_for(&self, pack: &ModelPack) -> zeidex_ocr::onnx::ModelPaths {
        let pack_paths = zeidex_models::pack_paths(&self.models_dir, &self.models_root, pack);
        let mut paths = zeidex_ocr::onnx::ModelPaths::from_dir(&self.models_dir);
        paths.det = pack_paths.det;
        paths.rec_en = pack_paths.rec_en;
        paths.rec_ar = pack_paths.rec_ar;
        paths.dict_en = pack_paths.dict_en;
        paths.dict_ar = pack_paths.dict_ar;
        paths.rec_ar_visual = pack_paths.rec_ar_visual;
        paths
    }

    /// Drops the cached engine so the next run loads the newly active pack.
    /// A run already holding an `Arc` finishes on the model it started with.
    pub fn reset_ocr_engine(&self) {
        let mut guard = self
            .ocr
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = None;
    }

    /// Claims the download slot. Returns the token the downloader checks.
    pub fn begin_model_download(&self, pack_id: &str) -> Result<Arc<AtomicBool>, String> {
        let mut guard = self
            .model_download
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(current) = guard.as_ref() {
            return Err(format!(
                "{} is still downloading; cancel it before starting another",
                current.pack_id
            ));
        }
        let token = Arc::new(AtomicBool::new(false));
        *guard = Some(ModelDownload {
            pack_id: pack_id.to_string(),
            cancel: token.clone(),
        });
        Ok(token)
    }

    /// Releases the slot for `token`, leaving a newer download alone.
    pub fn end_model_download(&self, token: &Arc<AtomicBool>) {
        let mut guard = self
            .model_download
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if guard
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(&current.cancel, token))
        {
            *guard = None;
        }
    }

    /// Asks the live download to stop. Returns whether one was live.
    pub fn cancel_model_download(&self) -> bool {
        let guard = self
            .model_download
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match guard.as_ref() {
            Some(current) => {
                current.cancel.store(true, Ordering::Relaxed);
                true
            }
            None => false,
        }
    }

    /// The pack id of the live download, for the UI to show a spinner on the
    /// right card after a remount.
    pub fn downloading_pack(&self) -> Option<String> {
        let guard = self
            .model_download
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.as_ref().map(|current| current.pack_id.clone())
    }
}

/// `$ZEIDEX_DB_PATH` wins (the dev/QA override the macOS shell used); otherwise
/// the per-user data dir. `dirs` resolves the platform convention:
/// `~/.local/share` on Linux, `%APPDATA%` on Windows.
pub fn resolve_db_path() -> PathBuf {
    if let Some(path) = std::env::var_os("ZEIDEX_DB_PATH") {
        let path = PathBuf::from(path);
        if !path.as_os_str().is_empty() {
            return path;
        }
    }
    let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("zeidex").join("zeidex.db")
}

/// `$ZEIDEX_MODELS_DIR` wins; then the bundled resources; then a checkout next to
/// the process (so `tauri dev` works without copying); finally the per-user data
/// dir. `fetch_models.sh` writes to `third_party/models`.
pub fn resolve_models_dir(app: &tauri::AppHandle) -> PathBuf {
    if let Some(dir) = std::env::var_os("ZEIDEX_MODELS_DIR") {
        let dir = PathBuf::from(dir);
        if !dir.as_os_str().is_empty() {
            return dir;
        }
    }
    if let Some(dir) = app
        .path()
        .resource_dir()
        .ok()
        .into_iter()
        .flat_map(|resources| {
            [
                resources.join("models"),
                resources.join("third_party").join("models"),
            ]
        })
        .find(|candidate| looks_like_model_dir(candidate))
    {
        return dir;
    }
    if let Some(dir) = ["third_party/models", "../third_party/models"]
        .into_iter()
        .map(PathBuf::from)
        .find(|candidate| looks_like_model_dir(candidate))
    {
        return dir;
    }
    let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("zeidex").join("models")
}

fn looks_like_model_dir(dir: &Path) -> bool {
    dir.join("ppocrv4_det.onnx").is_file()
}

/// Where downloaded packs live. Always writable and per-user: the bundled
/// folder may be a read-only package path.
///
/// `$ZEIDEX_MODELS_ROOT` wins, so a test or a portable install can redirect it.
pub fn resolve_models_root() -> PathBuf {
    if let Some(dir) = std::env::var_os("ZEIDEX_MODELS_ROOT") {
        let dir = PathBuf::from(dir);
        if !dir.as_os_str().is_empty() {
            return dir;
        }
    }
    let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("zeidex").join("models")
}

/// Optional path to a pdfium shared library, looked up in the bundle, next to the
/// executable, then the system loader. `ZEIDEX_PDFIUM_LIBRARY` overrides it.
pub fn resolve_pdfium_library(app: &tauri::AppHandle) -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("ZEIDEX_PDFIUM_LIBRARY")
        && !path.is_empty()
    {
        return Some(PathBuf::from(path));
    }
    find_bundled_library(app, &["libpdfium.so", "libpdfium.dylib", "pdfium.dll"])
}

/// Points ONNX Runtime's dynamic loader at a bundled shared library.
///
/// The `ort` crate is built with `load-dynamic`, so it resolves the library from
/// `ORT_DYLIB_PATH` at first use. Doing this during `setup` means a packaged app
/// works without the user exporting anything.
pub fn prepare_onnx_runtime(app: &tauri::AppHandle) {
    if std::env::var_os("ORT_DYLIB_PATH").is_some() {
        return;
    }
    let Some(path) = find_bundled_library(
        app,
        &[
            "libonnxruntime.so",
            "libonnxruntime.dylib",
            "onnxruntime.dll",
        ],
    ) else {
        return;
    };
    // SAFETY: this runs once in Tauri's `setup`, before the OCR engine or any
    // worker thread reads the environment.
    unsafe {
        std::env::set_var("ORT_DYLIB_PATH", path);
    }
}

fn find_bundled_library(app: &tauri::AppHandle, names: &[&str]) -> Option<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Ok(resources) = app.path().resource_dir() {
        roots.push(resources.join("lib"));
        roots.push(resources.clone());
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        roots.push(dir.to_path_buf());
        roots.push(dir.join("lib"));
    }
    roots
        .into_iter()
        .flat_map(|root| names.iter().map(move |name| root.join(name)))
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::AppState;

    // A fresh directory per test, removed at the end. The shell has no
    // `tempfile` dependency and adding one for two tests is not worth a new
    // crate in the lockfile.
    fn test_state(name: &str) -> (std::path::PathBuf, AppState) {
        let directory =
            std::env::temp_dir().join(format!("zeidex-state-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("temp dir");
        let state = AppState::new(
            directory.join("zeidex.db"),
            directory.join("models"),
            directory.join("packs"),
            None,
        )
        .expect("state");
        (directory, state)
    }

    #[test]
    fn a_run_owns_its_token_until_it_ends() {
        let (directory, state) = test_state("token");
        assert!(
            !state.cancel_ocr_run(),
            "there is nothing to cancel before a run"
        );
        let token = state.begin_ocr_run();
        assert!(!token.load(std::sync::atomic::Ordering::Relaxed));
        assert!(state.cancel_ocr_run(), "the live run is cancellable");
        assert!(token.load(std::sync::atomic::Ordering::Relaxed));
        state.end_ocr_run(&token);
        assert!(!state.cancel_ocr_run(), "the slot is empty again");
        drop(state);
        let _ = std::fs::remove_dir_all(directory);
    }

    /// The engine must load the files of the active pack, and only believe an
    /// active marker when that pack is actually installed. A marker left behind
    /// by a pack the user removed would otherwise make OCR load missing files.
    #[test]
    fn an_active_pack_decides_which_model_files_a_run_uses() {
        let (directory, state) = test_state("active-pack");
        let bundled = zeidex_models::pack("ppocrv4-mobile").expect("bundled pack");
        assert_eq!(
            state.active_pack().id,
            bundled.id,
            "with no marker the bundled pack is the default"
        );

        let det = bundled
            .files
            .iter()
            .find(|file| file.role == zeidex_models::FileRole::Det)
            .expect("the bundled pack has a detection model");
        assert_eq!(
            state.ocr_paths().det,
            state.models_dir.join(det.name),
            "a bundled run reads detection from the bundled folder"
        );

        // A marker naming a pack that is not installed is ignored: the shell
        // falls back instead of pointing the engine at an empty folder.
        std::fs::create_dir_all(&state.models_root).expect("models root");
        std::fs::write(
            state.models_root.join(zeidex_models::ACTIVE),
            br#"{"pack_id":"ppocrv5-server"}"#,
        )
        .expect("marker");
        assert_eq!(
            state.active_pack().id,
            bundled.id,
            "an uninstalled pack must not become active"
        );

        // Activating the bundled pack explicitly records it, and the resolved
        // paths stay the bundled ones.
        zeidex_models::set_active(&state.models_root, bundled).expect("activate bundled");
        assert_eq!(
            zeidex_models::active_pack(&state.models_root).as_deref(),
            Some(bundled.id)
        );
        assert_eq!(state.ocr_paths().det, state.models_dir.join(det.name));

        drop(state);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn an_old_run_cannot_steal_or_clear_the_new_one() {
        let (directory, state) = test_state("two-runs");
        let first = state.begin_ocr_run();
        let second = state.begin_ocr_run();
        state.end_ocr_run(&first);
        assert!(state.cancel_ocr_run(), "the second run is still live");
        assert!(
            !first.load(std::sync::atomic::Ordering::Relaxed),
            "ending a run must not cancel it"
        );
        assert!(second.load(std::sync::atomic::Ordering::Relaxed));
        state.end_ocr_run(&second);
        assert!(!state.cancel_ocr_run(), "no run left to cancel");
        drop(state);
        let _ = std::fs::remove_dir_all(directory);
    }
}
