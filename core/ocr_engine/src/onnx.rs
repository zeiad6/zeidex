use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use image::{DynamicImage, GenericImageView, Rgb, RgbImage};
#[cfg(any(feature = "cuda", feature = "directml"))]
use ort::ep::ExecutionProvider as _;
use ort::ep::{CPU, ExecutionProviderDispatch};
use ort::inputs;
use ort::session::Session;
use ort::value::Tensor;

use crate::bidi;
use crate::dict::Dictionary;
use crate::error::{OcrError, OcrResult};
use crate::geometry::BBox;
use crate::layout::{self, PageLayout, Region, RegionKind};
use crate::pico;
use crate::table::{self, StructureDict, TableStructure};

/// Location of every ONNX model and dictionary the pipeline needs. All paths are
/// resolved from one directory so the Tauri shell can bundle them as resources
/// and point `ZEIDEX_MODELS_DIR` at the unpacked folder.
#[derive(Clone, Debug)]
pub struct ModelPaths {
    pub det: PathBuf,
    pub rec_en: PathBuf,
    pub rec_ar: PathBuf,
    pub dict_en: PathBuf,
    pub dict_ar: PathBuf,
    pub layout: PathBuf,
    pub table: PathBuf,
    /// Structure-token class list for the table model. Optional: when absent the
    /// built-in PP-StructureV2 list is used, so an older model folder still runs.
    pub table_dict: PathBuf,
    /// True when the Arabic recognizer emits glyphs in visual (page) order
    /// rather than logical order. PP-OCRv4 emits logical order; the v5 mobile
    /// Arabic recognizer emits visual order, which [`crate::bidi::visual_to_logical`]
    /// converts back. Declared per model instead of guessed per line: a line
    /// cannot tell you which convention its recognizer used.
    pub rec_ar_visual: bool,
    /// Same flag for the Latin recognizer.
    pub rec_en_visual: bool,
}

impl ModelPaths {
    pub fn from_dir(dir: impl AsRef<Path>) -> Self {
        let dir = dir.as_ref();
        Self {
            det: dir.join("ppocrv4_det.onnx"),
            rec_en: dir.join("ppocrv4_rec_en.onnx"),
            rec_ar: dir.join("ppocrv4_rec_ar.onnx"),
            dict_en: dir.join("ppocr_keys_v1.txt"),
            dict_ar: dir.join("arabic_dict.txt"),
            layout: dir.join("ppstructurev2_layout.onnx"),
            table: dir.join("slanet_table.onnx"),
            table_dict: dir.join("table_structure_dict.txt"),
            rec_ar_visual: false,
            rec_en_visual: false,
        }
    }

    /// Marks the Arabic recognizer as visual-order. Used when a pack selects a
    /// v5 Arabic recognizer; `from_dir` describes the v4 layout, which is
    /// logical-order.
    pub fn with_visual_arabic(mut self) -> Self {
        self.rec_ar_visual = true;
        self
    }

    /// True when every file the pipeline needs is present. The engine refuses to
    /// start on a partial set rather than failing halfway through a document.
    pub fn is_complete(&self) -> bool {
        self.missing().is_empty()
    }

    /// Relative names of the files that are not present, for the UI's status
    /// panel and for `fetch_models.sh` verification.
    pub fn missing(&self) -> Vec<String> {
        [
            ("ppocrv4_det.onnx", &self.det),
            ("ppocrv4_rec_en.onnx", &self.rec_en),
            ("ppocrv4_rec_ar.onnx", &self.rec_ar),
            ("ppocr_keys_v1.txt", &self.dict_en),
            ("arabic_dict.txt", &self.dict_ar),
            ("ppstructurev2_layout.onnx", &self.layout),
            ("slanet_table.onnx", &self.table),
        ]
        .into_iter()
        .filter(|(_, path)| !path.is_file())
        // The actual name, not the bundled one: a downloaded pack calls the
        // detector `det.onnx`, and a message naming a file that is not part of
        // the pack sends the user looking for the wrong thing.
        .map(|(bundled_name, path)| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| bundled_name.to_string())
        })
        .collect()
    }
}

