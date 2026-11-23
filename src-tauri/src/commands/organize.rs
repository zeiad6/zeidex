//! OCR output organization, document profiles and text-format export.
//!
//! The OCR engine's `PageResult` is mapped onto the organizer's neutral source
//! model here, in the shell, so the organizer crate stays independent of ONNX,
//! PDFium and the rest of the recognition stack.

use serde::Deserialize;
use tauri::{AppHandle, State};
use zeidex_ocr::{PageResult, RegionKind};
use zeidex_organizer::{
    BatchExtraction, DocumentKind, Extraction, OrganizedDocument, SourceBlock, SourceCell,
    SourceDocument, SourcePage, SourceTable,
};

use super::dialogs;
use crate::state::AppState;

fn region_kind(kind: RegionKind) -> &'static str {
    match kind {
        RegionKind::Text => "text",
        RegionKind::Title => "title",
        RegionKind::List => "list",
        RegionKind::Table => "table",
        RegionKind::Figure => "figure",
        RegionKind::Equation => "equation",
        RegionKind::Header => "header",
        RegionKind::Footer => "footer",
        RegionKind::Caption => "caption",
    }
}

/// Converts the recognized pages into the organizer's input.
///
/// A page whose layout is empty (for example a text-only document read through
/// the native-text path) falls back to its plain text, split on blank lines, so
/// the Studio can still organize and export it.
pub fn source_document(pages: &[PageResult]) -> SourceDocument {
    SourceDocument {
        pages: pages
            .iter()
            .map(|page| {
                let mut blocks: Vec<SourceBlock> = page
                    .layout
                    .regions
                    .iter()
                    .filter_map(|region| {
                        let text = region.text.clone().unwrap_or_default();
                        let table = region.table.as_ref().map(|table| SourceTable {
                            rows: table.rows,
                            cols: table.cols,
                            cells: table
                                .cells
                                .iter()
                                .map(|cell| SourceCell {
                                    row: cell.row,
                                    col: cell.col,
                                    row_span: cell.row_span,
                                    col_span: cell.col_span,
                                    text: cell.text.clone(),
                                })
                                .collect(),
                        });
                        if text.trim().is_empty() && table.is_none() {
                            return None;
                        }
                        Some(SourceBlock {
                            kind: region_kind(region.kind).to_string(),
                            text,
                            table,
                            confidence: region.confidence,
                        })
                    })
                    .collect();

                if blocks.is_empty() {
                    blocks = page
                        .text
                        .split("\n\n")
                        .map(str::trim)
                        .filter(|paragraph| !paragraph.is_empty())
                        .map(|paragraph| SourceBlock {
                            kind: "text".to_string(),
                            text: paragraph.to_string(),
                            table: None,
                            confidence: 1.0,
                        })
                        .collect();
                }

                SourcePage { blocks }
            })
            .collect(),
    }
}

/// Builds the document the Studio previews. Pure, so it can run on the caller's
/// thread; the pages are already in memory.
#[tauri::command]
pub fn organize_pages(pages: Vec<PageResult>) -> OrganizedDocument {
    zeidex_organizer::organize(&source_document(&pages))
}

/// Extracts the profile fields for one document.
///
/// `profile` is the profile the user picked by hand; when it is absent or not
/// a known id the detector decides. A manual choice is never overridden, so a
/// wrong guess is always recoverable.
#[tauri::command]
pub fn extract_document(pages: Vec<PageResult>, profile: Option<String>) -> Extraction {
    let document = zeidex_organizer::organize(&source_document(&pages));
    match profile.as_deref().and_then(DocumentKind::from_id) {
        Some(kind) => zeidex_organizer::extract_as(&document, kind),
        None => zeidex_organizer::extract(&document),
    }
}

/// Extracts every page as its own document and returns one table.
///
/// This is the batch path for a stack of ID cards, invoices or receipts: one
/// row per page, in page order, with a blank row kept for a page that produced
/// no fields. `profile` is the same manual override the single-document command
/// takes, so a corrected document type cannot leave the table on the old one.
#[tauri::command]
pub fn extract_document_batch(pages: Vec<PageResult>, profile: Option<String>) -> BatchExtraction {
    let document = zeidex_organizer::organize(&source_document(&pages));
    match profile.as_deref().and_then(DocumentKind::from_id) {
        Some(kind) => zeidex_organizer::extract_batch_as(&document, kind),
        None => zeidex_organizer::extract_batch(&document),
    }
}

/// Maps an export format to its extension and body. `None` means the format is
/// not one this command writes.
fn render_for_export(format: &str, document: &OrganizedDocument) -> Option<(&'static str, String)> {
    match format {
        "txt" => Some(("txt", document.plain_text())),
        "csv" => Some(("csv", document.to_csv())),
        "json" => Some(("json", document.to_json())),
        "md" => Some(("md", document.to_markdown())),
        "html" => Some(("html", document.to_html())),
        _ => None,
    }
}

