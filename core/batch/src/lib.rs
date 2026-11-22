//! Batch search: the list a user pastes, the matching, and the report.

#![deny(clippy::expect_used, clippy::unwrap_used)]

pub mod error;
pub mod matcher;
pub mod parse;
pub mod report;

pub use error::{BatchError, BatchResult};
pub use matcher::{
    BatchEntryResult, BatchFile, BatchMatch, BatchReport, BatchSummary, DEFAULT_MATCH_LIMIT,
    DEFAULT_THRESHOLD, FileKind, MAX_THRESHOLD, MIN_THRESHOLD, MatchKind, search,
    search_with_limit,
};
pub use parse::{MAX_ENTRIES, ParsedList, decode_text, parse_list};
pub use report::{ReportMeta, export_report};