/// Loaded ONNX sessions and dictionaries. `Session` is `Send + Sync`, but
/// inference needs `&mut`, so each session is behind a mutex. A per-thread
/// session pool is a later optimization; the models here are small.
pub struct ModelSet {
    pub det: Mutex<Session>,
    pub rec_en: Mutex<Session>,
    pub rec_ar: Mutex<Session>,
    pub layout: Mutex<Session>,
    pub table: Mutex<Session>,
    pub dict_en: Dictionary,
    pub dict_ar: Dictionary,
    pub table_dict: StructureDict,
    /// Output order of each recognizer; see [`ModelPaths::rec_ar_visual`].
    pub rec_en_visual: bool,
    pub rec_ar_visual: bool,
}

/// Execution-provider fallback: CUDA, then DirectML, then CPU. The GPU features
/// are opt-in cargo features; without them the CPU provider is always used.
pub fn execution_providers() -> Vec<ExecutionProviderDispatch> {
    #[cfg(feature = "cuda")]
    if ort::ep::CUDA::is_available().unwrap_or(false) {
        return vec![ort::ep::CUDA::default().build()];
    }
    #[cfg(feature = "directml")]
    if ort::ep::DirectML::is_available().unwrap_or(false) {
        return vec![ort::ep::DirectML::default().build()];
    }
    vec![CPU::default().build()]
}

/// The process-wide ONNX Runtime environment.
///
/// ONNX Runtime allows exactly one environment per process and `ort::init()`
/// refuses a second `build()`, so the environment has to outlive any single
/// engine. It does not by accident: the shell drops the cached engine when the
/// user activates or removes a model pack (`AppState::reset_ocr_engine`), and
/// the next OCR run builds a new one. Building the environment once and cloning
/// the handle is what lets a pack switch take effect without restarting the app;
/// creating it per engine made the second load fail with "only one environment
/// is allowed per process", which left OCR dead until a restart.
static ENVIRONMENT: OnceLock<ort::environment::Environment> = OnceLock::new();

/// Serialises the first initialisation so two threads racing to load the engine
/// cannot both reach `build()`; the loser would get the "one environment" error
/// rather than a working handle.
static ENVIRONMENT_INIT: Mutex<()> = Mutex::new(());

/// The shared environment, built on first use.
fn environment() -> OcrResult<ort::environment::Environment> {
    if let Some(environment) = ENVIRONMENT.get() {
        return Ok(environment.clone());
    }
    let _guard = ENVIRONMENT_INIT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(environment) = ENVIRONMENT.get() {
        return Ok(environment.clone());
    }
    // `ort` panics rather than returning an error when the dylib cannot be
    // loaded, and a missing `libonnxruntime.so` must surface as "OCR is
    // unavailable", not as a panic that takes the caller with it.
    let built = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ort::init()
            .with_name("zeidex-ocr")
            .with_execution_providers(execution_providers())
            .build()
    })) {
        Ok(Ok(environment)) => environment,
        Ok(Err(err)) => return Err(OcrError::Onnx(err)),
        Err(_) => {
            return Err(OcrError::RuntimeUnavailable(
                "the shared library could not be loaded".to_string(),
            ));
        }
    };
    let _ = ENVIRONMENT.set(built.clone());
    Ok(built)
}

/// Threads one ONNX session may use.
///
/// The pipeline parallelises pages with rayon and guards every session with a
/// mutex, so at most three sessions run at once (detection and the two
/// recognizers). Dividing the cores between them keeps the process near the core
/// count instead of spawning `sessions x cores` threads - five sessions on a
/// seven-core machine meant up to 35 ORT threads, each with its own arena, which
/// is the wrong shape for the low-RAM devices the Compact tier targets.
///
/// It also fixes a startup error on this class of host: when the thread count is
/// left to auto-detection ORT tries to pin threads with
/// `pthread_setaffinity_np`, which fails in containers and on Android with
/// "Specify the number of threads explicitly so the affinity is not set".
fn intra_threads() -> usize {
    let cores = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1);
    (cores / 3).clamp(1, 4)
}

