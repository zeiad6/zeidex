pub mod normalize;
pub use normalize::normalize_for_search;

pub struct PreparedQuery<'a> {
    raw: &'a str,
    chars: Vec<char>,
}

const MAX_DP_QUERY_LEN: usize = 64;
const MATCH_BASE: i64 = 10;
const WORD_BOUNDARY_BONUS: i64 = 8;
const LEADING_BONUS_MAX_INDEX: usize = 5;
const LEADING_BONUS_STEP: i64 = 2;
const CONSECUTIVE_BONUS_STEP: i64 = 6;
const MAX_GAP_PENALTY: i64 = 20;

/// A title that is exactly the query.
const EXACT_SCORE: i64 = 2_000;
/// A title that starts with the query.
const PREFIX_SCORE: i64 = 1_500;
/// A title (or its extension-less stem) that ends with the query. Above a
/// plain containment match, below a prefix: `report` at the end of
/// `annual-report.pdf` is a stronger signal than in `old-report-archive.pdf`,
/// but a name that starts with the query is what the user is typing.
const SUFFIX_SCORE: i64 = 1_300;
/// One-character suffixes would boost every plural (`notes`, `photos`) for a
/// one-letter query; containment already surfaces those.
const MIN_SUFFIX_QUERY_CHARS: usize = 2;

/// Typo-tolerant (approximate) matching. The base stays below the engine's
/// token-all-match score (850) so a guessed word can never outrank a title that
/// really contains what was typed.
const APPROX_BASE_SCORE: i64 = 600;
const APPROX_EDIT_PENALTY: i64 = 110;
const MAX_APPROX_QUERY_CHARS: usize = 32;
const MAX_APPROX_QUERY_TOKENS: usize = 4;
const MAX_APPROX_TITLE_WORDS: usize = 16;
/// Shortest word that may be matched approximately: one edit in a three-letter
/// word is a different word, not a typo.
const MIN_APPROX_TOKEN_CHARS: usize = 4;
const MAX_APPROX_EDITS: usize = 3;

/// How a query relates to a title, strongest first. The launcher shows this so
/// a user can tell why a row is there: the name starts with what they typed,
/// ends with it, or is only close to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchKind {
    Exact,
    Prefix,
    Suffix,
    Contains,
    Fuzzy,
    Approximate,
}

impl MatchKind {
    /// Stable identifier for the IPC boundary and the UI.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Prefix => "prefix",
            Self::Suffix => "suffix",
            Self::Contains => "contains",
            Self::Fuzzy => "fuzzy",
            Self::Approximate => "approximate",
        }
    }
}

/// Which tier surfaced `title` for `query`, or `None` when the title does not
/// match at all. The query and title must already be search-normalized (see
/// [`normalize_for_search`]); the engine calls this only for the handful of
/// rows it is about to show, not for every candidate.
pub fn classify_match(query: &str, title: &str) -> Option<MatchKind> {
    if query.is_empty() || title.is_empty() {
        return None;
    }
    if title == query {
        return Some(MatchKind::Exact);
    }
    if title.starts_with(query) {
        return Some(MatchKind::Prefix);
    }
    if query.chars().count() >= MIN_SUFFIX_QUERY_CHARS && ends_with_query(title, query) {
        return Some(MatchKind::Suffix);
    }
    if title.contains(query) {
        return Some(MatchKind::Contains);
    }
    // Words typed in any order: "annual report" for `annual_report.pdf`.
    let words: Vec<&str> = query.split_whitespace().collect();
    if words.len() > 1 && words.iter().all(|word| title.contains(word)) {
        return Some(MatchKind::Contains);
    }
    if fuzzy_score(query, title).is_some() {
        return Some(MatchKind::Fuzzy);
    }
    let mut matcher = ApproximateMatcher::new(query)?;
    matcher.score(title).map(|_| MatchKind::Approximate)
}

impl PreparedQuery<'_> {
    fn raw(&self) -> &str {
        self.raw
    }

    pub fn len(&self) -> usize {
        self.chars.len()
    }

    pub fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }
}

#[inline]
pub fn prepare_query(query: &str) -> PreparedQuery<'_> {
    PreparedQuery {
        raw: query,
        chars: query.chars().collect(),
    }
}

pub fn fuzzy_score(query: &str, title: &str) -> Option<i64> {
    let prepared = prepare_query(query);
    fuzzy_score_prepared(&prepared, title)
}

