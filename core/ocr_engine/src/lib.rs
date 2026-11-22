#![deny(clippy::expect_used, clippy::unwrap_used)]

pub mod bidi;
pub mod dict;
pub mod error;
pub mod geometry;
pub mod imageio;
pub mod layout;
pub mod onnx;
pub mod pdf;
pub mod pico;
pub mod pipeline;
pub mod table;

pub use error::{OcrError, OcrResult};
pub use geometry::{BBox, Point};
pub use layout::{PageLayout, Region, RegionKind};
pub use pipeline::{OcrEngine, OcrEngineConfig, PageResult, PipelineProgress, PreviewImage};
pub use table::{StructureDict, TableCell, TableStructure};