/// Loads every model. `ort::Environment` is created once and shared; with the
/// `load-dynamic` feature the ONNX Runtime shared library is loaded at runtime,
/// so no GPU runtime is required to build or run on CPU.
pub fn load_models(paths: &ModelPaths) -> OcrResult<ModelSet> {
    if !paths.is_complete() {
        return Err(OcrError::Model(format!(
            "model set is incomplete under {}",
            paths.det.display()
        )));
    }

    let environment = environment()?;

    let providers = execution_providers();
    let det = load_session(&environment, &paths.det, &providers)?;
    let rec_en = load_session(&environment, &paths.rec_en, &providers)?;
    let rec_ar = load_session(&environment, &paths.rec_ar, &providers)?;
    let layout = load_session(&environment, &paths.layout, &providers)?;
    let table = load_session(&environment, &paths.table, &providers)?;

    Ok(ModelSet {
        det: Mutex::new(det),
        rec_en: Mutex::new(rec_en),
        rec_ar: Mutex::new(rec_ar),
        layout: Mutex::new(layout),
        table: Mutex::new(table),
        dict_en: Dictionary::load(&paths.dict_en)?,
        dict_ar: Dictionary::load(&paths.dict_ar)?,
        rec_en_visual: paths.rec_en_visual,
        rec_ar_visual: paths.rec_ar_visual,
        // A missing structure dictionary is not fatal: the built-in list is the
        // same 28 PP-StructureV2 fragments the export was trained with.
        table_dict: StructureDict::load(&paths.table_dict)
            .unwrap_or_else(|_| StructureDict::builtin()),
    })
}

fn load_session(
    environment: &ort::environment::Environment,
    path: &Path,
    providers: &[ExecutionProviderDispatch],
) -> OcrResult<Session> {
    let mut builder = Session::builder(environment)?
        .with_execution_providers(providers)
        .map_err(|err| OcrError::Model(err.to_string()))?
        // Set explicitly; see `intra_threads`. Left to auto-detection ORT pins
        // threads to cores, which fails in containers and on Android.
        .with_intra_threads(intra_threads())
        .map_err(|err| OcrError::Model(err.to_string()))?;
    Ok(builder.commit_from_file(path)?)
}

/// A detected text line before recognition.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextLine {
    pub bbox: BBox,
    pub confidence: f32,
}

const DET_THRESHOLD: f32 = 0.3;
const REC_TARGET_HEIGHT: u32 = 48;
/// Upper bound on a recognition crop's width. Freezing this at 320 (the scene-text
/// default) squashed long document lines; the ONNX input width is dynamic, so a
/// long line keeps its aspect instead of being distorted.
const REC_MAX_WIDTH: u32 = 2048;
/// Width used for a degenerate (zero-sized) crop.
const REC_DEFAULT_WIDTH: u32 = 320;

/// Long-side limit for text detection.
///
/// Scene-text PP-OCR defaults to 960 px, but a 300 DPI A4 page is 2480 x 3508.
/// 1600 px is only 137 DPI on that page: a probe on the PDF fixture showed the
/// detector dropping an isolated digit in a table column, while 2400 px
/// recovers it at 0.999 confidence. 2400 is ~205 DPI for A4 and keeps memory
/// bounded; the cap is still applied before every detection pass.
pub const DETECT_MAX_SIDE: u32 = 2400;

/// Detects text-line boxes with the PP-OCRv4 DBNet model at the default
/// [`DETECT_MAX_SIDE`].
pub fn detect_text_lines(models: &ModelSet, image: &DynamicImage) -> OcrResult<Vec<TextLine>> {
    detect_text_lines_with_limit(models, image, DETECT_MAX_SIDE)
}

