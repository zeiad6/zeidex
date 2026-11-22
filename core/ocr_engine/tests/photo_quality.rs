//! How recognition holds up when a page arrives as a photo instead of a scan.
//!
//! Skips unless `ZEIDEX_TEST_MODELS_DIR` and `ZEIDEX_TEST_IMAGE` are set and
//! `ORT_DYLIB_PATH` names an ONNX Runtime shared library. The fixture must be
//! `table.png`, whose twelve cells are known.
//!
//! Each variant is derived from that one fixture in memory, so the only thing
//! that changes is the pixel quality: contrast, uneven light, blur, sensor
//! noise, JPEG artifacts, and all of them together. The test prints exact cell
//! hits per variant and fails only if the combined photo case drops below the
//! floor recorded here. That floor is the evidence any future enhancement step
//! has to beat, not a promise that photos are perfect.

use std::io::Cursor;
use std::path::Path;

use image::{DynamicImage, Rgb, RgbImage, imageops};
use zeidex_ocr::error::OcrResult;
use zeidex_ocr::onnx::{ModelPaths, detect_text_lines, load_models, recognize_line};

const EXPECTED: [&str; 12] = [
    "item", "quantity", "price", "widget a", "12", "19.99", "widget b", "7", "4.50", "gadget c",
    "3", "129.00",
];

/// Exact cells the combined photo case must still read. Measured at 12/12 with
/// the current models; the floor leaves room for a model update without hiding
/// a real regression.
const PHOTO_FLOOR: usize = 10;

fn normalize(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn hits(texts: &[String]) -> usize {
    let mut remaining = EXPECTED.to_vec();
    let mut hits = 0;
    for text in texts {
        if let Some(index) = remaining.iter().position(|expected| expected == text) {
            remaining.remove(index);
            hits += 1;
        }
    }
    hits
}

/// A tiny deterministic generator, so a rerun compares the same pixels.
struct Noise(u32);

impl Noise {
    fn next(&mut self) -> i32 {
        // xorshift32
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 % 41) as i32 - 20
    }
}

fn clamp(value: f32) -> u8 {
    value.round().clamp(0.0, 255.0) as u8
}

fn map_pixels(source: &DynamicImage, mut f: impl FnMut(u32, u32, u8) -> u8) -> DynamicImage {
    let mut out = RgbImage::new(source.width(), source.height());
    for (x, y, pixel) in source.to_rgb8().enumerate_pixels() {
        out.put_pixel(
            x,
            y,
            Rgb([f(x, y, pixel[0]), f(x, y, pixel[1]), f(x, y, pixel[2])]),
        );
    }
    DynamicImage::ImageRgb8(out)
}

fn low_contrast(source: &DynamicImage) -> DynamicImage {
    map_pixels(source, |_, _, channel| {
        clamp(128.0 + (f32::from(channel) - 128.0) * 0.45)
    })
}

fn dim_lighting(source: &DynamicImage) -> DynamicImage {
    let width = source.width().max(1) as f32;
    map_pixels(source, |x, _, channel| {
        let light = 0.55 + 0.45 * (x as f32 / width);
        clamp(f32::from(channel) * light)
    })
}

fn blurred(source: &DynamicImage) -> DynamicImage {
    DynamicImage::ImageRgb8(imageops::blur(&source.to_rgb8(), 1.6))
}

fn noisy(source: &DynamicImage) -> DynamicImage {
    let mut noise = Noise(0x5eed_1234);
    map_pixels(source, |_, _, channel| {
        clamp(f32::from(channel) + noise.next() as f32)
    })
}

fn jpeg(source: &DynamicImage, quality: u8) -> DynamicImage {
    let mut encoded = Cursor::new(Vec::new());
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, quality);
    encoder
        .encode_image(source)
        .expect("in-memory JPEG encoding");
    image::load_from_memory(&encoded.into_inner())
        .expect("in-memory JPEG decoding")
        .to_rgb8()
        .into()
}

/// All the ways a handheld photo differs from a flatbed scan.
fn photo(source: &DynamicImage) -> DynamicImage {
    let dim = dim_lighting(source);
    let low = low_contrast(&dim);
    let soft = blurred(&low);
    let grain = noisy(&soft);
    jpeg(&grain, 45)
}

fn read_cells(
    models: &zeidex_ocr::onnx::ModelSet,
    image: &DynamicImage,
) -> OcrResult<(usize, Vec<String>)> {
    let lines = detect_text_lines(models, image)?;
    let mut texts = Vec::new();
    for line in &lines {
        let (text, _confidence) = recognize_line(models, image, *line)?;
        let normalized = normalize(&text);
        if !normalized.is_empty() {
            texts.push(normalized);
        }
    }
    let count = hits(&texts);
    Ok((count, texts))
}

#[test]
fn degraded_photos_still_read_the_table() -> OcrResult<()> {
    let Ok(models_dir) = std::env::var("ZEIDEX_TEST_MODELS_DIR") else {
        eprintln!("skipping: ZEIDEX_TEST_MODELS_DIR not set");
        return Ok(());
    };
    let Ok(image_path) = std::env::var("ZEIDEX_TEST_IMAGE") else {
        eprintln!("skipping: ZEIDEX_TEST_IMAGE not set");
        return Ok(());
    };
    if Path::new(&image_path)
        .file_name()
        .and_then(|name| name.to_str())
        != Some("table.png")
    {
        eprintln!("skipping: fixture is not table.png");
        return Ok(());
    }

    let models = load_models(&ModelPaths::from_dir(&models_dir))?;
    let source = image::open(&image_path).map_err(zeidex_ocr::OcrError::Image)?;

    let variants: [(&str, DynamicImage); 7] = [
        ("clean", source.clone()),
        ("low contrast", low_contrast(&source)),
        ("dim lighting", dim_lighting(&source)),
        ("blur 1.6", blurred(&source)),
        ("sensor noise", noisy(&source)),
        ("jpeg q45", jpeg(&source, 45)),
        ("photo", photo(&source)),
    ];

    let mut photo_hits = 0;
    for (name, image) in &variants {
        let (count, texts) = read_cells(&models, image)?;
        eprintln!("{name:>13}: {count}/12 cells");
        if *name == "photo" {
            photo_hits = count;
            eprintln!("  photo text: {texts:?}");
        }
    }

    assert!(
        photo_hits >= PHOTO_FLOOR,
        "a combined photo case read {photo_hits}/12 cells, below the recorded floor of {PHOTO_FLOOR}"
    );
    Ok(())
}