/// Batch formats. JSON keeps the batch shape (columns, rows, per-row
/// confidence) because that is the data a caller re-imports.
fn render_batch_for_export(
    format: &str,
    batch: &BatchExtraction,
) -> Option<(&'static str, String)> {
    match format {
        "txt" => Some(("txt", batch.to_document().plain_text())),
        "csv" => Some(("csv", batch.to_csv())),
        "json" => Some(("json", batch.to_json())),
        "md" => Some(("md", batch.to_markdown())),
        "html" => Some(("html", batch.to_document().to_html())),
        _ => None,
    }
}

/// Maps an extraction (the field table) to its extension and body, so a user
/// who corrected a field exports exactly what they see.
fn render_extraction_for_export(
    format: &str,
    extraction: &Extraction,
) -> Option<(&'static str, String)> {
    match format {
        "txt" => Some(("txt", extraction.plain_text())),
        "csv" => Some(("csv", extraction.to_csv())),
        "json" => Some(("json", extraction.to_json())),
        "md" => Some(("md", extraction.to_markdown())),
        "html" => Some(("html", extraction.to_html())),
        _ => None,
    }
}

/// Writes the organized document in a text format.
///
/// The save dialog runs in Rust, like the DOCX/XLSX exporters, so the page never
/// chooses the path it writes to. Returns the path written, or `None` when the
/// dialog was dismissed.
#[tauri::command(async)]
pub fn export_organized(
    app: AppHandle,
    state: State<'_, AppState>,
    document: OrganizedDocument,
    format: String,
    default_name: Option<String>,
    title: Option<String>,
    filter_name: Option<String>,
) -> Result<Option<String>, String> {
    let Some((extension, body)) = render_for_export(&format, &document) else {
        return Err(format!("Unsupported export format: {format}"));
    };
    let Some(target) =
        dialogs::save_file(&app, title, filter_name, extension, default_name.as_deref())?
    else {
        return Ok(None);
    };
    dialogs::write_atomically(&target, |path| {
        std::fs::write(path, body.as_bytes()).map_err(|error| error.to_string())
    })?;
    state.paths.mint(&target);
    Ok(Some(target.display().to_string()))
}

/// What a preview renders. Tagged so one command serves all three shapes and
/// the preview can never drift from the exporter: both call the same function.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExportPreview {
    Organized { document: OrganizedDocument },
    Extraction { extraction: Extraction },
    Batch { batch: BatchExtraction },
}

/// Renders an export body without touching the disk, for the preview pane.
#[tauri::command]
pub fn preview_export(payload: ExportPreview, format: String) -> Result<String, String> {
    let rendered = match &payload {
        ExportPreview::Organized { document } => render_for_export(&format, document),
        ExportPreview::Extraction { extraction } => {
            render_extraction_for_export(&format, extraction)
        }
        ExportPreview::Batch { batch } => render_batch_for_export(&format, batch),
    };
    rendered
        .map(|(_, body)| body)
        .ok_or_else(|| format!("Unsupported preview format: {format}"))
}

/// Writes one document's field table. Same dialog and atomic write as every
/// other export.
#[tauri::command(async)]
pub fn export_extraction(
    app: AppHandle,
    state: State<'_, AppState>,
    extraction: Extraction,
    format: String,
    default_name: Option<String>,
    title: Option<String>,
    filter_name: Option<String>,
) -> Result<Option<String>, String> {
    let Some((extension, body)) = render_extraction_for_export(&format, &extraction) else {
        return Err(format!("Unsupported export format: {format}"));
    };
    let Some(target) =
        dialogs::save_file(&app, title, filter_name, extension, default_name.as_deref())?
    else {
        return Ok(None);
    };
    dialogs::write_atomically(&target, |path| {
        std::fs::write(path, body.as_bytes()).map_err(|error| error.to_string())
    })?;
    state.paths.mint(&target);
    Ok(Some(target.display().to_string()))
}

