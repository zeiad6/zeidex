use serde::Serialize;
use zeidex_answers::{TranslateError, Translation};

/// The wire shape of a translation. The core type is deliberately shell-neutral
/// (it exposes `code()`/`message()` rather than a serialized enum), so the
/// user-facing message is formatted here, once, for every platform.
#[derive(Debug, Serialize)]
pub struct TranslationOutcome {
    pub original: String,
    pub translated: String,
    pub error: Option<TranslationFailure>,
}

#[derive(Debug, Serialize)]
pub struct TranslationFailure {
    pub code: String,
    pub message: String,
}

impl From<Translation> for TranslationOutcome {
    fn from(value: Translation) -> Self {
        Self {
            original: value.original,
            translated: value.translated,
            error: value.error.map(|error: TranslateError| TranslationFailure {
                code: error.code().to_string(),
                message: error.message().to_string(),
            }),
        }
    }
}

/// `tr"` — translates `text` into `target_lang`, auto-detecting the source.
/// The core helper is a blocking `curl` call, so it runs on Tauri's blocking
/// pool and never stalls the UI thread.
#[tauri::command]
pub async fn translate_text(
    text: String,
    target_lang: String,
) -> Result<TranslationOutcome, String> {
    tauri::async_runtime::spawn_blocking(move || {
        TranslationOutcome::from(zeidex_answers::translate(&text, &target_lang))
    })
    .await
    .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_successful_translation_has_no_error() {
        let outcome = TranslationOutcome::from(Translation {
            original: "hello".into(),
            translated: "مرحبا".into(),
            error: None,
        });
        assert_eq!(outcome.original, "hello");
        assert_eq!(outcome.translated, "مرحبا");
        assert!(outcome.error.is_none());
    }

    #[test]
    fn a_failure_carries_the_stable_code_and_the_user_message() {
        let outcome = TranslationOutcome::from(Translation {
            original: "hello".into(),
            translated: String::new(),
            error: Some(TranslateError::RateLimited),
        });
        let error = outcome.error.expect("the failure is carried");
        assert_eq!(error.code, "translate_rate_limited");
        assert_eq!(
            error.message,
            "Translation is rate limited, try again shortly"
        );
    }
}