/// Detects text-line boxes, downscaling so the long side is at most `max_side`.
pub fn detect_text_lines_with_limit(
    models: &ModelSet,
    image: &DynamicImage,
    max_side: u32,
) -> OcrResult<Vec<TextLine>> {
    let max_side = max_side.max(32);
    let rgb = image.to_rgb8();
    let (width, height) = (rgb.width(), rgb.height());
    // Only a downscale is applied. `scale` must never exceed 1.0: dividing the
    // output boxes by a factor that was not used to resize would shrink every
    // box, and a small page (shorter than `max_side`) would come out at the
    // wrong size and position.
    let scale = (max_side as f32 / width.max(height) as f32).min(1.0);
    let (resized_w, resized_h) = if scale < 1.0 {
        (
            ((width as f32 * scale).round() as u32).max(32),
            ((height as f32 * scale).round() as u32).max(32),
        )
    } else {
        (width, height)
    };
    let padded_w = resized_w.div_ceil(32) * 32;
    let padded_h = resized_h.div_ceil(32) * 32;

    let resized = image::imageops::resize(
        &rgb,
        resized_w,
        resized_h,
        image::imageops::FilterType::Triangle,
    );
    let data = CHW_PREPROC_MEAN
        .iter()
        .zip(CHW_PREPROC_STD.iter())
        .enumerate()
        .flat_map(|(channel, (mean, std))| {
            let mut out = Vec::with_capacity((padded_w * padded_h) as usize);
            for y in 0..padded_h {
                for x in 0..padded_w {
                    let pixel = if x < resized_w && y < resized_h {
                        resized.get_pixel(x, y)
                    } else {
                        &Rgb([255, 255, 255])
                    };
                    let value = pixel.0[channel] as f32 / 255.0;
                    out.push((value - mean) / std);
                }
            }
            out
        })
        .collect::<Vec<f32>>();

    let tensor = Tensor::from_array(([1usize, 3, padded_h as usize, padded_w as usize], data))?;
    let mut session = models
        .det
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let outputs = session.run(inputs![tensor])?;
    let (_shape, probabilities) = outputs[0].try_extract_tensor::<f32>()?;

    let mask = probabilities
        .iter()
        .map(|&value| value > DET_THRESHOLD)
        .collect::<Vec<bool>>();
    let components = connected_components(&mask, padded_w as usize, padded_h as usize);
    let mut lines = Vec::new();
    for component in components {
        if component.area() < 16.0 {
            continue;
        }
        let raw = BBox::new(
            component.x0 / scale,
            component.y0 / scale,
            component.x1 / scale,
            component.y1 / scale,
        );
        lines.push(TextLine {
            bbox: unclip(raw, DB_UNCLIP_RATIO, width, height),
            confidence: 1.0,
        });
    }
    Ok(lines)
}

/// Grow a DBNet box back to the glyph extent.
///
/// DBNet is trained on a *shrunk* text kernel, so its probability region covers
/// only the middle of each line. Cropping that band and squeezing it to 48 px
/// strips the ascenders and descenders that recognition needs — Arabic letter
/// forms in particular. PaddleOCR expands the polygon by
/// `area * ratio / perimeter`; for an axis-aligned box that is the same distance
/// on every side.
fn unclip(bbox: BBox, ratio: f32, width: u32, height: u32) -> BBox {
    let box_width = bbox.width();
    let box_height = bbox.height();
    if box_width <= 0.0 || box_height <= 0.0 {
        return bbox;
    }
    let distance = (box_width * box_height * ratio) / (2.0 * (box_width + box_height));
    BBox::new(
        bbox.x0 - distance,
        bbox.y0 - distance,
        bbox.x1 + distance,
        bbox.y1 + distance,
    )
    .clamp(width, height)
}

/// PaddleOCR's default DB `unclip_ratio`.
const DB_UNCLIP_RATIO: f32 = 1.5;

const CHW_PREPROC_MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const CHW_PREPROC_STD: [f32; 3] = [0.229, 0.224, 0.225];

/// 4-connected components over a binary mask. Used to turn the DBNet
/// probability map into boxes without pulling in OpenCV.
fn connected_components(mask: &[bool], width: usize, height: usize) -> Vec<BBox> {
    let mut visited = vec![false; width * height];
    let mut out = Vec::new();
    for start in 0..mask.len() {
        if !mask[start] || visited[start] {
            continue;
        }
        let mut stack = vec![start];
        let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
        while let Some(index) = stack.pop() {
            if visited[index] || !mask[index] {
                continue;
            }
            visited[index] = true;
            let x = index % width;
            let y = index / width;
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
            if x > 0 {
                stack.push(index - 1);
            }
            if x + 1 < width {
                stack.push(index + 1);
            }
            if y > 0 {
                stack.push(index - width);
            }
            if y + 1 < height {
                stack.push(index + width);
            }
        }
        if x0 != usize::MAX {
            out.push(BBox::new(
                x0 as f32,
                y0 as f32,
                (x1 + 1) as f32,
                (y1 + 1) as f32,
            ));
        }
    }
    out
}

/// Recognizes one line with a specific recognition session, returning the text
/// and the mean character confidence.
pub fn recognize_with(
    session: &mut Session,
    dictionary: &Dictionary,
    image: &DynamicImage,
    line: TextLine,
) -> OcrResult<(String, f32)> {
    let cropped = crop(image, line.bbox);
    let resized = resize_for_recognition(&cropped);
    let rec_width = resized.width() as usize;
    let data = rgb_to_rec_chw(&resized);

    let tensor = Tensor::from_array(([1usize, 3, REC_TARGET_HEIGHT as usize, rec_width], data))?;
    let outputs = session.run(inputs![tensor])?;
    let (shape, probabilities) = outputs[0].try_extract_tensor::<f32>()?;
    let (indices, confidence) = ctc_greedy(shape, probabilities);
    Ok((dictionary.decode(&indices), confidence))
}

