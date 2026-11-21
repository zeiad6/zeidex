//! Real-model table-structure test.
//!
//! Skips unless `ZEIDEX_TEST_MODELS_DIR`, `ZEIDEX_TEST_TABLE_IMAGE` and
//! `ORT_DYLIB_PATH` are set, so CI stays offline and fast. It exists because the
//! structure head is a *sequence* decoder: a regression that treats every one of
//! the 501 positions as a cell is invisible to unit tests but obvious here.

use image::GenericImageView;

use zeidex_ocr::error::OcrResult;
use zeidex_ocr::geometry::BBox;
use zeidex_ocr::layout::{Region, RegionKind};
use zeidex_ocr::onnx::{ModelPaths, detect_table, load_models};

#[test]
fn slanet_decodes_a_real_table_into_a_clean_grid() -> OcrResult<()> {
    let Ok(models_dir) = std::env::var("ZEIDEX_TEST_MODELS_DIR") else {
        eprintln!("skipping: ZEIDEX_TEST_MODELS_DIR not set");
        return Ok(());
    };
    let Ok(image_path) = std::env::var("ZEIDEX_TEST_TABLE_IMAGE") else {
        eprintln!("skipping: ZEIDEX_TEST_TABLE_IMAGE not set");
        return Ok(());
    };

    let models = load_models(&ModelPaths::from_dir(&models_dir))?;
    let image = image::open(&image_path).map_err(zeidex_ocr::OcrError::Image)?;
    let (width, height) = image.dimensions();

    let region = Region {
        kind: RegionKind::Table,
        bbox: BBox::new(0.0, 0.0, width as f32, height as f32),
        confidence: 1.0,
        text: None,
        table: None,
    };
    let structure = detect_table(&models, &image, &region)?;

    eprintln!(
        "table {}x{} with {} cells",
        structure.rows,
        structure.cols,
        structure.cells.len()
    );
    for cell in &structure.cells {
        eprintln!(
            "  ({}, {}) span {}x{} {:?}",
            cell.row, cell.col, cell.row_span, cell.col_span, cell.bbox
        );
    }

    // The fixture is a 4-row x 3-column grid (header + 3 data rows).
    assert!(structure.rows >= 2, "no rows decoded");
    assert!(structure.cols >= 2, "no columns decoded");
    // The bug this test guards: structural tokens must not become cells.
    assert!(
        structure.cells.len() <= structure.rows * structure.cols,
        "decoder fabricated {} cells for a {}x{} grid",
        structure.cells.len(),
        structure.rows,
        structure.cols
    );
    // Every cell must be anchored inside the grid.
    for cell in &structure.cells {
        assert!(cell.row < structure.rows, "cell row outside the grid");
        assert!(cell.col < structure.cols, "cell col outside the grid");
        assert!(cell.bbox.width() > 0.0 && cell.bbox.height() > 0.0);
    }
    Ok(())
}
