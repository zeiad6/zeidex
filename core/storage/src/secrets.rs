//! High-confidence credential detection for clipboard capture.
//!
//! Clipboard history is a searchable, persistent copy of everything the user
//! copies. Credentials are the one class of text where keeping that copy is
//! actively harmful, so a clip that matches a known key format is not written.
//!
//! The bar is deliberately high: every pattern here is a prefix that ordinary
//! prose, Arabic text, and code identifiers do not produce. A generic
//! "password=" or a bare base64 blob is *not* matched - those occur in normal
//! text often enough that dropping them would silently lose clips the user
//! expects to find.

/// `(prefix, minimum token length, exact token length)`. The minimum is the
/// shortest real key of that family and is what keeps `sk-` from matching a CSS
/// class name. `Some(exact)` additionally requires the total token to be that
/// long, for families whose length is fixed: AWS key ids are exactly 20
/// characters, so requiring that rejects an uppercase word such as
/// `ASIAPACIFIC2026CONFERENCE` that merely starts with the prefix.
const KEY_PREFIXES: &[(&str, usize, Option<usize>)] = &[
    ("AKIA", 20, Some(20)), // AWS access key id: AKIA + 16 uppercase alphanumerics
    ("ASIA", 20, Some(20)), // AWS temporary access key id
    ("ghp_", 40, None),     // GitHub personal access tokens
    ("gho_", 40, None),     // GitHub OAuth tokens
    ("ghu_", 40, None),     // GitHub user-to-server tokens
    ("ghs_", 40, None),     // GitHub server-to-server tokens
    ("ghr_", 40, None),     // GitHub refresh tokens
    ("github_pat_", 40, None), // GitHub fine-grained PAT
    ("xoxb-", 20, None),    // Slack bot token
    ("xoxp-", 20, None),    // Slack user token
    ("xoxa-", 20, None),    // Slack app-level token
    ("xoxr-", 20, None),    // Slack refresh token
    ("sk-", 40, None),      // OpenAI / Anthropic API keys
];

/// True when `text` contains something that is a credential with very high
/// probability. Callers must treat a `true` as "do not persist this".
pub(crate) fn looks_like_secret(text: &str) -> bool {
    // PEM blocks: RSA/EC/OPENSSH/PGP private keys all carry this marker.
    if text.contains("-----BEGIN ") && text.contains("PRIVATE KEY-----") {
        return true;
    }
    looks_like_jwt(text)
        || KEY_PREFIXES
            .iter()
            .any(|&(prefix, min, exact)| has_key_token(text, prefix, min, exact))
}

/// The longest run that can still be a credential. A window is what keeps the
/// scan linear in the size of the clip: without one, a clip made of repeated
/// `eyJ` rescans the same tail for every occurrence and a 256 KiB paste costs
/// 15 s of CPU (measured). No real key or JWT comes near this length, and a run
/// with no delimiter inside the window is not a credential either.
const MAX_TOKEN_BYTES: usize = 8 * 1024;

/// The token starting at the front of `candidate`: the longest run of key
/// characters, cut to the scan window. `dots` allows the `.` that separates JWT
/// segments, which plain keys never contain.
fn leading_token(candidate: &str, dots: bool) -> Option<&str> {
    let mut end = candidate.len().min(MAX_TOKEN_BYTES);
    // Index 0 is always a char boundary, so this cannot underflow.
    while !candidate.is_char_boundary(end) {
        end -= 1;
    }
    let window = &candidate[..end];
    match window
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-' || (dots && c == '.')))
    {
        Some(end) => Some(&window[..end]),
        // No delimiter inside the window. When the window is the whole text the
        // run simply ends with it, which is a token; otherwise the run goes on
        // past the window, and no credential is that long.
        None if candidate.len() <= MAX_TOKEN_BYTES => Some(window),
        None => None,
    }
}

/// A JWT is three base64url segments, and the header and payload both begin
/// with `eyJ` because each is a JSON object. Requiring that pair of prefixes is
/// what separates a token from any other dotted string.
fn looks_like_jwt(text: &str) -> bool {
    let mut rest = text;
    while let Some(pos) = rest.find("eyJ") {
        if let Some(token) = leading_token(&rest[pos..], true)
            && token.len() >= 40
        {
            let mut segments = token.split('.');
            let header = segments.next().unwrap_or_default();
            let payload = segments.next().unwrap_or_default();
            let signature = segments.next().unwrap_or_default();
            // `unwrap_or_default` cannot hide a missing segment here: a token
            // with fewer than three dots has an empty signature or payload, and
            // the `starts_with` checks reject it.
            if header.starts_with("eyJ")
                && payload.starts_with("eyJ")
                && !signature.is_empty()
                && segments.next().is_none()
            {
                return true;
            }
        }
        rest = &rest[pos + 3..];
    }
    false
}

