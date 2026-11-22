use thiserror::Error;

/// Everything the OCR pipeline can fail on. `ort::Error` and `image::ImageError`
/// convert automatically; callers at the Tauri boundary map this to a string.
#[derive(Debug, Error)]
pub enum OcrError {
    #[error("ONNX runtime error: {0}")]
    Onnx(#[from] ort::Error),

    #[error("image error: {0}")]
    Image(#[from] image::ImageError),

    #[error("model error: {0}")]
    Model(String),

    #[error("pdf error: {0}")]
    Pdf(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid input: {0}")]
    Invalid(String),

    /// The caller asked the pipeline to stop between pages. Spelled exactly
    /// `cancelled` because the shell forwards the message to the UI, which
    /// shows a neutral notice instead of an error for this one case.
    #[error("cancelled")]
    Cancelled,
}

pub type OcrResult<T> = Result<T, OcrError>;