#[inline]
pub fn fuzzy_score_prepared(query: &PreparedQuery<'_>, title: &str) -> Option<i64> {
    let q = query.raw();
    let t = title;

    if t == q {
        return Some(EXACT_SCORE);
    }

    // Kept in bytes to stay O(1): this is a tiebreaker, and the DP below
    // dominates the cost of the call anyway.
    let length_penalty = (t.len() as i64 - q.len() as i64).max(0);

    if t.starts_with(q) {
        return Some(PREFIX_SCORE - length_penalty);
    }

    if query.len() >= MIN_SUFFIX_QUERY_CHARS && ends_with_query(t, q) {
        return Some(SUFFIX_SCORE - length_penalty);
    }

    // Bounded DP gives better alignment quality for normal launcher queries.
    // Very long queries fall back to greedy to keep worst-case CPU predictable.
    if query.len() > MAX_DP_QUERY_LEN {
        return greedy_subsequence_score(query, t);
    }

    fuzzy_score_dp_prepared(query, t)
}

/// True when the name ends with `query`, counting the extension-less stem.
///
/// The stem rule is what makes `2026-014` find `invoice-2026-014.pdf`: the whole
/// name ends with `.pdf`, not with what the user typed. A title without a dot,
/// or a dotfile such as `.gitignore`, has no stem and is compared whole.
///
/// A query that is only the extension does not count. Typing `pdf` is a format
/// filter, and every PDF ending with it would otherwise be labelled "End",
/// which tells the user nothing.
fn ends_with_query(title: &str, query: &str) -> bool {
    if extension(title) == Some(query) {
        return false;
    }
    title.ends_with(query) || title_stem(title).ends_with(query)
}

/// The final extension, without the dot. `None` for a name without one and for
/// a dotfile, where the leading dot is part of the name.
fn extension(title: &str) -> Option<&str> {
    match title.rfind('.') {
        Some(dot) if dot > 0 && dot + 1 < title.len() => Some(&title[dot + 1..]),
        _ => None,
    }
}

/// The file name without its final extension. `.gitignore` keeps its dot: the
/// leading dot is part of the name, not an extension separator.
fn title_stem(title: &str) -> &str {
    match title.rfind('.') {
        Some(dot) if dot > 0 => &title[..dot],
        _ => title,
    }
}

fn greedy_subsequence_score(query: &PreparedQuery<'_>, title: &str) -> Option<i64> {
    let mut qi = 0usize;
    let mut score = 0i64;

    for ch in title.chars() {
        if qi < query.len() && ch == query.chars[qi] {
            qi += 1;
            score += MATCH_BASE;
        }
    }

    if qi == query.len() { Some(score) } else { None }
}

#[derive(Clone, Copy)]
struct MatchState {
    score: i64,
    last_match_idx: usize,
    consecutive: i64,
}

fn fuzzy_score_dp_prepared(query: &PreparedQuery<'_>, title: &str) -> Option<i64> {
    let q_len = query.len();
    if q_len == 0 {
        return Some(0);
    }

    // 1D DP where dp[i] stores the best way to match query[0..=i]
    // up to the current title position.
    let mut dp = [None::<MatchState>; MAX_DP_QUERY_LEN];
    let mut prev_char: Option<char> = None;

    for (ti, ch) in title.chars().enumerate() {
        let is_word_boundary = ti == 0 || matches!(prev_char, Some(' ' | '-' | '_' | '/' | '.'));
        let leading_bonus = if ti < LEADING_BONUS_MAX_INDEX {
            (LEADING_BONUS_MAX_INDEX - ti) as i64 * LEADING_BONUS_STEP
        } else {
            0
        };
        let position_bonus = if is_word_boundary {
            WORD_BOUNDARY_BONUS
        } else {
            0
        } + leading_bonus;

        // Iterate backwards so current character updates do not affect states
        // needed by larger qi in the same title position.
        for qi in (0..q_len).rev() {
            if ch != query.chars[qi] {
                continue;
            }

            let (new_score, new_consecutive) = if qi == 0 {
                (MATCH_BASE + position_bonus, 0)
            } else if let Some(prev) = dp[qi - 1] {
                let gap = (ti - prev.last_match_idx - 1) as i64;
                if gap == 0 {
                    let consecutive = prev.consecutive + 1;
                    (
                        prev.score
                            + MATCH_BASE
                            + position_bonus
                            + (consecutive * CONSECUTIVE_BONUS_STEP),
                        consecutive,
                    )
                } else {
                    (
                        prev.score + MATCH_BASE + position_bonus - gap.min(MAX_GAP_PENALTY),
                        0,
                    )
                }
            } else {
                continue;
            };

            if dp[qi].is_none_or(|existing| new_score > existing.score) {
                dp[qi] = Some(MatchState {
                    score: new_score,
                    last_match_idx: ti,
                    consecutive: new_consecutive,
                });
            }
        }

        prev_char = Some(ch);
    }

    dp[q_len - 1].map(|state| state.score.max(0))
}