/// Runs both recognition models and keeps the better result.
///
/// Arabic and English lines are interleaved in real documents, so the choice is
/// made per line. Mean log-probabilities are not comparable across two different
/// character sets — the Latin model happily emits plausible Latin gibberish for
/// an Arabic line at a similar score — so a script-consistent result that clears
/// [`ARABIC_PREFERENCE_MIN`] wins outright.
pub fn recognize_line(
    models: &ModelSet,
    image: &DynamicImage,
    line: TextLine,
) -> OcrResult<(String, f32)> {
    let (text_en, score_en) = {
        let mut session = models
            .rec_en
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        recognize_with(&mut session, &models.dict_en, image, line)?
    };
    let (text_ar, score_ar) = {
        let mut session = models
            .rec_ar
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        recognize_with(&mut session, &models.dict_ar, image, line)?
    };
    let text_en = if models.rec_en_visual {
        bidi::visual_to_logical(&text_en)
    } else {
        text_en
    };
    let text_ar = if models.rec_ar_visual {
        bidi::visual_to_logical(&text_ar)
    } else {
        text_ar
    };
    Ok(choose_recognition((text_en, score_en), (text_ar, score_ar)))
}

/// Confidence floor for preferring Arabic-script output over a marginally
/// higher-scoring Latin result.
const ARABIC_PREFERENCE_MIN: f32 = 0.5;

fn choose_recognition(english: (String, f32), arabic: (String, f32)) -> (String, f32) {
    if bidi::has_arabic(&arabic.0) && arabic.1 >= ARABIC_PREFERENCE_MIN {
        return arabic;
    }
    if arabic.1 > english.1 {
        arabic
    } else {
        english
    }
}

/// Greedy CTC over a `[T, C]` (or `[1, T, C]`) softmax output. Returns the class
/// indices and the mean max-probability as a confidence score.
fn ctc_greedy(shape: &[i64], data: &[f32]) -> (Vec<u32>, f32) {
    let (time, classes) = match shape.len() {
        1 => (1, shape[0] as usize),
        2 => (shape[0] as usize, shape[1] as usize),
        3 => (shape[1] as usize, shape[2] as usize),
        _ => return (Vec::new(), 0.0),
    };
    let mut indices = Vec::with_capacity(time);
    let mut log_sum = 0.0f32;
    for t in 0..time {
        let start = t * classes;
        let end = start + classes;
        if end > data.len() {
            break;
        }
        let (best_index, best) = data[start..end]
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(index, &value)| (index as u32, value))
            .unwrap_or((0, 0.0));
        indices.push(best_index);
        log_sum += best.max(1e-9).ln();
    }
    let confidence = (log_sum / time as f32).exp();
    (indices, confidence)
}

fn crop(image: &DynamicImage, bbox: BBox) -> DynamicImage {
    let (width, height) = image.dimensions();
    let box_rect = bbox.clamp(width, height);
    let x = box_rect.x0.max(0.0) as u32;
    let y = box_rect.y0.max(0.0) as u32;
    let w = box_rect.width().max(1.0) as u32;
    let h = box_rect.height().max(1.0) as u32;
    image.crop_imm(x, y, w.min(width - x), h.min(height - y))
}

fn resize_for_recognition(image: &DynamicImage) -> RgbImage {
    let rgb = image.to_rgb8();
    let (width, height) = (rgb.width(), rgb.height());
    if width == 0 || height == 0 {
        return RgbImage::from_pixel(REC_DEFAULT_WIDTH, REC_TARGET_HEIGHT, Rgb([255, 255, 255]));
    }
    let aspect = width as f32 / height as f32;
    let target_width = ((REC_TARGET_HEIGHT as f32 * aspect).round() as u32)
        .clamp(REC_TARGET_HEIGHT, REC_MAX_WIDTH);
    image::imageops::resize(
        &rgb,
        target_width,
        REC_TARGET_HEIGHT,
        image::imageops::FilterType::Triangle,
    )
}

