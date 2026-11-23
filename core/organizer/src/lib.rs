//! Deterministic OCR output organizer.
//!
//! The OCR engine answers "what characters are on this page". This crate
//! answers "what is this page" - which blocks are titles, paragraphs, lists,
//! key/value pairs, dates, amounts, contacts and tables - without rewriting a
//! single character. Every table carries a confidence, and a table below the
//! review threshold is flagged instead of being silently corrected.
//!
//! The output is the same block list for every serializer, so TXT, CSV, JSON,
//! Markdown and HTML cannot disagree about the content.

pub mod detect;
pub mod extract;
pub mod model;
pub mod render;

pub use extract::{
    BatchColumn, BatchExtraction, BatchRow, DocumentKind, Extraction, Field, detect_kind, extract,
    extract_as, extract_batch, extract_batch_as, fold,
};
pub use model::*;

/// Organizes a source document. This is the only entry point the shell needs.
pub fn organize(source: &SourceDocument) -> OrganizedDocument {
    detect::run(source)
}

/// Convenience for pasted text: every blank-line-separated paragraph becomes a
/// text block.
pub fn organize_text(text: &str) -> OrganizedDocument {
    let blocks: Vec<SourceBlock> = text
        .split("\n\n")
        .map(str::trim)
        .filter(|paragraph| !paragraph.is_empty())
        .map(SourceBlock::text)
        .collect();
    organize(&SourceDocument {
        pages: vec![SourcePage { blocks }],
    })
}
