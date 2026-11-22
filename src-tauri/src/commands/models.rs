//! The OCR Models screen's backend: catalog, hardware advisor, downloads,
//! activation and the optional benchmark.
//!
//! The catalog, the classification rules and the install layout live in
//! `zeidex-models`, where they are unit tested without a network or a GUI. This
//! module adds the two things that need the shell: the machine probe and the
//! HTTP downloader. Nothing here reads user files or sends anything except the
//! pinned model URLs from the catalog.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use zeidex_models::{
    self as models, DeviceClass, DeviceProfile, InstallState, ModelPack, Recommendation, Tier,
    install, status,
};
use zeidex_ocr::onnx::ModelPaths;
use zeidex_ocr::{OcrEngine, OcrEngineConfig};

use crate::hardware;
use crate::state::AppState;

/// Progress channel for the Models screen. One event stream, filtered by pack id.
const PROGRESS_EVENT: &str = "models-progress";

/// Returned when the user cancels: the UI shows it as "cancelled", not as a
/// failure, and keeps the partial files for a resume.
const CANCELLED: &str = "download cancelled";

#[derive(Debug, Serialize)]
pub struct ModelFileView {
    pub role: zeidex_models::FileRole,
    pub name: String,
    pub present: bool,
    pub expected_bytes: u64,
    pub actual_bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct PackView {
    pub id: &'static str,
    pub tier: Tier,
    pub tier_label: &'static str,
    pub tier_label_ar: &'static str,
    pub name: &'static str,
    pub name_ar: &'static str,
    pub summary: &'static str,
    pub summary_ar: &'static str,
    pub ram_mb: u64,
    pub download_bytes: u64,
    pub installed_bytes: u64,
    pub bundled: bool,
    pub arabic: bool,
    pub english: bool,
    pub rec_ar_visual: bool,
    pub source: &'static str,
    pub license: &'static str,
    pub state: InstallState,
    pub active: bool,
    pub downloading: bool,
    pub dir: String,
    pub files: Vec<ModelFileView>,
}

#[derive(Debug, Serialize)]
pub struct AdvisorView {
    pub profile: DeviceProfile,
    pub class: DeviceClass,
    pub class_label: &'static str,
    pub class_label_ar: &'static str,
    pub score: u8,
    pub max_score: u8,
    pub criteria: Vec<zeidex_models::Criterion>,
    pub recommendation: Recommendation,
    pub recommended_pack: PackView,
    /// Installed RAM in use, for the "memory pressure" line. 0 when unknown.
    pub ram_used_percent: u8,
}

#[derive(Debug, Serialize)]
pub struct VerifyView {
    pub pack_id: &'static str,
    pub ok: bool,
    pub problems: Vec<String>,
    pub checked_files: usize,
    pub checked_bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct SampleView {
    pub name: &'static str,
    pub milliseconds: u64,
    pub characters: usize,
    pub expected_characters: usize,
    /// Character error rate, 0.0 = perfect. Clamped to 1.0 for display.
    pub cer: f32,
    /// What the model actually read, so the number can be checked.
    pub text: String,
}

#[derive(Debug, Serialize)]
pub struct BenchmarkView {
    pub pack_id: &'static str,
    pub pack_name: &'static str,
    pub load_ms: u64,
    pub total_ms: u64,
    /// Resident memory added by loading the pack, MiB. 0 when unmeasurable.
    pub memory_mb: u64,
    pub samples: Vec<SampleView>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DownloadProgress {
    pub pack_id: String,
    /// downloading | verifying | installing | done | cancelled | error
    pub phase: &'static str,
    pub file: Option<String>,
    pub file_index: usize,
    pub file_count: usize,
    pub downloaded: u64,
    pub total: u64,
    pub speed_bps: u64,
    pub message: Option<String>,
}

#[tauri::command(async)]
pub fn models_catalog(state: State<'_, AppState>) -> Vec<PackView> {
    catalog(&state)
}

#[tauri::command(async)]
pub fn models_advisor(state: State<'_, AppState>) -> AdvisorView {
    let profile = hardware::probe(&state.models_root);
    let recommendation = models::recommend(&profile);
    let recommended_pack =
        models::pack(recommendation.pack_id).unwrap_or_else(models::default_pack);
    let ram_used_percent = if profile.total_ram_mb == 0 {
        0
    } else {
        let used = profile
            .total_ram_mb
            .saturating_sub(profile.available_ram_mb);
        ((used * 100) / profile.total_ram_mb).min(100) as u8
    };
    AdvisorView {
        class: recommendation.device_class,
        class_label: recommendation.device_class.label(),
        class_label_ar: recommendation.device_class.label_ar(),
        score: recommendation.score,
        max_score: 7,
        criteria: models::criteria(&profile),
        recommended_pack: pack_view(&state, recommended_pack),
        ram_used_percent,
        profile,
        recommendation,
    }
}

/// Slowest average a download may sustain after its first minute, in bytes per
/// second. Below this the connection is not slow, it is stalled.
const MIN_DOWNLOAD_BYTES_PER_SECOND: f64 = 1_024.0;

/// Downloads, verifies and installs one pack. The pack is not activated here:
/// the user chooses when the working model changes.
#[tauri::command]
pub async fn models_download(
    app: AppHandle,
    state: State<'_, AppState>,
    pack_id: String,
) -> Result<PackView, String> {
    let pack = models::pack(&pack_id).ok_or_else(|| format!("unknown model pack: {pack_id}"))?;
    if pack.bundled {
        return Err(format!("{} is bundled and needs no download", pack.name));
    }
    // "Installed" is not the same as "working". A pack whose files no longer
    // match the catalog — truncated by an interrupted copy, damaged by a restore,
    // or left over from an older layout — reports Installed, so refusing here
    // left the user with no repair path at all: activation failed its digest
    // check and the only way forward was Remove. A pack that verifies is still
    // refused, because re-downloading 300 MB for nothing is worse.
    let current = status(&state.models_root, pack);
    if current.is_installed() && models::verify(&state.models_root, pack).is_ok() {
        return Err(format!("{} is already installed", pack.name));
    }

    let token = state.begin_model_download(pack.id)?;
    let root = state.models_root.clone();
    let result = install_pack(&app, &root, pack, &token).await;
    state.end_model_download(&token);

    match result {
        Ok(()) => {
            emit(
                &app,
                DownloadProgress {
                    pack_id: pack.id.to_string(),
                    phase: "done",
                    file: None,
                    file_index: pack.files.len(),
                    file_count: pack.files.len(),
                    downloaded: pack.download_bytes(),
                    total: pack.download_bytes(),
                    speed_bps: 0,
                    message: None,
                },
            );
            Ok(pack_view(&state, pack))
        }
        Err(err) => {
            let phase = if token.load(Ordering::Relaxed) {
                "cancelled"
            } else {
                "error"
            };
            emit(
                &app,
                DownloadProgress {
                    pack_id: pack.id.to_string(),
                    phase,
                    file: None,
                    file_index: 0,
                    file_count: pack.files.len(),
                    downloaded: 0,
                    total: pack.download_bytes(),
                    speed_bps: 0,
                    message: Some(err.clone()),
                },
            );
            Err(err)
        }
    }
}

#[tauri::command]
pub fn models_cancel_download(state: State<'_, AppState>) -> bool {
    state.cancel_model_download()
}

/// Switches the engine to a pack. Downloaded packs are hashed in full first, so
/// a corrupted model can never become the active one.
#[tauri::command(async)]
pub fn models_activate(
    state: State<'_, AppState>,
    pack_id: String,
) -> Result<Vec<PackView>, String> {
    let pack = models::pack(&pack_id).ok_or_else(|| format!("unknown model pack: {pack_id}"))?;
    if pack.bundled {
        let missing = ModelPaths::from_dir(&state.models_dir).missing();
        if !missing.is_empty() {
            return Err(format!(
                "the bundled models are incomplete: {}",
                missing.join(", ")
            ));
        }
    }
    models::set_active(&state.models_root, pack)?;
    state.reset_ocr_engine();
    Ok(catalog(&state))
}

#[tauri::command(async)]
pub fn models_remove(state: State<'_, AppState>, pack_id: String) -> Result<Vec<PackView>, String> {
    let pack = models::pack(&pack_id).ok_or_else(|| format!("unknown model pack: {pack_id}"))?;
    // `remove` deletes the staging folder that an in-flight download is writing
    // into. On Unix the writer keeps writing to unlinked inodes and then fails
    // at the hash with ENOENT — a confusing error, and the single download slot
    // stays busy until it happens. Refuse instead, and say why.
    if let Some(active) = state.downloading_pack()
        && active == pack.id
    {
        return Err(format!(
            "{} is still downloading; cancel the download before removing it",
            pack.name
        ));
    }
    models::remove(&state.models_root, pack)?;
    state.reset_ocr_engine();
    Ok(catalog(&state))
}

#[tauri::command(async)]
pub fn models_verify(state: State<'_, AppState>, pack_id: String) -> Result<VerifyView, String> {
    let pack = models::pack(&pack_id).ok_or_else(|| format!("unknown model pack: {pack_id}"))?;
    if pack.bundled {
        let paths = state.ocr_paths_for(pack);
        let missing = paths.missing();
        let checked_bytes = [
            &paths.det,
            &paths.rec_en,
            &paths.rec_ar,
            &paths.dict_en,
            &paths.dict_ar,
            &paths.layout,
            &paths.table,
        ]
        .into_iter()
        .filter_map(|path| std::fs::metadata(path).ok())
        .map(|meta| meta.len())
        .sum();
        return Ok(VerifyView {
            pack_id: pack.id,
            ok: missing.is_empty(),
            problems: missing
                .iter()
                .map(|name| format!("{name} is missing"))
                .collect(),
            checked_files: 7,
            checked_bytes,
        });
    }

    match models::verify(&state.models_root, pack) {
        Ok(manifest) => Ok(VerifyView {
            pack_id: pack.id,
            ok: true,
            problems: Vec::new(),
            checked_files: manifest.files.len(),
            checked_bytes: install::installed_bytes(&state.models_root, pack),
        }),
        Err(problems) => Ok(VerifyView {
            pack_id: pack.id,
            ok: false,
            problems,
            checked_files: pack.files.len(),
            checked_bytes: 0,
        }),
    }
}

/// Runs two small sample pages through one pack. Optional: the UI calls it only
/// when the user presses "Test model", never on open.
#[tauri::command]
pub async fn models_benchmark(
    state: State<'_, AppState>,
    pack_id: String,
) -> Result<BenchmarkView, String> {
    let pack = models::pack(&pack_id).ok_or_else(|| format!("unknown model pack: {pack_id}"))?;
    let paths = state.ocr_paths_for(pack);
    let missing = paths.missing();
    if !missing.is_empty() {
        return Err(format!(
            "{} is not ready: {}",
            pack.name,
            missing.join(", ")
        ));
    }
    let pdfium = state.pdfium_library.clone();
    tauri::async_runtime::spawn_blocking(move || run_benchmark(pack, paths, pdfium))
        .await
        .map_err(|err| err.to_string())?
}

fn catalog(state: &AppState) -> Vec<PackView> {
    // A promotion that was interrupted between its two renames leaves the pack
    // folder absent and the previous bytes orphaned under `.retired-<id>`. The
    // list is the first thing the models screen asks for, so repairing here means
    // the user sees a working pack instead of a 300 MB download button.
    for pack in models::PACKS {
        if !pack.bundled {
            models::recover(&state.models_root, pack);
        }
    }
    models::PACKS
        .iter()
        .map(|pack| pack_view(state, pack))
        .collect()
}

fn pack_view(state: &AppState, pack: &'static ModelPack) -> PackView {
    let pack_status = status(&state.models_root, pack);
    let active = state.active_pack().id == pack.id;
    let downloading = state.downloading_pack().as_deref() == Some(pack.id);
    let dir = if pack.bundled {
        state.models_dir.clone()
    } else {
        pack_status.dir.clone()
    };
    PackView {
        id: pack.id,
        tier: pack.tier,
        tier_label: pack.tier.label(),
        tier_label_ar: pack.tier.label_ar(),
        name: pack.name,
        name_ar: pack.name_ar,
        summary: pack.summary,
        summary_ar: pack.summary_ar,
        ram_mb: pack.ram_mb,
        download_bytes: pack.download_bytes(),
        installed_bytes: installed_bytes(state, pack),
        bundled: pack.bundled,
        // Both recognizers are part of every pack; the flags are explicit so the
        // UI never has to infer capability from a model name.
        arabic: true,
        english: true,
        rec_ar_visual: pack.rec_ar_visual,
        source: pack.source,
        license: pack.license,
        state: pack_status.state,
        active,
        downloading,
        dir: dir.to_string_lossy().into_owned(),
        files: pack_status
            .files
            .into_iter()
            .map(|file| ModelFileView {
                role: file.role,
                name: file.name,
                present: file.present,
                expected_bytes: file.expected_bytes,
                actual_bytes: file.actual_bytes,
            })
            .collect(),
    }
}

fn installed_bytes(state: &AppState, pack: &'static ModelPack) -> u64 {
    if pack.bundled {
        let paths = state.ocr_paths_for(pack);
        [
            &paths.det,
            &paths.rec_en,
            &paths.rec_ar,
            &paths.dict_en,
            &paths.dict_ar,
            &paths.layout,
            &paths.table,
        ]
        .into_iter()
        .filter_map(|path| std::fs::metadata(path).ok())
        .map(|meta| meta.len())
        .sum()
    } else {
        install::installed_bytes(&state.models_root, pack)
    }
}

fn emit(app: &AppHandle, progress: DownloadProgress) {
    let _ = app.emit(PROGRESS_EVENT, progress);
}

/// Downloads every file, verifies it, writes the manifest and promotes the
/// staging folder. Files that already verify are skipped, so a retry resumes
/// where the last attempt stopped instead of re-downloading a 113 MB model.
async fn install_pack(
    app: &AppHandle,
    root: &Path,
    pack: &'static ModelPack,
    token: &Arc<AtomicBool>,
) -> Result<(), String> {
    let staging = install::staging_dir(root, pack);
    std::fs::create_dir_all(&staging).map_err(|err| {
        format!(
            "could not create the staging folder {}: {err}",
            staging.display()
        )
    })?;

    let total = pack.download_bytes();
    let free_mb = hardware::free_space_mb(root);
    // The same requirement the advisor uses, so a device is never told a pack
    // does not fit and then allowed to install it.
    let needed_mb = models::disk_requirement_mb(pack);
    if free_mb != 0 && free_mb < needed_mb {
        return Err(format!(
            "{} needs about {needed_mb} MB of free disk; only {free_mb} MB is available on the models volume",
            pack.name
        ));
    }

    let client = reqwest::Client::builder()
        .user_agent(concat!("Zeidex/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(30))
        // A stalled connection must not hang the download forever; a healthy
        // 300 MB transfer still has data arriving every second.
        .read_timeout(Duration::from_secs(120))
        .build()
        .map_err(|err| format!("could not create the download client: {err}"))?;

    let mut downloaded = 0u64;
    for (index, file) in pack.files.iter().enumerate() {
        if token.load(Ordering::Relaxed) {
            return Err(CANCELLED.to_string());
        }
        let final_path = staging.join(file.name);
        if final_path.is_file()
            && install::hash_file(&final_path).ok().as_deref() == Some(file.sha256)
        {
            downloaded += file.bytes;
            emit(
                app,
                DownloadProgress {
                    pack_id: pack.id.to_string(),
                    phase: "downloading",
                    file: Some(file.name.to_string()),
                    file_index: index + 1,
                    file_count: pack.files.len(),
                    downloaded,
                    total,
                    speed_bps: 0,
                    message: None,
                },
            );
            continue;
        }

        let part = install::part_path(&staging, file);
        fetch_file(
            &client, app, pack, file, &part, index, downloaded, total, token,
        )
        .await?;

        emit(
            app,
            DownloadProgress {
                pack_id: pack.id.to_string(),
                phase: "verifying",
                file: Some(file.name.to_string()),
                file_index: index + 1,
                file_count: pack.files.len(),
                downloaded: downloaded + file.bytes,
                total,
                speed_bps: 0,
                message: None,
            },
        );
        let digest = install::hash_file(&part)?;
        if digest != file.sha256 {
            let _ = std::fs::remove_file(&part);
            return Err(format!(
                "{} failed its SHA-256 check (expected {}, got {digest}); the file was discarded",
                file.name, file.sha256
            ));
        }
        // A YAML model config is turned into the dictionary the engine reads;
        // everything else is used as published.
        let prepared = install::prepare_file(file, &part)?;
        std::fs::write(&final_path, prepared)
            .map_err(|err| format!("could not write {}: {err}", final_path.display()))?;
        let _ = std::fs::remove_file(&part);
        downloaded += file.bytes;
    }

    if token.load(Ordering::Relaxed) {
        return Err(CANCELLED.to_string());
    }
    emit(
        app,
        DownloadProgress {
            pack_id: pack.id.to_string(),
            phase: "installing",
            file: None,
            file_index: pack.files.len(),
            file_count: pack.files.len(),
            downloaded: total,
            total,
            speed_bps: 0,
            message: None,
        },
    );
    install::write_manifest(&staging, pack)?;
    // `promote` re-verifies the staging folder before the swap: a corrupted
    // download must never replace a pack that currently works.
    install::promote(root, pack)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn fetch_file(
    client: &reqwest::Client,
    app: &AppHandle,
    pack: &ModelPack,
    file: &zeidex_models::PackFile,
    part: &Path,
    file_index: usize,
    base_downloaded: u64,
    total: u64,
    token: &Arc<AtomicBool>,
) -> Result<(), String> {
    let mut offset = std::fs::metadata(part).map(|meta| meta.len()).unwrap_or(0);
    if offset > file.bytes {
        let _ = std::fs::remove_file(part);
        offset = 0;
    }
    // A complete `.part` from a previous attempt needs no request: it is hashed
    // and either installed or discarded by the caller.
    if offset == file.bytes && offset > 0 {
        return Ok(());
    }

    let mut request = client.get(file.url);
    if offset > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={offset}-"));
    }
    let mut response = request
        .send()
        .await
        .map_err(|err| format!("{}: {err}", file.name))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!(
            "{}: the server answered {status}; check the connection and retry",
            file.name
        ));
    }
    // A body longer than the catalog says must not be written. `reqwest` follows
    // up to ten redirects and does not constrain the final host, so a hostile or
    // compromised mirror could stream an unbounded body into the staging folder
    // until the volume filled. The digest check would still reject the content;
    // the point here is to refuse before the disk is gone.
    if let Some(length) = response.content_length()
        && length > file.bytes.saturating_sub(offset)
    {
        return Err(format!(
            "{}: the server offered {length} bytes but the catalog expects {}; refusing the download",
            file.name, file.bytes
        ));
    }
    // A server that ignores Range answers 200 with the whole file. Start over
    // rather than appending a second copy to the partial one.
    let resuming = offset > 0 && status == reqwest::StatusCode::PARTIAL_CONTENT;
    if !resuming {
        offset = 0;
    }

    let mut sink = if resuming {
        std::fs::OpenOptions::new()
            .append(true)
            .open(part)
            .map_err(|err| format!("{}: {err}", part.display()))?
    } else {
        std::fs::File::create(part).map_err(|err| format!("{}: {err}", part.display()))?
    };

    let started = Instant::now();
    let mut received = offset;
    let mut last_emit = Instant::now();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|err| format!("{}: {err}", file.name))?
    {
        if token.load(Ordering::Relaxed) {
            sink.flush().ok();
            return Err(CANCELLED.to_string());
        }
        // The declared length can be absent or wrong, so the running total is
        // checked too: the write must never take the staging folder past the
        // size the catalog promises.
        if received + chunk.len() as u64 > file.bytes {
            sink.flush().ok();
            drop(sink);
            let _ = std::fs::remove_file(part);
            return Err(format!(
                "{}: the server sent more than the {} bytes the catalog expects; the download was discarded",
                file.name, file.bytes
            ));
        }
        sink.write_all(&chunk)
            .map_err(|err| format!("{}: {err}", part.display()))?;
        received += chunk.len() as u64;
        // A server that dribbles a byte at a time never trips the per-read
        // timeout, and the single download slot would stay busy for as long as it
        // cared to. A minimum average rate after a grace period closes that
        // without hurting a genuinely slow link: even a 56k modem clears 5 KB/s.
        let elapsed = started.elapsed();
        if elapsed > Duration::from_secs(60) {
            let average = (received - offset) as f64 / elapsed.as_secs_f64().max(1.0);
            if average < MIN_DOWNLOAD_BYTES_PER_SECOND {
                sink.flush().ok();
                drop(sink);
                let _ = std::fs::remove_file(part);
                return Err(format!(
                    "{}: the connection is delivering about {average:.0} bytes per second; the download was stopped",
                    file.name
                ));
            }
        }
        if last_emit.elapsed() >= Duration::from_millis(120) {
            let elapsed = started.elapsed().as_secs_f64().max(0.001);
            let speed_bps = ((received - offset) as f64 / elapsed) as u64;
            emit(
                app,
                DownloadProgress {
                    pack_id: pack.id.to_string(),
                    phase: "downloading",
                    file: Some(file.name.to_string()),
                    file_index: file_index + 1,
                    file_count: pack.files.len(),
                    downloaded: base_downloaded + received,
                    total,
                    speed_bps,
                    message: None,
                },
            );
            last_emit = Instant::now();
        }
    }
    sink.flush()
        .map_err(|err| format!("{}: {err}", part.display()))?;
    if received != file.bytes {
        return Err(format!(
            "{}: the download ended after {received} of {} bytes; retry to resume",
            file.name, file.bytes
        ));
    }
    emit(
        app,
        DownloadProgress {
            pack_id: pack.id.to_string(),
            phase: "downloading",
            file: Some(file.name.to_string()),
            file_index: file_index + 1,
            file_count: pack.files.len(),
            downloaded: base_downloaded + received,
            total,
            speed_bps: 0,
            message: None,
        },
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Benchmark
// ---------------------------------------------------------------------------

/// Two pages rendered from known text. Small, clean and deterministic: the
/// benchmark compares models on the same input, it does not estimate accuracy on
/// a real corpus.
const SAMPLE_EN_PNG: &[u8] = include_bytes!("../../resources/samples/ocr-benchmark-en.png");
const SAMPLE_AR_PNG: &[u8] = include_bytes!("../../resources/samples/ocr-benchmark-ar.png");
const SAMPLE_EN_TEXT: &str = "Zeidex reads documents offline. Invoice 2024 total: 1250.00 USD.";
const SAMPLE_AR_TEXT: &str = "زايدكس يقرأ المستندات دون اتصال بالإنترنت. الفاتورة رقم 2024.";

fn run_benchmark(
    pack: &'static ModelPack,
    paths: ModelPaths,
    pdfium: Option<PathBuf>,
) -> Result<BenchmarkView, String> {
    let baseline_mb = hardware::process_memory_mb();
    let started = Instant::now();
    let engine = OcrEngine::with_paths(
        paths,
        OcrEngineConfig {
            pdfium_library: pdfium,
            ..OcrEngineConfig::default()
        },
    )
    .map_err(|err| err.to_string())?;
    let load_ms = started.elapsed().as_millis() as u64;
    let memory_mb = hardware::process_memory_mb().saturating_sub(baseline_mb);

    let english = run_sample(&engine, "english", SAMPLE_EN_PNG, SAMPLE_EN_TEXT)?;
    let arabic = run_sample(&engine, "arabic", SAMPLE_AR_PNG, SAMPLE_AR_TEXT)?;
    Ok(BenchmarkView {
        pack_id: pack.id,
        pack_name: pack.name,
        load_ms,
        total_ms: started.elapsed().as_millis() as u64,
        memory_mb,
        samples: vec![english, arabic],
    })
}

fn run_sample(
    engine: &OcrEngine,
    name: &'static str,
    png: &[u8],
    expected: &str,
) -> Result<SampleView, String> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!(
        "zeidex-benchmark-{}-{nanos}-{name}.png",
        std::process::id()
    ));
    std::fs::write(&path, png).map_err(|err| format!("could not write the sample: {err}"))?;
    let started = Instant::now();
    let outcome = engine.process_image(&path);
    let milliseconds = started.elapsed().as_millis() as u64;
    let _ = std::fs::remove_file(&path);
    let page = outcome.map_err(|err| err.to_string())?;