/// PP-OCR recognition normalization: scale to 0..1, then `(x - 0.5) / 0.5`.
fn rgb_to_rec_chw(image: &RgbImage) -> Vec<f32> {
    let mut out = Vec::with_capacity((image.width() * image.height() * 3) as usize);
    for channel in 0..3 {
        for pixel in image.pixels() {
            let value = pixel.0[channel] as f32 / 255.0;
            out.push((value - 0.5) / 0.5);
        }
    }
    out
}

/// ImageNet normalization for the PicoDet layout model.
fn rgb_to_imagenet_chw(image: &RgbImage) -> Vec<f32> {
    const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
    const STD: [f32; 3] = [0.229, 0.224, 0.225];
    let mut out = Vec::with_capacity((image.width() * image.height() * 3) as usize);
    for channel in 0..3 {
        for pixel in image.pixels() {
            let value = pixel.0[channel] as f32 / 255.0;
            out.push((value - MEAN[channel]) / STD[channel]);
        }
    }
    out
}

/// Runs the PP-StructureV2 PicoDet layout model. The downloaded export emits
/// four score tensors `[N, C]` and four box tensors `[N, 32]` (one per FPN
/// level); `pico` decodes the distance bins and runs class-wise NMS. A single
/// `[N, 6]` packed tensor is accepted as a fallback.
pub fn detect_layout(models: &ModelSet, image: &DynamicImage) -> OcrResult<PageLayout> {
    let (width, height) = image.dimensions();
    // The PicoDet PP-StructureV2 layout export expects H=800, W=608.
    let (input_w, input_h) = (608u32, 800u32);
    let rgb = image.to_rgb8();
    let resized = image::imageops::resize(
        &rgb,
        input_w,
        input_h,
        image::imageops::FilterType::Triangle,
    );
    let data = rgb_to_imagenet_chw(&resized);
    let tensor = Tensor::from_array(([1usize, 3, input_h as usize, input_w as usize], data))?;

    let mut session = models
        .layout
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let outputs = session.run(inputs![tensor])?;

    if outputs.len() >= 8 {
        let mut scores = Vec::with_capacity(4);
        let mut boxes = Vec::with_capacity(4);
        for index in 0..4 {
            let (shape, data) = outputs[index].try_extract_tensor::<f32>()?;
            scores.push((&shape[..], data));
        }
        for index in 4..8 {
            let (shape, data) = outputs[index].try_extract_tensor::<f32>()?;
            boxes.push((&shape[..], data));
        }
        let detections = pico::decode_layout(
            &scores, &boxes, input_w, input_h, width, height, 0.3, 0.5, 100,
        );
        let mut regions = Vec::new();
        for detection in detections {
            if let Some(kind) = layout::classify_pico_label(detection.label) {
                regions.push(Region {
                    kind,
                    bbox: detection.bbox,
                    confidence: detection.score,
                    text: None,
                    table: None,
                });
            }
        }
        if !regions.is_empty() {
            layout::sort_reading_order(&mut regions, false);
            return Ok(PageLayout {
                width,
                height,
                regions,
            });
        }
    }
    if let Some(page) = parse_packed_layout(&outputs, width, height, input_w, input_h) {
        return Ok(page);
    }

    Ok(full_page_text(width, height))
}

/// Parses a single `[N, 6]` `(label, score, x0, y0, x1, y1)` layout tensor.
fn parse_packed_layout(
    outputs: &ort::session::SessionOutputs<'_>,
    width: u32,
    height: u32,
    input_w: u32,
    input_h: u32,
) -> Option<PageLayout> {
    let (shape, values) = outputs[0].try_extract_tensor::<f32>().ok()?;
    let cols = *shape.last().unwrap_or(&6) as usize;
    if cols < 6 {
        return None;
    }
    let rows = values.len() / cols;
    let mut regions = Vec::new();
    for row in 0..rows {
        let base = row * cols;
        if base + cols > values.len() {
            break;
        }
        let label = values[base].round() as i64;
        let score = values[base + 1];
        let bbox = BBox::new(
            values[base + 2] / input_w as f32 * width as f32,
            values[base + 3] / input_h as f32 * height as f32,
            values[base + 4] / input_w as f32 * width as f32,
            values[base + 5] / input_h as f32 * height as f32,
        );
        if let Some(kind) = layout::classify_label(label) {
            regions.push(Region {
                kind,
                bbox: bbox.clamp(width, height),
                confidence: score,
                text: None,
                table: None,
            });
        }
    }
    if regions.is_empty() {
        None
    } else {
        layout::sort_reading_order(&mut regions, false);
        Some(PageLayout {
            width,
            height,
            regions,
        })
    }
}

