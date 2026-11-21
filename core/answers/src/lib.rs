//! Shared URL + translation helpers. AI web answers removed.

#[cfg(feature = "translate")]
pub mod http;
#[cfg(feature = "translate")]
mod translate;
mod url;

#[cfg(feature = "translate")]
pub use translate::{TranslateError, Translation, translate};
pub use url::{UrlMatch, UrlTier, classify_url};