/// Scans for `prefix` followed by a key-shaped token. The digit requirement is
/// a second guard against prose: real generated keys essentially always contain
/// one, while words that happen to start with the prefix do not.
fn has_key_token(text: &str, prefix: &str, min_len: usize, exact_len: Option<usize>) -> bool {
    let mut rest = text;
    while let Some(pos) = rest.find(prefix) {
        if let Some(token) = leading_token(&rest[pos..], false)
            && token.len() >= min_len
            && token.bytes().any(|b| b.is_ascii_digit())
            && exact_len.is_none_or(|exact| token.len() == exact)
        {
            return true;
        }
        rest = &rest[pos + prefix.len()..];
    }
    false
}

#[cfg(test)]
mod tests {
    use super::looks_like_secret;

    #[test]
    fn detects_private_key_blocks() {
        let pem = "-----BEGIN RSA PRIVATE KEY-----\nMIIEow...\n-----END RSA PRIVATE KEY-----";
        assert!(looks_like_secret(pem));
        assert!(looks_like_secret(
            "-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXk=\n"
        ));
    }

    #[test]
    fn detects_cloud_and_forge_tokens() {
        // The AWS documentation's own example key id.
        assert!(looks_like_secret("AKIAIOSFODNN7EXAMPLE"));
        assert!(looks_like_secret(concat!(
            "token: ghp_",
            "16C7e42F292c6912E7710c838347Ae178B4a"
        )));
        assert!(looks_like_secret(concat!(
            "github_pat_",
            "11ABCDEFG0abcdefghijklmnopqrstuvwxyz0123456789"
        )));
        assert!(looks_like_secret(concat!(
            "xoxb-",
            "123456789012-abcdefghijklmnop"
        )));
        assert!(looks_like_secret(concat!(
            "sk-proj-",
            "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGH"
        )));
    }

    #[test]
    fn detects_jwts() {
        let jwt = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U";
        assert!(looks_like_secret(jwt));
        assert!(looks_like_secret(&format!("Authorization: Bearer {jwt}")));
    }

    #[test]
    fn a_run_longer_than_the_scan_window_is_not_a_credential() {
        // The window is what bounds the scan: a run that continues past it is a
        // document, not a key. The digit makes the old unbounded scan match, so
        // this is also the tripwire for the window itself. The 256 KiB case that
        // motivated it (15 s of CPU in release, far worse unoptimised) is left
        // out of the suite on purpose: the bound is the same one either way.
        assert!(!looks_like_secret(&format!("sk-{}1", "a".repeat(9_000))));
        assert!(!looks_like_secret(&format!("AKIA{}1", "B".repeat(9_000))));
        assert!(!looks_like_secret(&format!("eyJ{}", "a".repeat(9_000))));
    }

    #[test]
    fn a_key_inside_a_large_document_is_still_found() {
        let filler = "هذا مستند طويل بلا أي مفتاح. ".repeat(2_000);
        let document = format!("{filler}\nAKIAIOSFODNN7EXAMPLE\n{filler}");
        assert!(document.len() > 100 * 1024);
        assert!(looks_like_secret(&document));
    }

    #[test]
    fn empty_input_does_not_match() {
        assert!(!looks_like_secret(""));
    }

    #[test]
    fn keeps_ordinary_text_and_code() {
        // Arabic prose, including Arabic-Indic digits and a date.
        assert!(!looks_like_secret("فاتورة ٢٠٢٦ بمبلغ 1250 ريالاً"));
        assert!(!looks_like_secret("مدرسة الأطفال في شارع الملك فهد"));
        // The prefix alone, or a word that merely starts with it.
        assert!(!looks_like_secret("AKIA is the prefix AWS uses"));
        // Prefix plus a long word with digits in it: a real AWS key id is exactly
        // 20 characters, so an all-caps event name must not be dropped.
        assert!(!looks_like_secret("ASIAPACIFIC2026CONFERENCE"));
        assert!(!looks_like_secret("AKIA2026CONFERENCEPROGRAMME"));
        assert!(!looks_like_secret("sk-button sk-input sk-label"));
        // Code, a path, and a plain base64 blob are not credentials we can
        // identify without false positives.
        assert!(!looks_like_secret(
            "let token = compute_token();\nfn main() {}"
        ));
        assert!(!looks_like_secret("/home/user/sk-notes/report.txt"));
        assert!(!looks_like_secret(
            "SGVsbG8gd29ybGQgdGhpcyBpcyBub3QgYSBrZXk="
        ));
        // A JWT-shaped string with only two segments is not a JWT.
        assert!(!looks_like_secret("eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0"));
        // Empty input must not panic or match.
        assert!(!looks_like_secret(""));
    }
}