fn full_page_text(width: u32, height: u32) -> PageLayout {
    PageLayout {
        width,
        height,
        regions: vec![Region {
            kind: RegionKind::Text,
            bbox: BBox::new(0.0, 0.0, width as f32, height as f32),
            confidence: 1.0,
            text: None,
            table: None,
        }],
    }
}

/// Runs SLANet table-structure recognition on a table region.
///
/// The structure head emits `[N, 501, classes]` token scores and the location
/// head `[N, 501, 4]` normalized boxes. Decoding is a *sequence* problem: the
/// structural tokens (`<thead>`, `<tr>`, `</tr>`, ...) describe the grid and only
/// the cell tokens carry a real box. Treating every one of the 501 positions as
/// a cell would fabricate a dozen junk cells per row.
pub fn detect_table(
    models: &ModelSet,
    image: &DynamicImage,
    region: &Region,
) -> OcrResult<TableStructure> {
    // Layout boxes can extend past the page edge. Crop and map against the
    // clamped rectangle; using the unclamped one would shift every cell by the
    // clipped margin.
    let (image_width, image_height) = image.dimensions();
    let region_bbox = region.bbox.clamp(image_width, image_height);
    let cropped = crop(image, region_bbox);
    let rgb = cropped.to_rgb8();
    // SLANet is trained on 488x488 table crops: the long side is scaled to 488
    // and the remainder padded, preserving the table's aspect. Resizing straight
    // to 896x896 stretched wide tables until columns collapsed (a 3-column table
    // decoded as 2), which left recognized text with no cell to land in.
    const TABLE_INPUT_SIDE: u32 = 488;
    let (width, height) = rgb.dimensions();
    let scale = TABLE_INPUT_SIDE as f32 / width.max(height).max(1) as f32;
    let resized_w = ((width as f32 * scale).round() as u32).clamp(1, TABLE_INPUT_SIDE);
    let resized_h = ((height as f32 * scale).round() as u32).clamp(1, TABLE_INPUT_SIDE);
    let resized = image::imageops::resize(
        &rgb,
        resized_w,
        resized_h,
        image::imageops::FilterType::Triangle,
    );
    let mut canvas = RgbImage::from_pixel(TABLE_INPUT_SIDE, TABLE_INPUT_SIDE, Rgb([0, 0, 0]));
    image::imageops::replace(&mut canvas, &resized, 0, 0);
    let data = rgb_to_imagenet_chw(&canvas);
    let tensor = Tensor::from_array((
        [
            1usize,
            3,
            TABLE_INPUT_SIDE as usize,
            TABLE_INPUT_SIDE as usize,
        ],
        data,
    ))?;

    let mut session = models
        .table
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let outputs = session.run(inputs![tensor])?;

    let (_box_shape, boxes) = outputs[0].try_extract_tensor::<f32>()?;
    let (score_shape, scores) = outputs[1].try_extract_tensor::<f32>()?;
    let classes = (*score_shape.last().unwrap_or(&1)).max(1) as usize;
    let positions = boxes.len() / 4;

    // Boxes are normalized over the padded square canvas. Undo the padding so a
    // box lands on the text it belongs to instead of in the blank band below it.
    let x_scale = TABLE_INPUT_SIDE as f32 / resized_w as f32;
    let y_scale = TABLE_INPUT_SIDE as f32 / resized_h as f32;

    let mut tokens: Vec<i64> = Vec::with_capacity(positions);
    let mut normalized: Vec<BBox> = Vec::with_capacity(positions);
    for position in 0..positions {
        let score_base = position * classes;
        let box_base = position * 4;
        if score_base + classes > scores.len() || box_base + 4 > boxes.len() {
            break;
        }
        let (class, confidence) = scores[score_base..score_base + classes]
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(index, &value)| (index as i64, value))
            .unwrap_or((0, 0.0));
        // A low-confidence token is noise, not structure: drop it to the blank so
        // it cannot open a phantom row or cell.
        tokens.push(if confidence < STRUCTURE_TOKEN_THRESHOLD {
            0
        } else {
            class
        });
        normalized.push(BBox::new(
            (boxes[box_base] * x_scale).clamp(0.0, 1.0),
            (boxes[box_base + 1] * y_scale).clamp(0.0, 1.0),
            (boxes[box_base + 2] * x_scale).clamp(0.0, 1.0),
            (boxes[box_base + 3] * y_scale).clamp(0.0, 1.0),
        ));
    }

    let mut structure =
        table::decode_structure(&models.table_dict, &tokens, &normalized, region_bbox);
    // A structure head that produced no cell tokens leaves nothing to export;
    // fall back to a single cell covering the region instead of an empty table.
    if structure.cells.is_empty() {
        structure = TableStructure {
            bbox: region_bbox,
            rows: 1,
            cols: 1,
            cells: vec![table::TableCell {
                row: 0,
                col: 0,
                row_span: 1,
                col_span: 1,
                bbox: region_bbox,
                text: String::new(),
            }],
        };
    }
    Ok(structure)
}

