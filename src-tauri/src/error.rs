use thiserror::Error;

/// Shell-level errors. Command handlers convert this to `String` at the Tauri
/// boundary; inside the crate every fallible path returns `AppResult`.
#[derive(Debug, Error)]
pub enum AppError {
    #[error("storage error: {0}")]
    Storage(#[from] zeidex_storage::StorageError),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("clipboard error: {0}")]
    Clipboard(String),

    #[error("ocr error: {0}")]
    Ocr(#[from] zeidex_ocr::OcrError),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("invalid argument: {0}")]
    Invalid(String),

    #[error("{0}")]
    Other(String),
}

impl From<arboard::Error> for AppError {
    fn from(value: arboard::Error) -> Self {
        Self::Clipboard(value.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