#[inline]
pub fn fuzzy_quality_bonus_prepared(query: &PreparedQuery<'_>, title: &str) -> i64 {
    if query.is_empty() || title == query.raw() || title.starts_with(query.raw()) {
        return 0;
    }

    let mut qi = 0usize;
    let mut bonus = 0i64;
    let mut prev_match_idx: Option<usize> = None;
    let mut consecutive: i64 = 0;
    let mut prev_char: Option<char> = None;

    for (ti, ch) in title.chars().enumerate() {
        if qi >= query.len() {
            break;
        }

        if ch != query.chars[qi] {
            prev_char = Some(ch);
            continue;
        }

        qi += 1;

        if let Some(prev) = prev_match_idx {
            if prev + 1 == ti {
                consecutive += 1;
                bonus += consecutive * 8;
            } else {
                consecutive = 0;
                let gap = (ti - prev - 1) as i64;
                bonus -= gap.min(20);
            }
        }

        if ti == 0 || matches!(prev_char, Some(' ' | '-' | '_' | '/' | '.')) {
            bonus += 20;
        }

        if ti < 5 {
            bonus += (5 - ti as i64) * 3;
        }

        prev_match_idx = Some(ti);
        prev_char = Some(ch);
    }

    if qi == query.len() { bonus.max(0) } else { 0 }
}

/// A reusable typo-tolerant matcher for one query.
///
/// Built once per search pass: it owns the query tokens and the DP scratch
/// buffers, so scoring a candidate allocates nothing. `new` returns `None` when
/// the query cannot be matched approximately at all (too short, too long, too
/// many words), which lets the caller skip the per-candidate work entirely.
pub struct ApproximateMatcher {
    tokens: Vec<Vec<char>>,
    budgets: Vec<usize>,
    best: Vec<usize>,
    word: Vec<char>,
    rows: [Vec<usize>; 3],
    min_title_len: usize,
}

impl ApproximateMatcher {
    pub fn new(query: &str) -> Option<Self> {
        let mut tokens: Vec<Vec<char>> = Vec::new();
        let mut budgets: Vec<usize> = Vec::new();
        let mut min_title_len = 0usize;

        // Split on separators as well as spaces: a typo can sit in one segment
        // of `invoise-2026`, and comparing the whole hyphenated string against
        // a single title word would never fit.
        for token in query.split(|c: char| !c.is_alphanumeric()) {
            if token.is_empty() {
                continue;
            }
            if tokens.len() == MAX_APPROX_QUERY_TOKENS {
                return None;
            }
            let chars: Vec<char> = token.chars().collect();
            // A three-letter word with one edit is a different word, and a
            // query that short is better served by the exact tiers.
            if chars.len() < MIN_APPROX_TOKEN_CHARS {
                return None;
            }
            min_title_len += chars.len();
            // One edit per four characters, so longer words tolerate more
            // typos without letting a short one drift into another word.
            budgets.push((chars.len() / 4).clamp(1, MAX_APPROX_EDITS));
            tokens.push(chars);
        }

        if tokens.is_empty() || min_title_len > MAX_APPROX_QUERY_CHARS {
            return None;
        }

        Some(Self {
            tokens,
            budgets,
            best: Vec::new(),
            word: Vec::new(),
            rows: [Vec::new(), Vec::new(), Vec::new()],
            min_title_len,
        })
    }