    let mut text = page.text;
    if text.chars().count() > 600 {
        text = text.chars().take(600).collect();
    }
    Ok(SampleView {
        name,
        milliseconds,
        characters: text.chars().filter(|ch| ch.is_alphanumeric()).count(),
        expected_characters: expected.chars().filter(|ch| ch.is_alphanumeric()).count(),
        cer: character_error_rate(expected, &text),
        text,
    })
}

/// Character error rate over letters and digits only.
///
/// Spacing and punctuation are a layout decision, not a recognition one, so
/// they are excluded; everything else counts, including order.
fn character_error_rate(expected: &str, actual: &str) -> f32 {
    let expected = normalise_for_comparison(expected);
    let actual = normalise_for_comparison(actual);
    if expected.is_empty() {
        return 0.0;
    }
    (levenshtein(&expected, &actual) as f32 / expected.len() as f32).min(1.0)
}

fn normalise_for_comparison(text: &str) -> Vec<char> {
    text.chars()
        .filter(|ch| ch.is_alphanumeric())
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

fn levenshtein(left: &[char], right: &[char]) -> usize {
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    let mut current = vec![0usize; right.len() + 1];
    for (row, left_char) in left.iter().enumerate() {
        current[0] = row + 1;
        for (column, right_char) in right.iter().enumerate() {
            let substitution = previous[column] + usize::from(left_char != right_char);
            current[column + 1] = (previous[column + 1] + 1)
                .min(current[column] + 1)
                .min(substitution);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_text_has_no_error() {
        assert_eq!(character_error_rate("Invoice 2024", "invoice 2024"), 0.0);
    }

    #[test]
    fn punctuation_and_spacing_do_not_count() {
        assert_eq!(character_error_rate("a, b!", "a b"), 0.0);
    }

    #[test]
    fn one_transposition_costs_two_edits() {
        // Levenshtein, not Damerau: "reprot" needs a delete and an insert. The
        // rate is the standard CER definition, so it must not hide that.
        let rate = character_error_rate("report", "reprot");
        assert!((rate - 2.0 / 6.0).abs() < 1e-6, "{rate}");
    }

    #[test]
    fn one_missing_character_is_one_edit() {
        let rate = character_error_rate("report", "repor");
        assert!((rate - 1.0 / 6.0).abs() < 1e-6, "{rate}");
    }

    #[test]
    fn a_missing_line_is_a_total_error() {
        assert_eq!(character_error_rate("report", ""), 1.0);
    }

    #[test]
    fn arabic_letters_are_compared_not_dropped() {
        let rate = character_error_rate("فاتورة", "فاتوره");
        assert!(rate > 0.0, "a changed Arabic letter must count");
        assert_eq!(character_error_rate("فاتورة", "فاتورة"), 0.0);
    }

    #[test]
    fn the_benchmark_samples_are_png_files() {
        assert!(SAMPLE_EN_PNG.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(SAMPLE_AR_PNG.starts_with(b"\x89PNG\r\n\x1a\n"));
    }
}

/// The screen reads these names. Rust structs and the TypeScript interfaces are
/// two hand-written halves of one contract, and a `rename_all` on one side only
/// is exactly how the Models screen shipped blank: `recommended_pack` arrived as
/// `recommendedPack`, the card dereferenced `undefined`, and React unmounted the
/// tree. These tests pin the wire names so the two halves cannot drift again.
#[cfg(test)]
mod contract {
    use super::*;

    fn pack() -> PackView {
        PackView {
            id: "ppocrv4-mobile",
            tier: Tier::Compact,
            tier_label: "Lightweight",
            tier_label_ar: "خفيف",
            name: "Fast",
            name_ar: "سريع",
            summary: "summary",
            summary_ar: "ملخص",
            ram_mb: 320,
            download_bytes: 0,
            installed_bytes: 0,
            bundled: true,
            arabic: true,
            english: true,
            rec_ar_visual: false,
            source: "PaddleOCR",
            license: "Apache-2.0",
            state: InstallState::Installed,
            active: true,
            downloading: false,
            dir: "/models".to_string(),
            files: vec![ModelFileView {
                role: zeidex_models::FileRole::Det,
                name: "det.onnx".to_string(),
                present: true,
                expected_bytes: 1,
                actual_bytes: 1,
            }],
        }
    }

    fn keys(value: &serde_json::Value) -> Vec<&str> {
        let mut names: Vec<&str> = value
            .as_object()
            .expect("a struct serialises to an object")
            .keys()
            .map(String::as_str)
            .collect();
        names.sort_unstable();
        names
    }

    #[test]
    fn pack_payload_uses_the_names_the_ui_reads() {
        let json = serde_json::to_value(pack()).unwrap();
        assert_eq!(
            keys(&json),
            vec![
                "active",
                "arabic",
                "bundled",
                "dir",
                "download_bytes",
                "downloading",
                "english",
                "files",
                "id",
                "installed_bytes",
                "license",
                "name",
                "name_ar",
                "ram_mb",
                "rec_ar_visual",
                "source",
                "state",
                "summary",
                "summary_ar",
                "tier",
                "tier_label",
                "tier_label_ar",
            ]
        );
        assert_eq!(
            keys(&json["files"][0]),
            vec!["actual_bytes", "expected_bytes", "name", "present", "role"]
        );
    }

    #[test]
    fn advisor_payload_uses_the_names_the_ui_reads() {
        let advisor = AdvisorView {
            profile: DeviceProfile {
                cpu_model: "Test CPU".to_string(),
                physical_cores: 8,
                logical_cores: 8,
                total_ram_mb: 16_384,
                available_ram_mb: 8_192,
                free_disk_mb: 100_000,
                os: "linux".to_string(),
                arch: "aarch64".to_string(),
                gpu: None,
                gpu_memory_mb: None,
                gpu_acceleration: false,
            },
            class: DeviceClass::Standard,
            class_label: "Standard",
            class_label_ar: "قياسي",
            score: 3,
            max_score: 7,
            criteria: vec![],
            recommendation: Recommendation {
                pack_id: "ppocrv4-mobile",
                device_class: DeviceClass::Standard,
                score: 3,
                reason: "reason".to_string(),
                reason_ar: "سبب".to_string(),
                downgraded: false,
                insufficient_memory: false,
                ocr_concurrency: 2,
                batch_chunk: 64,
                index_threads: 2,
            },
            recommended_pack: pack(),
            ram_used_percent: 40,
        };
        let json = serde_json::to_value(advisor).unwrap();
        assert_eq!(
            keys(&json),
            vec![
                "class",
                "class_label",
                "class_label_ar",
                "criteria",
                "max_score",
                "profile",
                "ram_used_percent",
                "recommendation",
                "recommended_pack",
                "score",
            ]
        );
        assert_eq!(
            keys(&json["recommendation"]),
            vec![
                "batch_chunk",
                "device_class",
                "downgraded",
                "index_threads",
                "insufficient_memory",
                "ocr_concurrency",
                "pack_id",
                "reason",
                "reason_ar",
                "score",
            ]
        );
        assert_eq!(
            keys(&json["profile"]),
            vec![
                "arch",
                "available_ram_mb",
                "cpu_model",
                "free_disk_mb",
                "gpu",
                "gpu_acceleration",
                "gpu_memory_mb",
                "logical_cores",
                "os",
                "physical_cores",
                "total_ram_mb",
            ]
        );
    }

    #[test]
    fn progress_payload_uses_the_names_the_ui_reads() {
        let progress = DownloadProgress {
            pack_id: "ppocrv5-server".to_string(),
            phase: "downloading",
            file: None,
            file_index: 0,
            file_count: 1,
            downloaded: 0,
            total: 1,
            speed_bps: 0,
            message: None,
        };
        assert_eq!(
            keys(&serde_json::to_value(progress).unwrap()),
            vec![
                "downloaded",
                "file",
                "file_count",
                "file_index",
                "message",
                "pack_id",
                "phase",
                "speed_bps",
                "total",
            ]
        );
    }
}
