use zeidex_indexing::Candidate;

const USAGE_LOG_SCALE: f64 = 5.0;
/// The title is the query, after normalization.
const EXACT_BONUS: i64 = 500;
/// The name starts with what the user typed: they are still typing it.
const PREFIX_BONUS: i64 = 200;
/// The name ends with the query. Weaker than a prefix - the significant word
/// may be anywhere - but stronger than a bare containment, so `code` ranks
/// `Visual Studio Code` above `code-archive-2019`.
const SUFFIX_BONUS: i64 = 120;
/// Below the prefix bonus, so a shorter name that starts with the query keeps
/// winning over a longer one that ends with it.
const MIN_SUFFIX_QUERY_CHARS: usize = 2;

pub fn rank_score(base_score: i64, query: &str, candidate: &Candidate, title_lower: &str) -> i64 {
    let mut score = base_score;

    // `title_lower` is the search-normalized title, so this comparison also
    // catches `Café` typed as `cafe` and the Arabic letter variants that
    // normalization folds together. `eq_ignore_ascii_case` against the raw
    // title missed both.
    if title_lower == query {
        score += EXACT_BONUS;
    } else if title_lower.starts_with(query) {
        score += PREFIX_BONUS;
    } else if query.chars().count() >= MIN_SUFFIX_QUERY_CHARS && ends_with_query(title_lower, query)
    {
        score += SUFFIX_BONUS;
    }

    // Log scaling keeps frequent items helpful without permanently dominating
    // weak textual matches.
    if candidate.use_count > 0 {
        score += ((candidate.use_count as f64).log2() * USAGE_LOG_SCALE) as i64;
    }

    if candidate.last_used_at_unix_s.is_some() {
        score += 25;
    }

    score
}

/// Mirrors the matching crate's rule: the name ends with the query, counting
/// the extension-less stem, but a query that is only the extension is a format
/// filter rather than an ending.
fn ends_with_query(title: &str, query: &str) -> bool {
    let dot = title.rfind('.').filter(|dot| *dot > 0);
    if let Some(dot) = dot
        && dot + 1 < title.len()
        && &title[dot + 1..] == query
    {
        return false;
    }
    title.ends_with(query) || dot.is_some_and(|dot| title[..dot].ends_with(query))
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeidex_indexing::CandidateKind;

    fn test_candidate(title: &str, use_count: u64, last_used: Option<i64>) -> Candidate {
        Candidate {
            id: "test".into(),
            kind: CandidateKind::App,
            title: title.into(),
            subtitle: None,
            path: "/test".into(),
            use_count,
            last_used_at_unix_s: last_used,
            ..Default::default()
        }
    }

    #[test]
    fn exact_match_gets_bonus() {
        let c = test_candidate("Safari", 0, None);
        // The query reaching `rank_score` is search-normalized (lowercase).
        let score = rank_score(100, "safari", &c, "safari");
        assert_eq!(score, 100 + EXACT_BONUS);
    }

    #[test]
    fn exact_match_is_detected_after_folding() {
        // `Café` and `cafe` are the same query once normalized; the raw-title
        // comparison this replaced missed it.
        let c = test_candidate("Café", 0, None);
        let score = rank_score(100, "cafe", &c, "cafe");
        assert_eq!(score, 100 + EXACT_BONUS);
    }

    #[test]
    fn prefix_match_gets_bonus() {
        let c = test_candidate("Safari Browser", 0, None);
        let score = rank_score(100, "safari", &c, "safari browser");
        assert_eq!(score, 100 + PREFIX_BONUS);
    }

    #[test]
    fn suffix_match_gets_a_smaller_bonus_than_a_prefix() {
        let prefix = test_candidate("report-2026.pdf", 0, None);
        let suffix = test_candidate("annual-report.pdf", 0, None);
        let prefix_score = rank_score(100, "report", &prefix, "report-2026.pdf");
        let suffix_score = rank_score(100, "report", &suffix, "annual-report.pdf");
        assert_eq!(prefix_score, 100 + PREFIX_BONUS);
        assert_eq!(suffix_score, 100 + SUFFIX_BONUS);
        assert!(prefix_score > suffix_score);
    }

    #[test]
    fn suffix_match_ignores_the_extension() {
        let c = test_candidate("invoice-2026-014.pdf", 0, None);
        let score = rank_score(100, "2026-014", &c, "invoice-2026-014.pdf");
        assert_eq!(score, 100 + SUFFIX_BONUS);
    }

    #[test]
    fn a_query_that_is_only_the_extension_earns_no_suffix_bonus() {
        let c = test_candidate("invoice-2026-014.pdf", 0, None);
        let score = rank_score(100, "pdf", &c, "invoice-2026-014.pdf");
        assert_eq!(score, 100);
    }

    #[test]
    fn a_single_letter_query_earns_no_suffix_bonus() {
        let c = test_candidate("notes.txt", 0, None);
        let score = rank_score(100, "s", &c, "notes.txt");
        assert_eq!(score, 100);
    }

    #[test]
    fn usage_count_boosts_score() {
        let no_usage = test_candidate("App", 0, None);
        let medium_usage = test_candidate("App", 10, None);
        let high_usage = test_candidate("App", 100, None);
        let s1 = rank_score(100, "app", &no_usage, "app");
        let s2 = rank_score(100, "app", &medium_usage, "app");
        let s3 = rank_score(100, "app", &high_usage, "app");
        assert!(s2 > s1);
        assert!(s3 > s2);
        assert!(s2 - s1 <= 20, "10 uses should stay bounded: {}", s2 - s1);
        assert!(s3 - s2 <= 20, "100 uses should flatten growth: {}", s3 - s2);
    }

    #[test]
    fn recent_usage_gives_bonus() {
        let never_used = test_candidate("App", 0, None);
        let recently_used = test_candidate("App", 0, Some(1_000_000));
        let s1 = rank_score(100, "x", &never_used, "app");
        let s2 = rank_score(100, "x", &recently_used, "app");
        assert!(s2 - s1 >= 25);
    }
}