    /// The approximate score for `title`, or `None` when the title is not
    /// within the query's typo budget. `title` must be search-normalized.
    pub fn score(&mut self, title: &str) -> Option<i64> {
        // A title shorter than the query in bytes cannot hold it even with
        // insertions (bytes >= chars, so this never rejects a real match).
        if title.is_empty() || title.len() < self.min_title_len {
            return None;
        }

        // Cheap rejection: the first letter of the query has to exist
        // somewhere in the title. Most unrelated titles fail here, before any
        // edit-distance work runs.
        let first = self.tokens[0][0];
        if !title.contains(first) {
            return None;
        }

        self.best.clear();
        self.best.resize(self.tokens.len(), usize::MAX);

        let mut words = 0usize;
        for word in title.split(|c: char| !c.is_alphanumeric()) {
            if word.is_empty() {
                continue;
            }
            words += 1;
            if words > MAX_APPROX_TITLE_WORDS {
                break;
            }
            self.word.clear();
            self.word.extend(word.chars());

            for (index, token) in self.tokens.iter().enumerate() {
                if self.best[index] == 0 {
                    continue;
                }
                let budget = self.budgets[index];
                let mut distance = banded_osa(token, &self.word, budget, &mut self.rows);
                // A typo in the prefix of a concatenated name
                // (`reportfinal` for `reprot`) never fits the whole word, so
                // the word is also compared up to the query's length.
                if distance.is_none() && self.word.len() > token.len() {
                    distance = banded_osa(token, &self.word[..token.len()], budget, &mut self.rows);
                }
                if let Some(distance) = distance {
                    self.best[index] = self.best[index].min(distance);
                }
            }
        }

        let mut total = 0usize;
        for &distance in &self.best {
            if distance == usize::MAX {
                return None;
            }
            total += distance;
        }

        // The score itself enforces the global tolerance: past five edits the
        // result is no longer "close", and the row is dropped instead of
        // being shown with a meaningless score.
        let score = APPROX_BASE_SCORE - (total as i64 * APPROX_EDIT_PENALTY);
        (score > 0).then_some(score)
    }
}

