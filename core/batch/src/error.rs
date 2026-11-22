use thiserror::Error;

pub type BatchResult<T> = Result<T, BatchError>;

#[derive(Debug, Error)]
pub enum BatchError {
    /// Building or saving the workbook failed. `rust_xlsxwriter` reports IO
    /// errors through the same type, so a full disk and a bad sheet name both
    /// arrive here with their own message.
    #[error("write the report: {0}")]
    Workbook(#[from] rust_xlsxwriter::XlsxError),
}
