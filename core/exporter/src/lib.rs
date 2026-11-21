#![deny(clippy::expect_used, clippy::unwrap_used)]

pub mod docx;
pub mod error;
pub mod xlsx;

pub use error::{ExportError, ExportResult};