/// Optimal-string-alignment distance between `a` and `b`, computed only along
/// the band `|i - j| <= max`. Returns `None` as soon as the distance is known
/// to exceed `max`, so a hopeless pair costs a few rows instead of a full DP.
///
/// OSA (Levenshtein plus adjacent transposition) is what makes `reprot` one
/// edit from `report` instead of two.
fn banded_osa(a: &[char], b: &[char], max: usize, rows: &mut [Vec<usize>; 3]) -> Option<usize> {
    const INF: usize = usize::MAX / 4;
    let n = a.len();
    let m = b.len();

    if n.abs_diff(m) > max {
        return None;
    }
    if n == 0 {
        return (m <= max).then_some(m);
    }
    if m == 0 {
        return (n <= max).then_some(n);
    }

    for row in rows.iter_mut() {
        row.clear();
        row.resize(m + 1, INF);
    }
    rows[0][0] = 0;
    // Column index is the value: one insertion per character.
    for (j, cell) in rows[0].iter_mut().enumerate().take(m.min(max) + 1).skip(1) {
        *cell = j;
    }

    for i in 1..=n {
        let current = i % 3;
        let previous = (i + 2) % 3;
        let two_back = (i + 1) % 3;
        for cell in rows[current].iter_mut() {
            *cell = INF;
        }

        let low = i.saturating_sub(max).max(1);
        let high = (i + max).min(m);
        if i <= max {
            rows[current][0] = i;
        }

        for j in low..=high {
            let substitution = usize::from(a[i - 1] != b[j - 1]);
            let mut value = (rows[previous][j] + 1)
                .min(rows[current][j - 1] + 1)
                .min(rows[previous][j - 1] + substitution);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                value = value.min(rows[two_back][j - 2] + 1);
            }
            rows[current][j] = value;
        }
    }

    let distance = rows[n % 3][m];
    (distance <= max).then_some(distance)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_match_returns_max_score() {
        assert_eq!(fuzzy_score("safari", "safari"), Some(2_000));
    }

    #[test]
    fn prefix_match_returns_high_score() {
        let score = fuzzy_score("saf", "safari").unwrap();
        assert!(score > 1_000);
        assert!(score < 2_000);
    }

    #[test]
    fn prefix_score_penalizes_longer_titles() {
        let short = fuzzy_score("saf", "safari").unwrap();
        let long = fuzzy_score("saf", "safari browser extended").unwrap();
        assert!(short > long);
    }

    #[test]
    fn subsequence_match_returns_positive_score() {
        let score = fuzzy_score("vsc", "visual studio code").unwrap();
        assert!(score > 0);
    }

    #[test]
    fn no_match_returns_none() {
        assert_eq!(fuzzy_score("xyz", "safari"), None);
    }

    #[test]
    fn empty_query_matches_everything() {
        let score = fuzzy_score("", "safari").unwrap();
        assert!(score > 0);
    }

    #[test]
    fn empty_title_only_matches_empty_query() {
        assert_eq!(fuzzy_score("", ""), Some(2_000));
        assert_eq!(fuzzy_score("a", ""), None);
    }

    #[test]
    fn single_char_query() {
        assert!(fuzzy_score("s", "safari").is_some());
        assert!(fuzzy_score("z", "safari").is_none());
    }

    #[test]
    fn score_hierarchy_is_consistent() {
        let exact = fuzzy_score("safari", "safari").unwrap();
        let prefix = fuzzy_score("saf", "safari").unwrap();
        let subseq = fuzzy_score("sri", "safari").unwrap();

        assert!(exact > prefix, "exact ({exact}) > prefix ({prefix})");
        assert!(
            prefix > subseq,
            "prefix ({prefix}) > subsequence ({subseq})"
        );
    }

    #[test]
    fn prepared_query_matches_raw_behavior() {
        let prepared = prepare_query("vsc");
        assert_eq!(
            fuzzy_score_prepared(&prepared, "visual studio code"),
            fuzzy_score("vsc", "visual studio code")
        );
    }

    #[test]
    fn quality_bonus_prefers_compact_word_boundaries() {
        let prepared = prepare_query("vsc");
        let tight = fuzzy_quality_bonus_prepared(&prepared, "vs code");
        let loose = fuzzy_quality_bonus_prepared(&prepared, "visual studio code");
        assert!(tight > loose);
    }

    #[test]
    fn dp_base_score_prefers_dense_clusters_over_sparse_greedy_paths() {
        let prepared = prepare_query("app");
        let sparse = fuzzy_score_prepared(&prepared, "a_partial_app.rs").unwrap();
        let dense = fuzzy_score_prepared(&prepared, "my_app.rs").unwrap();
        assert!(
            dense > sparse,
            "dense ({dense}) should be > sparse ({sparse})"
        );
    }

    #[test]
    fn suffix_match_scores_above_containment_and_below_prefix() {
        let suffix = fuzzy_score("report", "annual-report").unwrap();
        let prefix = fuzzy_score("report", "report-2026").unwrap();
        let contained = fuzzy_score("report", "annual-report-2025.pdf").unwrap();

        assert!(suffix > 1_000, "suffix scores high: {suffix}");
        assert!(prefix > suffix, "prefix ({prefix}) > suffix ({suffix})");
        assert!(
            suffix > contained,
            "suffix ({suffix}) > contained ({contained})"
        );
    }

    #[test]
    fn suffix_match_ignores_the_extension() {
        // The whole name ends with `.pdf`; what the user typed is at the end of
        // the stem, and that has to count.
        assert!(fuzzy_score("2026-014", "invoice-2026-014.pdf").unwrap() > 1_000);
        assert!(fuzzy_score("final", "backup.final.pdf").unwrap() > 1_000);
    }

    #[test]
    fn a_query_that_is_only_the_extension_is_not_an_ending() {
        // `pdf` is a format filter: every PDF would otherwise say "End".
        let score = fuzzy_score("pdf", "invoice-2026-014.pdf").unwrap();
        assert!(score < SUFFIX_SCORE, "got {score}");
        assert_eq!(
            classify_match("pdf", "invoice-2026-014.pdf"),
            Some(MatchKind::Contains)
        );
        // A longer query that happens to include the extension still ends the
        // name and keeps the suffix tier.
        assert_eq!(
            classify_match("2026-014.pdf", "invoice-2026-014.pdf"),
            Some(MatchKind::Suffix)
        );
    }

    #[test]
    fn a_single_letter_query_does_not_earn_a_suffix_bonus() {
        // Every plural would otherwise outrank a real prefix match.
        let score = fuzzy_score("s", "notes").unwrap();
        assert!(score < SUFFIX_SCORE, "got {score}");
    }

    #[test]
    fn a_dotfile_is_compared_whole() {
        assert_eq!(title_stem(".gitignore"), ".gitignore");
        assert_eq!(title_stem("report.pdf"), "report");
        assert_eq!(title_stem("no-extension"), "no-extension");
    }

    #[test]
    fn transposed_letters_are_one_edit_not_two() {
        let mut matcher = ApproximateMatcher::new("reprot").expect("six letters can approximate");
        let score = matcher
            .score("report.pdf")
            .expect("transposition is a typo");
        assert!(score > 0);
        assert!(score < 600, "a typo costs something: {score}");
    }

    #[test]
    fn a_substituted_letter_is_a_typo() {
        let mut matcher = ApproximateMatcher::new("invoise").expect("seven letters");
        assert!(matcher.score("invoice-2026-014.pdf").is_some());
    }

    #[test]
    fn a_typo_in_a_concatenated_name_still_matches() {
        let mut matcher = ApproximateMatcher::new("reprot").expect("six letters");
        assert!(matcher.score("reportfinal.pdf").is_some());
    }

    #[test]
    fn an_unrelated_word_is_not_approximate() {
        let mut matcher = ApproximateMatcher::new("invoice").expect("seven letters");
        assert!(matcher.score("holiday-photo.png").is_none());
    }

    #[test]
    fn short_queries_are_never_approximate() {
        assert!(ApproximateMatcher::new("pdf").is_none());
        assert!(ApproximateMatcher::new("ab").is_none());
    }

    #[test]
    fn every_word_of_a_multi_word_query_has_to_be_close() {
        let mut matcher = ApproximateMatcher::new("visul code").expect("two words");
        assert!(matcher.score("visual studio code").is_some());
        assert!(matcher.score("visual studio notes").is_none());
    }

    #[test]
    fn approximate_matching_handles_arabic_typos() {
        // `مدرسه` vs `مدرسة` is already folded by normalization; this is a real
        // typo: a missing letter inside the word.
        let mut matcher = ApproximateMatcher::new("تقري").expect("four letters");
        assert!(matcher.score("تقرير-2026.pdf").is_some());
    }

    #[test]
    fn classification_names_the_tier_that_matched() {
        assert_eq!(classify_match("safari", "safari"), Some(MatchKind::Exact));
        assert_eq!(classify_match("saf", "safari"), Some(MatchKind::Prefix));
        assert_eq!(
            classify_match("2026-014", "invoice-2026-014.pdf"),
            Some(MatchKind::Suffix)
        );
        assert_eq!(
            classify_match("annual", "annual-report.pdf"),
            Some(MatchKind::Prefix)
        );
        assert_eq!(
            classify_match("report", "annual-report.pdf"),
            Some(MatchKind::Suffix)
        );
        assert_eq!(
            classify_match("annual report", "annual-report.pdf"),
            Some(MatchKind::Contains)
        );
        assert_eq!(
            classify_match("vsc", "visual studio code"),
            Some(MatchKind::Fuzzy)
        );
        assert_eq!(
            classify_match("invoise", "invoice.pdf"),
            Some(MatchKind::Approximate)
        );
        assert_eq!(classify_match("xyz", "invoice.pdf"), None);
    }

    #[test]
    fn banded_distance_agrees_with_the_full_table_on_short_words() {
        // A tiny full-table OSA, so the banded version is checked against a
        // reference instead of against itself.
        fn full_osa(a: &[char], b: &[char]) -> usize {
            let n = a.len();
            let m = b.len();
            let mut dp = vec![vec![0usize; m + 1]; n + 1];
            for (i, row) in dp.iter_mut().enumerate() {
                row[0] = i;
            }
            for j in 0..=m {
                dp[0][j] = j;
            }
            for i in 1..=n {
                for j in 1..=m {
                    let cost = usize::from(a[i - 1] != b[j - 1]);
                    let mut value = (dp[i - 1][j] + 1)
                        .min(dp[i][j - 1] + 1)
                        .min(dp[i - 1][j - 1] + cost);
                    if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                        value = value.min(dp[i - 2][j - 2] + 1);
                    }
                    dp[i][j] = value;
                }
            }
            dp[n][m]
        }

        let pairs = [
            ("report", "reprot"),
            ("invoice", "invoise"),
            ("kitten", "sitting"),
            ("report", "reportfinal"),
            ("abc", "abc"),
            ("", "abc"),
            ("abcd", "dcba"),
            ("مدرسة", "مدرسه"),
        ];
        let mut rows = [Vec::new(), Vec::new(), Vec::new()];
        for (a, b) in pairs {
            let a: Vec<char> = a.chars().collect();
            let b: Vec<char> = b.chars().collect();
            let expected = full_osa(&a, &b);
            for max in 0..=4 {
                let banded = banded_osa(&a, &b, max, &mut rows);
                let want = (expected <= max).then_some(expected);
                assert_eq!(banded, want, "{a:?} vs {b:?} with max {max}");
            }
        }
    }
}
