use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tauri::Manager;
use zeidex_engine::QueryEngine;
use zeidex_ocr::OcrEngine;
use zeidex_storage::SqliteStore;

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
    pub models_dir: PathBuf,
    pub pdfium_library: Option<PathBuf>,
    pub db_path: PathBuf,
}

impl AppState {
    pub fn new(
        db_path: PathBuf,
        models_dir: PathBuf,
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
            models_dir,
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
        let engine = std::sync::Arc::new(OcrEngine::new(zeidex_ocr::OcrEngineConfig {
            models_dir: self.models_dir.clone(),
            pdfium_library: self.pdfium_library.clone(),
            dpi: 300.0,
            ..Default::default()
        })?);
        *guard = Some(engine.clone());
        Ok(engine)
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
