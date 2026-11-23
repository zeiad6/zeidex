//! Data model for the organizer.
//!
//! The input types are deliberately independent of the OCR engine: the shell
//! maps a `PageResult` onto [`SourceDocument`], and the same types can be built
//! from pasted text in a test. The output types are the wire shape the Studio
//! previews and the exporters serialize.

use serde::{Deserialize, Serialize};

/// One page of source blocks, in reading order.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SourceDocument {
    pub pages: Vec<SourcePage>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SourcePage {
    pub blocks: Vec<SourceBlock>,
}

/// A block as the OCR pipeline saw it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SourceBlock {
    /// Layout label (`text`, `title`, `list`, `table`, `caption`, ...).
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub text: String,
    /// Reconstructed grid for a table region.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<SourceTable>,
    /// Layout/recognition confidence, 0..=1.
    #[serde(default = "full_confidence")]
    pub confidence: f32,
}

fn full_confidence() -> f32 {
    1.0
}

impl SourceBlock {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            kind: "text".to_string(),
            text: text.into(),
            table: None,
            confidence: 1.0,
        }
    }

    pub fn titled(text: impl Into<String>) -> Self {
        Self {
            kind: "title".to_string(),
            text: text.into(),
            table: None,
            confidence: 1.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SourceTable {
    pub rows: usize,
    pub cols: usize,
    pub cells: Vec<SourceCell>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SourceCell {
    pub row: usize,
    pub col: usize,
    #[serde(default = "one")]
    pub row_span: usize,
    #[serde(default = "one")]
    pub col_span: usize,
    #[serde(default)]
    pub text: String,
}

fn one() -> usize {
    1
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockKind {
    Title,
    Heading,
    Paragraph,
    List,
    KeyValue,
    Table,
    Contact,
    Date,
    Amount,
    Figure,
    Equation,
    Header,
    Footer,
    Caption,
}

impl BlockKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Heading => "heading",
            Self::Paragraph => "paragraph",
            Self::List => "list",
            Self::KeyValue => "key_value",
            Self::Table => "table",
            Self::Contact => "contact",
            Self::Date => "date",
            Self::Amount => "amount",
            Self::Figure => "figure",
            Self::Equation => "equation",
            Self::Header => "header",
            Self::Footer => "footer",
            Self::Caption => "caption",
        }
    }
}

/// A table after reconstruction. `header` is empty when the first row did not
/// look like a header; `rows` then contains every row.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Table {
    pub header: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub columns: usize,
    /// 0..=1. Derived from row consistency and cell fill, never invented.
    pub confidence: f32,
    /// True when the detector is not sure enough to export silently.
    pub needs_review: bool,
}

impl Table {
    pub fn is_empty(&self) -> bool {
        self.header.iter().all(|cell| cell.trim().is_empty()) && self.rows.is_empty()
    }

    /// Every row, with the header first when one was detected.
    pub fn all_rows(&self) -> Vec<&[String]> {
        let mut rows: Vec<&[String]> = Vec::with_capacity(self.rows.len() + 1);
        if !self.header.iter().all(|cell| cell.trim().is_empty()) {
            rows.push(&self.header);
        }
        rows.extend(self.rows.iter().map(Vec::as_slice));
        rows
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Block {
    pub kind: BlockKind,
    /// Recognized text, with runs of whitespace collapsed. Numbers and letters
    /// are never rewritten.
    pub text: String,
    /// Heading level: 1 for a document title, 2 for a section heading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<u8>,
    /// List items when `kind` is `list`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Table when `kind` is `table`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<Table>,
    /// Confidence of the classification, not of the OCR text.
    pub confidence: f32,
    /// Index of the source block this came from.
    pub source_index: usize,
    /// Page the block came from. Batch extraction groups by this, so a page
    /// boundary is never crossed and rows cannot interleave.
    pub page: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    Arabic,
    English,
    Mixed,
    Unknown,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    pub characters: usize,
    pub words: usize,
    /// Share of letters that are Arabic, 0..=1.
    pub arabic_ratio: f32,
    pub blocks: usize,
    pub tables: usize,
    pub needs_review: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Warning {
    /// Stable identifier for the UI to translate.
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OrganizedDocument {
    /// Bumped when the wire shape changes.
    pub schema: u32,
    /// Number of source pages. Batch extraction emits exactly one row per page,
    /// including pages that produced no text.
    pub pages: usize,
    pub language: Language,
    pub blocks: Vec<Block>,
    pub stats: Stats,
    pub warnings: Vec<Warning>,
}

impl OrganizedDocument {
    pub fn plain_text(&self) -> String {
        crate::render::text(self)
    }

    pub fn to_json(&self) -> String {
        crate::render::json(self)
    }

    pub fn to_csv(&self) -> String {
        crate::render::csv(self)
    }

    pub fn to_markdown(&self) -> String {
        crate::render::markdown(self)
    }

    pub fn to_html(&self) -> String {
        crate::render::html(self)
    }

    /// Parses a document the Studio edited and sent back. Returns a message
    /// rather than panicking so a malformed payload cannot take down the shell.
    pub fn from_json(text: &str) -> Result<Self, String> {
        serde_json::from_str(text).map_err(|error| error.to_string())
    }
}
