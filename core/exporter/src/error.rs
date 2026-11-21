use thiserror::Error;

/// Export failures. Both backends return their own error type; they convert
/// automatically so a caller only matches on `ExportError`.
#[derive(Debug, Error)]
pub enum ExportError {
    #[error("Word export error: {0}")]
    Docx(#[from] docx_rs::DocxError),

    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("Excel export error: {0}")]
    Xlsx(#[from] rust_xlsxwriter::XlsxError),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid input: {0}")]
    Invalid(String),
}

pub type ExportResult<T> = Result<T, ExportError>;