/// Writes a batch table. Same dialog and atomic write as every other export.
#[tauri::command(async)]
pub fn export_batch(
    app: AppHandle,
    state: State<'_, AppState>,
    batch: BatchExtraction,
    format: String,
    default_name: Option<String>,
    title: Option<String>,
    filter_name: Option<String>,
) -> Result<Option<String>, String> {
    let Some((extension, body)) = render_batch_for_export(&format, &batch) else {
        return Err(format!("Unsupported export format: {format}"));
    };
    let Some(target) =
        dialogs::save_file(&app, title, filter_name, extension, default_name.as_deref())?
    else {
        return Ok(None);
    };
    dialogs::write_atomically(&target, |path| {
        std::fs::write(path, body.as_bytes()).map_err(|error| error.to_string())
    })?;
    state.paths.mint(&target);
    Ok(Some(target.display().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeidex_ocr::{BBox, PageLayout, Region, TableCell, TableStructure};

    fn text_page(index: usize, text: &str) -> PageResult {
        PageResult {
            page_index: index,
            width: 100,
            height: 100,
            layout: PageLayout {
                width: 100,
                height: 100,
                regions: vec![Region {
                    kind: RegionKind::Text,
                    bbox: BBox::default(),
                    confidence: 1.0,
                    text: Some(text.to_string()),
                    table: None,
                }],
            },
            text: text.to_string(),
        }
    }

    fn card_page(index: usize, name: &str, national_id: &str) -> PageResult {
        PageResult {
            page_index: index,
            width: 100,
            height: 100,
            layout: PageLayout {
                width: 100,
                height: 100,
                regions: vec![
                    Region {
                        kind: RegionKind::Title,
                        bbox: BBox::default(),
                        confidence: 0.95,
                        text: Some("بطاقة تحقيق الشخصية".to_string()),
                        table: None,
                    },
                    Region {
                        kind: RegionKind::Text,
                        bbox: BBox::default(),
                        confidence: 0.95,
                        text: Some(format!("الاسم: {name}")),
                        table: None,
                    },
                    Region {
                        kind: RegionKind::Text,
                        bbox: BBox::default(),
                        confidence: 0.95,
                        text: Some(format!("الرقم القومي: {national_id}")),
                        table: None,
                    },
                ],
            },
            text: format!("بطاقة تحقيق الشخصية\nالاسم: {name}\nالرقم القومي: {national_id}"),
        }
    }

    fn page_with_table() -> PageResult {
        PageResult {
            page_index: 0,
            width: 100,
            height: 100,
            layout: PageLayout {
                width: 100,
                height: 100,
                regions: vec![
                    Region {
                        kind: RegionKind::Title,
                        bbox: BBox::default(),
                        confidence: 0.95,
                        text: Some("Report".to_string()),
                        table: None,
                    },
                    Region {
                        kind: RegionKind::Table,
                        bbox: BBox::default(),
                        confidence: 0.9,
                        text: None,
                        table: Some(TableStructure {
                            bbox: BBox::default(),
                            rows: 2,
                            cols: 2,
                            cells: vec![
                                TableCell {
                                    row: 0,
                                    col: 0,
                                    row_span: 1,
                                    col_span: 1,
                                    bbox: BBox::default(),
                                    text: "Name".to_string(),
                                },
                                TableCell {
                                    row: 0,
                                    col: 1,
                                    row_span: 1,
                                    col_span: 1,
                                    bbox: BBox::default(),
                                    text: "Qty".to_string(),
                                },
                                TableCell {
                                    row: 1,
                                    col: 0,
                                    row_span: 1,
                                    col_span: 1,
                                    bbox: BBox::default(),
                                    text: "Pen".to_string(),
                                },
                                TableCell {
                                    row: 1,
                                    col: 1,
                                    row_span: 1,
                                    col_span: 1,
                                    bbox: BBox::default(),
                                    text: "3".to_string(),
                                },
                            ],
                        }),
                    },
                ],
            },
            text: "Report".to_string(),
        }
    }

    #[test]
    fn a_layout_table_survives_the_mapping() {
        let document = zeidex_organizer::organize(&source_document(&[page_with_table()]));
        assert_eq!(document.blocks.len(), 2);
        assert_eq!(document.blocks[0].kind, zeidex_organizer::BlockKind::Title);
        let table = document.blocks[1].table.as_ref().expect("table block");
        assert_eq!(table.header, vec!["Name", "Qty"]);
        assert_eq!(table.rows, vec![vec!["Pen", "3"]]);
    }

    #[test]
    fn an_empty_layout_falls_back_to_the_page_text() {
        let page = PageResult {
            page_index: 0,
            width: 10,
            height: 10,
            layout: PageLayout {
                width: 10,
                height: 10,
                regions: Vec::new(),
            },
            text: "Alpha\n\nBeta".to_string(),
        };
        let document = zeidex_organizer::organize(&source_document(&[page]));
        assert_eq!(document.blocks.len(), 2);
        assert_eq!(document.blocks[0].text, "Alpha");
        assert_eq!(document.blocks[1].text, "Beta");
    }

    #[test]
    fn every_text_format_has_an_extension() {
        let document = zeidex_organizer::organize_text("Total: 42");
        for (format, extension) in [
            ("txt", "txt"),
            ("csv", "csv"),
            ("json", "json"),
            ("md", "md"),
            ("html", "html"),
        ] {
            let (actual, body) = render_for_export(format, &document).expect("format");
            assert_eq!(actual, extension);
            assert!(!body.is_empty());
        }
        assert!(render_for_export("docx", &document).is_none());
        assert!(render_for_export("exe", &document).is_none());
    }

    #[test]
    fn a_stack_of_cards_keeps_one_row_per_page() {
        let pages = vec![
            card_page(0, "أحمد محمد علي", "29801011234567"),
            card_page(1, "منى سعيد حسن", "30105234567890"),
        ];
        let batch = extract_document_batch(pages, None);
        assert_eq!(batch.rows.len(), 2);
        let name = batch
            .columns
            .iter()
            .position(|column| column.key == "full_name")
            .expect("name column");
        assert_eq!(batch.rows[0].values[name], "أحمد محمد علي");
        assert_eq!(batch.rows[1].values[name], "منى سعيد حسن");
    }

    #[test]
    fn a_manual_profile_reaches_the_batch_table() {
        let pages = vec![
            card_page(0, "أحمد محمد علي", "29801011234567"),
            card_page(1, "منى سعيد حسن", "30105234567890"),
        ];
        let batch = extract_document_batch(pages, Some("passport".to_string()));
        assert_eq!(batch.kind, DocumentKind::Passport);
        assert_eq!(batch.confidence, 1.0);
        assert_eq!(batch.rows.len(), 2);
        assert!(
            batch
                .columns
                .iter()
                .any(|column| column.key == "passport_number")
        );
    }

    #[test]
    fn a_manual_profile_is_used() {
        let extraction = extract_document(
            vec![text_page(0, "Invoice No: 7\nTotal: 10.00")],
            Some("invoice".to_string()),
        );
        assert_eq!(extraction.kind, DocumentKind::Invoice);
        assert_eq!(extraction.confidence, 1.0);
    }

    #[test]
    fn an_unknown_profile_id_falls_back_to_detection() {
        let extraction = extract_document(
            vec![text_page(0, "Invoice No: 7\nTotal: 10.00")],
            Some("not_a_profile".to_string()),
        );
        assert_eq!(extraction.kind, DocumentKind::Invoice);
    }

    #[test]
    fn a_preview_matches_the_exported_body() {
        let document = zeidex_organizer::organize_text("Total: 42");
        let preview = preview_export(
            ExportPreview::Organized {
                document: document.clone(),
            },
            "csv".to_string(),
        )
        .expect("preview");
        let (_, exported) = render_for_export("csv", &document).expect("export");
        assert_eq!(preview, exported);
    }

    #[test]
    fn an_unsupported_preview_format_is_an_error() {
        let document = zeidex_organizer::organize_text("Total: 42");
        assert!(preview_export(ExportPreview::Organized { document }, "pdf".to_string()).is_err());
    }

    #[test]
    fn every_extraction_format_has_an_extension() {
        let extraction = extract_document(vec![text_page(0, "Invoice No: 7\nTotal: 10.00")], None);
        for (format, extension) in [
            ("txt", "txt"),
            ("csv", "csv"),
            ("json", "json"),
            ("md", "md"),
            ("html", "html"),
        ] {
            let (actual, body) = render_extraction_for_export(format, &extraction).expect("format");
            assert_eq!(actual, extension);
            assert!(!body.is_empty());
        }
        assert!(render_extraction_for_export("pdf", &extraction).is_none());
    }

    #[test]
    fn a_corrected_field_value_is_what_gets_exported() {
        let mut extraction =
            extract_document(vec![text_page(0, "Invoice No: 7\nTotal: 10.00")], None);
        let total = extraction
            .fields
            .iter_mut()
            .find(|field| field.key == "total")
            .expect("total field");
        total.value = "11.50".to_string();
        let (_, csv) = render_extraction_for_export("csv", &extraction).expect("csv");
        assert!(csv.contains("11.50"), "{csv}");
        assert!(!csv.contains("10.00"), "{csv}");
    }

    #[test]
    fn every_batch_format_has_an_extension() {
        let batch = extract_document_batch(vec![card_page(0, "أحمد", "29801011234567")], None);
        for (format, extension) in [
            ("txt", "txt"),
            ("csv", "csv"),
            ("json", "json"),
            ("md", "md"),
            ("html", "html"),
        ] {
            let (actual, body) = render_batch_for_export(format, &batch).expect("format");
            assert_eq!(actual, extension);
            assert!(!body.is_empty());
        }
        assert!(render_batch_for_export("pdf", &batch).is_none());
    }
}