/// Minimum argmax probability for a structure token to count as structure. The
/// shipped export scores real tokens at 0.57–0.92 and padding far below this.
const STRUCTURE_TOKEN_THRESHOLD: f32 = 0.3;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arabic_script_wins_over_latin_gibberish() {
        let chosen = choose_recognition(("+++ury".to_string(), 0.52), ("مرحبا".to_string(), 0.50));
        assert_eq!(chosen.0, "مرحبا");
    }

    #[test]
    fn a_low_confidence_arabic_result_loses_to_english() {
        let chosen = choose_recognition(("Invoice".to_string(), 0.90), (String::new(), 0.20));
        assert_eq!(chosen.0, "Invoice");
    }

    #[test]
    fn results_without_arabic_script_fall_back_to_confidence() {
        let chosen = choose_recognition(("1234".to_string(), 0.40), ("١٢٣٤".to_string(), 0.45));
        assert_eq!(chosen.0, "١٢٣٤");
        let chosen = choose_recognition(("Total".to_string(), 0.80), ("tl".to_string(), 0.30));
        assert_eq!(chosen.0, "Total");
    }

    /// ONNX Runtime allows one environment per process, and the shell rebuilds
    /// the engine whenever the user activates or removes a model pack
    /// (`AppState::reset_ocr_engine`). Taking the environment twice is therefore
    /// the difference between a pack switch that works and one that leaves OCR
    /// dead until the app is restarted.
    #[test]
    fn the_environment_is_handed_out_more_than_once() {
        let Ok(first) = environment() else {
            // No ONNX Runtime on this host: the rest of the suite skips the same
            // way rather than failing a machine that cannot run the models.
            eprintln!("skipping: ONNX Runtime is not loadable here");
            return;
        };
        let second = environment().expect("a second environment handle");
        drop((first, second));
    }

    /// A machine without `libonnxruntime.so` must get an error, not a dead
    /// process. `ort` panics inside `build()`, and the guard that turns the
    /// panic into `RuntimeUnavailable` is only reachable before the shared
    /// environment exists — a `OnceLock` cannot be unset — so the test re-runs
    /// itself in a child process with a bogus `ORT_DYLIB_PATH`.
    #[test]
    fn a_missing_runtime_is_an_error_not_a_panic() {
        const CHILD_FLAG: &str = "ZEIDEX_ONNX_MISSING_CHILD";
        if std::env::var_os(CHILD_FLAG).is_some() {
            match environment() {
                // A statically linked runtime ignores the path; there is
                // nothing to prove in that configuration.
                Ok(_) => println!("skip: this build does not load ORT_DYLIB_PATH"),
                Err(OcrError::RuntimeUnavailable(_)) => println!("unavailable, as expected"),
                Err(other) => panic!("expected RuntimeUnavailable, got {other}"),
            }
            return;
        }
        let exe = std::env::current_exe().expect("test binary path");
        let output = std::process::Command::new(exe)
            .args([
                "--exact",
                "onnx::tests::a_missing_runtime_is_an_error_not_a_panic",
                "--nocapture",
            ])
            .env(CHILD_FLAG, "1")
            .env(
                "ORT_DYLIB_PATH",
                "/nonexistent/libonnxruntime-does-not-exist.so",
            )
            .output()
            .expect("spawn the test binary");
        assert!(
            output.status.success(),
            "the child process died instead of returning an error (status {:?}):\n{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
