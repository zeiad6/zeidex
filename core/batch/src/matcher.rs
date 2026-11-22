//! Matching a pasted list of names against the files Zeidex has indexed.
//!
//! The request is normalized once (`normalize_for_search`: case, diacritics,
//! Arabic letter variants, Arabic-Indic digits) and then scored against every
//! candidate name and its stem, which is the name without its last extension.
//! Four measurements are taken and the best one wins:
//!
//! | Measure | Score | What it means |
//! | --- | --- | --- |
//! | exact / stem | 100 | the whole normalized name (or name without extension) equals the request |
//! | containment | 60–99 | one text is a contiguous part of the other, at least 3 characters; +20 when the shorter one is a prefix |
//! | tokens | 85 | every request token appears among the candidate's tokens |
//! | fuzzy | 0–99 | Levenshtein similarity |
//!
//! Every score is an integer percentage and the slider's threshold is applied
//! to that same number, so the number shown next to a result is the number that
//! decided it. At 100% only the first row can pass: containment tops out at 99
//! and fuzzy at 99, so "100%" means equality after normalization, nothing else.
//!
//! The fuzzy pass is the only expensive one, and it is bounded twice. First by
//! length: an edit distance of `d` changes the length by at most `d`, so at a
//! 70% threshold a 10-character request cannot match a 20-character name.
//! Second by [`MAX_EDITS`]: a name that differs by more than four characters is
//! a different name, not a typo, and a percentage alone would let a long name
//! drift far further. Within that budget the distance is computed on a diagonal
//! band and abandoned as soon as a whole row leaves it, so a hopeless candidate
//! costs a few cells per character rather than a full matrix.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use zeidex_matching::normalize_for_search;

/// The slider's documented range. Below 50% almost every file in an index is a
/// "possible match", which is not a result, so the value is clamped rather than
/// silently accepted.
pub const MIN_THRESHOLD: u8 = 50;
pub const MAX_THRESHOLD: u8 = 100;
pub const DEFAULT_THRESHOLD: u8 = 70;
/// Candidates returned per requested name. The total is still reported.
pub const DEFAULT_MATCH_LIMIT: usize = 8;

/// Score given when every request token appears in the candidate's tokens.
const TOKEN_SCORE: u8 = 85;
/// Containment starts here and reaches 99 as the lengths approach equality.
const CONTAINMENT_BASE: u32 = 60;
/// A prefix is a stronger signal than a part found in the middle: a pasted name
/// is usually the start of the real one (`invoice` for `invoice-2024-final.pdf`).
const PREFIX_BONUS: u32 = 20;
/// Shortest text a containment match is allowed for. Without this, a one-letter
/// request would match every name containing that letter.
const MIN_CONTAINMENT_CHARS: usize = 3;
/// Most character edits a fuzzy match may have, whatever the percentage allows.
/// Two or three typos in a name are plausible; nine are a different name.
pub const MAX_EDITS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    File,
    Folder,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchKind {
    /// The whole normalized name equals the request.
    Exact,
    /// The name without its extension equals the request (`invoice` -> `invoice.pdf`).
    Stem,
    /// One text is a contiguous part of the other.
    Containment,
    /// Every request token appears among the name's tokens.
    Tokens,
    /// Levenshtein similarity.
    Fuzzy,
}

/// What a request's best candidate amounts to.
///
/// A typed enum rather than a string: the report colours and labels rows by
/// this value, and a mistyped `"posible"` used to fall through to the muted
/// default instead of failing to compile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchStatus {
    /// The whole normalized name, or the name without its extension, equals the
    /// request.
    Exact,
    /// A strong but inexact match: 85% or more.
    High,
    /// At or above the threshold, below 85%.
    Possible,
    /// The search ran and nothing reached the threshold.
    NotFound,
    /// The request had no searchable characters, so no score exists for it.
    Error,
}

impl MatchKind {
    pub fn label(self) -> &'static str {
        match self {
            MatchKind::Exact => "Exact",
            MatchKind::Stem => "Exact (name)",
            MatchKind::Containment => "Contains",
            MatchKind::Tokens => "All words",
            MatchKind::Fuzzy => "Fuzzy",
        }
    }
}

/// One indexed file, prepared once for every entry in a batch.
#[derive(Clone, Debug)]
pub struct BatchFile {
    pub path: String,
    pub name: String,
    pub stem: String,
    pub kind: FileKind,
    pub modified_unix_s: Option<i64>,
    normalized_name: String,
    normalized_stem: String,
    name_len: usize,
    stem_len: usize,
}

impl BatchFile {
    pub fn new(
        path: impl Into<String>,
        name: impl Into<String>,
        kind: FileKind,
        modified_unix_s: Option<i64>,
    ) -> Self {
        let path = path.into();
        let name = name.into();
        let stem = stem_of(&name, kind);
        let normalized_name = normalize_for_search(&name);
        let normalized_stem = normalize_for_search(&stem);
        let name_len = normalized_name.chars().count();
        let stem_len = normalized_stem.chars().count();
        Self {
            path,
            name,
            stem,
            kind,
            modified_unix_s,
            normalized_name,
            normalized_stem,
            name_len,
            stem_len,
        }
    }

    pub fn extension(&self) -> Option<&str> {
        match self.kind {
            FileKind::Folder => None,
            // A single leading dot is part of the name, matching `stem_of`: a
            // dotfile has no extension, so `.bashrc` is not a file of type
            // "bashrc".
            FileKind::File => self
                .name
                .rsplit_once('.')
                .filter(|(stem, extension)| !stem.is_empty() && !extension.is_empty())
                .map(|(_, extension)| extension),
        }
    }
}

/// The name without its last extension. A leading dot is part of the name
/// (`.gitignore` is all stem), and a folder never has one.
fn stem_of(name: &str, kind: FileKind) -> String {
    if kind == FileKind::Folder {
        return name.to_string();
    }
    match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() && !extension.is_empty() => stem.to_string(),
        _ => name.to_string(),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BatchMatch {
    pub path: String,
    pub name: String,
    pub extension: Option<String>,
    pub size_bytes: Option<u64>,
    pub modified_unix_s: Option<i64>,
    pub kind: FileKind,
    pub score: u8,
    pub match_type: MatchKind,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BatchEntryResult {
    pub requested: String,
    /// Derived from the best candidate, not from the displayed page.
    pub status: BatchStatus,
    /// Best first, at most the requested limit.
    pub matches: Vec<BatchMatch>,
    /// How many candidates passed the threshold, before the limit.
    pub total_matches: usize,
    /// Wall-clock time this one name took. Measured per entry because a batch
    /// is dominated by a few slow requests, not spread evenly.
    pub duration_ms: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BatchSummary {
    pub total: usize,
    pub found: usize,
    pub not_found: usize,
    /// Entries whose best match is an exact or stem equality.
    pub exact: usize,
    /// Entries found, but not exactly.
    pub approximate: usize,
    /// Entries with more than one candidate at or above the threshold.
    pub multiple: usize,
    /// Entries that could not be searched at all: after normalization the
    /// request had no characters left (a line of diacritics or tatweel), so no
    /// honest score exists for it. Counted separately from "not found", which
    /// means the search ran and the index did not contain the name.
    pub errors: usize,
    pub success_percent: f64,
    pub duration_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BatchReport {
    pub threshold: u8,
    /// How many files and folders the batch was matched against. Carried with
    /// the results so the report can say what the search covered without the
    /// UI passing a number of its own.
    pub indexed_files: usize,
    pub entries: Vec<BatchEntryResult>,
    pub summary: BatchSummary,
}

/// A request, normalized once and shared by every candidate comparison.
struct Query {
    text: String,
    /// The request as characters, once. Rebuilding this per candidate was the
    /// single largest cost of the fuzzy pass.
    chars: Vec<char>,
    len: usize,
    tokens: Vec<String>,
}

impl Query {
    fn new(requested: &str) -> Option<Self> {
        let text = normalize_for_search(requested.trim());
        if text.is_empty() {
            return None;
        }
        let chars: Vec<char> = text.chars().collect();
        let len = chars.len();
        let tokens = tokens_of(&text);
        Some(Self {
            text,
            chars,
            len,
            tokens,
        })
    }
}

/// Runs the whole batch. Pure apart from the `stat` that fills in file sizes for
/// the handful of matches that are returned.
pub fn search(files: &[BatchFile], entries: &[String], threshold: u8) -> BatchReport {
    search_with_limit(files, entries, threshold, DEFAULT_MATCH_LIMIT)
}

pub fn search_with_limit(
    files: &[BatchFile],
    entries: &[String],
    threshold: u8,
    limit: usize,
) -> BatchReport {
    let started = Instant::now();
    let threshold = threshold.clamp(MIN_THRESHOLD, MAX_THRESHOLD);
    let index = ExactIndex::new(files);

    // Entries are independent, and a batch is dominated by the ones that miss:
    // each of those scans the whole index. Spreading them over the cores is the
    // difference between a progress bar and a frozen window on a large paste.
    let results: Vec<BatchEntryResult> = entries
        .par_iter()
        .map(|requested| search_one(&index, files, requested, threshold, limit))
        .collect();

    let mut summary = BatchSummary {
        total: results.len(),
        ..BatchSummary::default()
    };
    for entry in &results {
        if entry.status == BatchStatus::Error {
            summary.errors += 1;
            continue;
        }
        if entry.matches.is_empty() {
            summary.not_found += 1;
            continue;
        }
        summary.found += 1;
        if entry.status == BatchStatus::Exact {
            summary.exact += 1;
        } else {
            summary.approximate += 1;
        }
        if entry.total_matches > 1 {
            summary.multiple += 1;
        }
    }
    summary.success_percent = if summary.total == 0 {
        0.0
    } else {
        (summary.found as f64 / summary.total as f64 * 1000.0).round() / 10.0
    };
    summary.duration_ms = started.elapsed().as_millis() as u64;

    BatchReport {
        threshold,
        indexed_files: files.len(),
        entries: results,
        summary,
    }
}

/// One requested name, start to finish.
fn search_one(
    index: &ExactIndex<'_>,
    files: &[BatchFile],
    requested: &str,
    threshold: u8,
    limit: usize,
) -> BatchEntryResult {
    let started = Instant::now();
    let mut result = BatchEntryResult {
        requested: requested.to_string(),
        status: BatchStatus::NotFound,
        matches: Vec::new(),
        total_matches: 0,
        duration_ms: 0,
    };

    match Query::new(requested) {
        Some(query) => {
            let mut found = index.exact(&query, files);
            let (total, best) = if found.is_empty() {
                if threshold < MAX_THRESHOLD {
                    // At 100% the fuzzy pass is provably empty: containment and
                    // fuzzy both cap at 99, so every candidate would be
                    // rejected. Skipping it turns a full index scan into nothing.
                    let (best, counted, top) = fuzzy_matches(files, &query, threshold, limit);
                    found = best;
                    (counted, top)
                } else {
                    (0, None)
                }
            } else {
                (found.len(), Some(100))
            };
            // The status follows the best candidate, not the displayed page, so
            // a display limit of zero cannot turn a found file into a missing
            // one.
            result.status = status_of(best);
            result.total_matches = total;
            found.truncate(limit);
            for entry in &mut found {
                entry.size_bytes = size_of(&entry.path, entry.kind);
            }
            result.matches = found;
        }
        // Nothing survived normalization, so there is nothing to compare. This
        // is reported as an error rather than as "not found": the index was
        // never the reason the name did not match.
        None => result.status = BatchStatus::Error,
    }

    result.duration_ms = started.elapsed().as_millis() as u64;
    result
}

fn status_of(best: Option<u8>) -> BatchStatus {
    match best {
        None => BatchStatus::NotFound,
        Some(score) if score >= 100 => BatchStatus::Exact,
        Some(score) if score >= 85 => BatchStatus::High,
        Some(_) => BatchStatus::Possible,
    }
}

fn size_of(path: &str, kind: FileKind) -> Option<u64> {
    if kind == FileKind::Folder {
        return None;
    }
    std::fs::metadata(Path::new(path))
        .ok()
        .map(|metadata| metadata.len())
}

/// Name and stem to indices, so an exact request costs a hash lookup instead of
/// a scan. Built once per batch.
struct ExactIndex<'a> {
    by_name: HashMap<&'a str, Vec<usize>>,
    by_stem: HashMap<&'a str, Vec<usize>>,
}

impl<'a> ExactIndex<'a> {
    fn new(files: &'a [BatchFile]) -> Self {
        let mut by_name: HashMap<&str, Vec<usize>> = HashMap::new();
        let mut by_stem: HashMap<&str, Vec<usize>> = HashMap::new();
        for (index, file) in files.iter().enumerate() {
            by_name
                .entry(file.normalized_name.as_str())
                .or_default()
                .push(index);
            if file.normalized_stem != file.normalized_name {
                by_stem
                    .entry(file.normalized_stem.as_str())
                    .or_default()
                    .push(index);
            }
        }
        Self { by_name, by_stem }
    }

    /// An equality hit ends the search for this entry: guessing at near-misses
    /// when the file with exactly that name is present would bury the answer.
    fn exact(&self, query: &Query, files: &[BatchFile]) -> Vec<BatchMatch> {
        let mut seen = std::collections::HashSet::new();
        let mut matches = Vec::new();
        for (kind, table) in [
            (MatchKind::Exact, &self.by_name),
            (MatchKind::Stem, &self.by_stem),
        ] {
            for &index in table
                .get(query.text.as_str())
                .map(Vec::as_slice)
                .unwrap_or(&[])
            {
                if !seen.insert(index) {
                    continue;
                }
                matches.push(match_from(&files[index], 100, kind));
            }
        }
        sort_matches(&mut matches);
        matches
    }
}

/// The best few matches, the number that reached the threshold, and the top
/// score.
///
/// Only the survivors are materialised. A short request at a low threshold can
/// match most of a large index, and building a [`BatchMatch`] (three `String`
/// clones) for every one of them, sorting the lot and then keeping eight was
/// both a large transient allocation per entry and pure waste. The scan keeps a
/// bounded, unsorted shortlist and replaces its worst entry instead.
fn fuzzy_matches(
    files: &[BatchFile],
    query: &Query,
    threshold: u8,
    limit: usize,
) -> (Vec<BatchMatch>, usize, Option<u8>) {
    // One buffer for the whole pass: a `Vec<char>` per candidate would mean
    // tens of thousands of allocations per entry.
    let mut scratch = Vec::new();
    let mut kept: Vec<(u8, MatchKind, usize)> = Vec::with_capacity(limit.min(64));
    let mut total = 0usize;
    let mut top: Option<u8> = None;

    for (index, file) in files.iter().enumerate() {
        let Some((score, kind)) = best_score(file, query, threshold, &mut scratch) else {
            continue;
        };
        total += 1;
        if top.is_none_or(|current| score > current) {
            top = Some(score);
        }
        if limit == 0 {
            continue;
        }
        if kept.len() < limit {
            kept.push((score, kind, index));
            continue;
        }
        // Replace the weakest kept candidate when this one outranks it. `limit`
        // is small, so the linear scan is cheaper than a heap.
        if let Some(position) = kept
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| rank(a, b, files))
            .map(|(position, _)| position)
            && rank(&(score, kind, index), &kept[position], files) == std::cmp::Ordering::Greater
        {
            kept[position] = (score, kind, index);
        }
    }

    kept.sort_by(|a, b| rank(a, b, files).reverse());
    let matches = kept
        .into_iter()
        .map(|(score, kind, index)| match_from(&files[index], score, kind))
        .collect();
    (matches, total, top)
}

/// Orders two candidates the way the results list does: highest score first,
/// then name, then path. `Greater` means "ranks higher".
fn rank(a: &(u8, MatchKind, usize), b: &(u8, MatchKind, usize), files: &[BatchFile]) -> Ordering {
    let (left, right) = (&files[a.2], &files[b.2]);
    a.0.cmp(&b.0)
        .then_with(|| right.name.to_lowercase().cmp(&left.name.to_lowercase()))
        .then_with(|| right.path.cmp(&left.path))
}

/// The best score this file can offer for the request, or `None` when nothing
/// reaches the threshold.
///
/// The threshold only prunes the expensive pass; the score itself does not
/// depend on it. That matters because the number shown next to a result has to
/// be the file's own score: when the comparison was filtered by the threshold,
/// the same file reported 85 at an 80% threshold and 90 at an 86% one.
fn best_score(
    file: &BatchFile,
    query: &Query,
    threshold: u8,
    scratch: &mut Vec<char>,
) -> Option<(u8, MatchKind)> {
    let mut best: Option<(u8, MatchKind)> = None;

    // A folder's stem is its name, so the second pass would repeat the first.
    let mut texts: [(&str, usize); 2] = [
        (&file.normalized_name, file.name_len),
        (&file.normalized_stem, file.stem_len),
    ];
    let passes = if file.normalized_stem == file.normalized_name {
        texts[1] = texts[0];
        1
    } else {
        2
    };

    for (text, len) in texts.into_iter().take(passes) {
        if let Some(score) = containment_score(&query.text, text) {
            improve(&mut best, score, MatchKind::Containment);
        }
        if query.tokens.len() > 1 && all_tokens_present(text, &query.tokens) {
            improve(&mut best, TOKEN_SCORE, MatchKind::Tokens);
        }
        // Nothing can beat 99, and the edit distance is the costly pass, so it
        // is skipped once the answer cannot improve.
        if best.is_none_or(|(current, _)| current < 99)
            && let Some(score) = fuzzy_score(query, text, len, threshold, scratch)
        {
            improve(&mut best, score, MatchKind::Fuzzy);
        }
    }

    best.filter(|(score, _)| *score >= threshold)
}

/// Keeps the highest score seen. The threshold is applied by the caller, so the
/// score a file reports is its own and not a function of the slider.
fn improve(best: &mut Option<(u8, MatchKind)>, score: u8, kind: MatchKind) {
    if best.is_none_or(|(current, _)| score > current) {
        *best = Some((score, kind));
    }
}

/// `60 + 40 * short/long`, or `None` when neither text is inside the other or
/// the shorter one is too short to be meaningful.
fn containment_score(query: &str, candidate: &str) -> Option<u8> {
    if candidate.is_empty() || query.is_empty() {
        return None;
    }
    let (short, long) = if query.len() <= candidate.len() {
        (query, candidate)
    } else {
        (candidate, query)
    };
    let short_chars = short.chars().count();
    if short_chars < MIN_CONTAINMENT_CHARS || !long.contains(short) {
        return None;
    }
    let long_chars = long.chars().count();
    let ratio = short_chars as f64 / long_chars.max(1) as f64;
    let mut score = f64::from(CONTAINMENT_BASE) + (100.0 - f64::from(CONTAINMENT_BASE)) * ratio;
    if long.starts_with(short) {
        score += f64::from(PREFIX_BONUS);
    }
    Some(score.round().clamp(0.0, 99.0) as u8)
}

/// Levenshtein similarity as a percentage, or `None` when the candidate cannot
/// reach the threshold. `candidate_len` is the character count, precomputed.
fn fuzzy_score(
    query: &Query,
    candidate: &str,
    candidate_len: usize,
    threshold: u8,
    scratch: &mut Vec<char>,
) -> Option<u8> {
    if query.len == 0 || candidate_len == 0 {
        return None;
    }
    let max_len = query.len.max(candidate_len);
    let allowed = max_distance(max_len, threshold);
    // The length difference alone is a lower bound on the edit distance.
    if query.len.abs_diff(candidate_len) > allowed {
        return None;
    }
    scratch.clear();
    scratch.extend(candidate.chars());
    let distance = bounded_levenshtein(&query.chars, scratch, allowed);
    if distance > allowed {
        return None;
    }
    let similarity = 1.0 - distance as f64 / max_len as f64;
    let score = (similarity * 100.0).round().clamp(0.0, 99.0);
    Some(score as u8)
}

/// The largest edit distance that can still reach `threshold` percent of
/// `max_len`, capped at [`MAX_EDITS`]. At 100% that is zero: only equality.
fn max_distance(max_len: usize, threshold: u8) -> usize {
    let slack = usize::from(MAX_THRESHOLD - threshold.clamp(MIN_THRESHOLD, MAX_THRESHOLD));
    // The score is `round(100 * (1 - d / max_len))`, so the budget has to round
    // the same way. Floor division dropped a distance whose rounded score still
    // equals the threshold: one edit in eleven characters is 91%, but at a 91%
    // threshold the floor rule allowed zero edits and the match was never
    // scored.
    ((max_len * slack + max_len / 2) / 100).min(MAX_EDITS)
}

fn match_from(file: &BatchFile, score: u8, match_type: MatchKind) -> BatchMatch {
    BatchMatch {
        path: file.path.clone(),
        name: file.name.clone(),
        extension: file.extension().map(str::to_string),
        size_bytes: None,
        modified_unix_s: file.modified_unix_s,
        kind: file.kind,
        score,
        match_type,
    }
}

fn sort_matches(matches: &mut [BatchMatch]) {
    // `sort_by_cached_key` lowercases each name once instead of twice per
    // comparison. The lists are short, but this runs for every request in the
    // batch and the comparator was the only per-comparison allocation left in
    // the matcher.
    matches.sort_by_cached_key(|entry| {
        (
            std::cmp::Reverse(entry.score),
            entry.name.to_lowercase(),
            entry.path.clone(),
        )
    });
}

/// Splits on anything that is not a letter or a digit. Arabic letters are
/// alphanumeric, so Arabic words survive as tokens.
fn tokens_of(text: &str) -> Vec<String> {
    text.split(|ch: char| !ch.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .collect()
}

/// Whether every request token appears as a whole token in `text`.
///
/// The count is over *request* tokens that have been seen, not over candidate
/// tokens that matched: counting occurrences let a file named `alpha-alpha.pdf`
/// satisfy the request `alpha beta`, because one repeated token supplied both
/// hits. Comparisons are slices of the already-normalized text, so there is no
/// allocation per candidate.
fn all_tokens_present(text: &str, tokens: &[String]) -> bool {
    // A request has a handful of words, so a bitmask covers it without
    // allocating; a pathological request falls back to the general path.
    let mut seen = 0u64;
    let mut found = 0usize;
    for token in text.split(|ch: char| !ch.is_alphanumeric()) {
        if token.is_empty() {
            continue;
        }
        for (index, wanted) in tokens.iter().enumerate() {
            if wanted == token && seen & (1 << index) == 0 {
                seen |= 1 << index;
                found += 1;
                if found == tokens.len() {
                    return true;
                }
                // A token may satisfy several identical request words, but
                // duplicates are already collapsed by `Query`, so one is enough.
                break;
            }
        }
    }
    false
}

/// Character-level edit distance, computed only inside the diagonal band
/// `|i - j| <= max` and abandoned as soon as a whole row leaves it. Returns
/// `max + 1` when the distance is larger; the caller only compares against
/// `max`.
///
/// Two properties make the early exit sound: the band holds every cell whose
/// distance can still be `<= max` (each step changes `i - j` by at most one),
/// and the minimum of a row never decreases as rows advance (appending a
/// character to both strings cannot shorten the distance).
///
/// `row[k]` is `D(i, j)` for `j = i - max - 1 + k`, so a row is `2 * max + 3`
/// entries wide no matter how long the strings are.
fn bounded_levenshtein(a: &[char], b: &[char], max: usize) -> usize {
    /// `2 * MAX_EDITS + 3`: the widest a row can be, so the rows live on the
    /// stack instead of being allocated per candidate.
    const WIDTH: usize = 2 * MAX_EDITS + 3;
    debug_assert!(max <= MAX_EDITS, "band wider than MAX_EDITS");
    let (n, m) = (a.len(), b.len());
    if n.abs_diff(m) > max {
        return max + 1;
    }
    let width = 2 * max + 3;
    let out_of_band = max + 1;
    let mut previous = [out_of_band; WIDTH];
    for (k, slot) in previous.iter_mut().enumerate().take(width) {
        // Row 0: `j = -max - 1 + k`, and `D(0, j) = j`.
        let j = k as isize - max as isize - 1;
        if (0..=m as isize).contains(&j) {
            *slot = j as usize;
        }
    }
    let mut current = [out_of_band; WIDTH];

    for i in 1..=n {
        current[..width].fill(out_of_band);
        let mut row_min = out_of_band;
        // `D(i, j - 1)` for the left edge of the band is outside it.
        let mut left = out_of_band;
        // The last slot of the row would need `previous[width]`; its cell is
        // `j = i + max + 1`, outside the band by construction, so it is never
        // read for an answer.
        for k in 0..width - 1 {
            let j = i as isize - max as isize - 1 + k as isize;
            if j < 0 || j > m as isize {
                continue;
            }
            let j = j as usize;
            let value = if j == 0 {
                // `D(i, 0) = i`: every character of `a` deleted.
                i.min(out_of_band)
            } else {
                let substitution = if a[i - 1] == b[j - 1] {
                    previous[k]
                } else {
                    previous[k].saturating_add(1)
                };
                substitution
                    .min(previous[k + 1].saturating_add(1))
                    .min(left.saturating_add(1))
                    .min(out_of_band)
            };
            current[k] = value;
            left = value;
            row_min = row_min.min(value);
        }
        if row_min > max {
            return out_of_band;
        }
        std::mem::swap(&mut previous, &mut current);
    }

    // `D(n, m)` sits at `k = m - n + max + 1`.
    let k = m as isize - n as isize + max as isize + 1;
    if k < 0 || k as usize >= width {
        return out_of_band;
    }
    previous[k as usize]
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;

    fn files(names: &[&str]) -> Vec<BatchFile> {
        names
            .iter()
            .map(|name| {
                BatchFile::new(
                    format!("/docs/{name}"),
                    *name,
                    FileKind::File,
                    Some(1_700_000_000),
                )
            })
            .collect()
    }

    fn entry<'a>(report: &'a BatchReport, requested: &str) -> &'a BatchEntryResult {
        report
            .entries
            .iter()
            .find(|entry| entry.requested == requested)
            .expect("requested entry")
    }

    #[test]
    fn an_exact_name_beats_every_near_miss() {
        let report = search(
            &files(&["invoice.pdf", "invoice-2024.pdf", "invoices.pdf"]),
            &["invoice.pdf".to_string()],
            70,
        );
        let found = entry(&report, "invoice.pdf");
        assert_eq!(found.status, BatchStatus::Exact);
        assert_eq!(found.matches.len(), 1);
        assert_eq!(found.matches[0].name, "invoice.pdf");
        assert_eq!(found.matches[0].match_type, MatchKind::Exact);
    }

    #[test]
    fn a_name_without_its_extension_finds_the_file() {
        let report = search(&files(&["report.pdf"]), &["report".to_string()], 70);
        let found = entry(&report, "report");
        assert_eq!(found.status, BatchStatus::Exact);
        assert_eq!(found.matches[0].match_type, MatchKind::Stem);
    }

    #[test]
    fn a_partial_name_is_containment_not_equality() {
        let report = search(
            &files(&["invoice-2024-final.pdf"]),
            &["invoice".to_string()],
            70,
        );
        let found = entry(&report, "invoice");
        assert_eq!(found.status, BatchStatus::High);
        assert_eq!(found.matches[0].match_type, MatchKind::Containment);
        assert!(found.matches[0].score >= 70 && found.matches[0].score < 100);
    }

    #[test]
    fn a_typo_is_found_by_edit_distance() {
        let report = search(
            &files(&["quarterly-report.pdf"]),
            &["quartrly-report.pdf".to_string()],
            80,
        );
        let found = entry(&report, "quartrly-report.pdf");
        assert_eq!(found.matches[0].name, "quarterly-report.pdf");
        assert_eq!(found.matches[0].match_type, MatchKind::Fuzzy);
    }

    #[test]
    fn words_in_any_order_match_on_tokens() {
        let report = search(
            &files(&["2024-invoice-march.pdf"]),
            &["invoice 2024".to_string()],
            70,
        );
        let found = entry(&report, "invoice 2024");
        assert_eq!(found.matches[0].match_type, MatchKind::Tokens);
        assert_eq!(found.matches[0].score, TOKEN_SCORE);
    }

    #[test]
    fn the_threshold_changes_the_answer() {
        let candidates = files(&["quarterly-report.pdf"]);
        // Two edits in a seventeen-character name: 88% similar.
        let request = "quartely-reports.pdf".to_string();
        let strict = search(&candidates, std::slice::from_ref(&request), 95);
        let loose = search(&candidates, std::slice::from_ref(&request), 60);
        assert_eq!(entry(&strict, &request).status, BatchStatus::NotFound);
        assert_ne!(entry(&loose, &request).status, BatchStatus::NotFound);
    }

    #[test]
    fn one_hundred_percent_is_equality_only() {
        let candidates = files(&["invoice-2024.pdf"]);
        let partial = "invoice".to_string();
        let report = search(&candidates, std::slice::from_ref(&partial), 100);
        assert_eq!(entry(&report, &partial).status, BatchStatus::NotFound);

        let exact = "Invoice-2024.PDF".to_string();
        let report = search(&candidates, std::slice::from_ref(&exact), 100);
        assert_eq!(entry(&report, &exact).status, BatchStatus::Exact);
    }

    /// A repeated candidate token must not stand in for two different request
    /// words. `alpha-alpha-2024.pdf` used to report 85% / Tokens for the request
    /// `alpha beta`, although `beta` appears nowhere in the name.
    #[test]
    fn a_repeated_candidate_word_does_not_satisfy_two_request_words() {
        let candidates = files(&["alpha-alpha-2024.pdf"]);
        let request = "alpha beta".to_string();
        let report = search(&candidates, std::slice::from_ref(&request), 85);
        let found = entry(&report, &request);
        assert_ne!(
            found.status,
            BatchStatus::High,
            "a repeated token must not pass as two words: {found:?}"
        );
        assert!(
            found
                .matches
                .iter()
                .all(|hit| hit.match_type != MatchKind::Tokens),
            "no token match may be claimed: {found:?}"
        );
    }

    /// A request whose words are genuinely all present still matches.
    #[test]
    fn every_request_word_present_is_still_a_token_match() {
        let candidates = files(&["alpha-beta-2024.pdf"]);
        let request = "alpha beta".to_string();
        let report = search(&candidates, std::slice::from_ref(&request), 85);
        let found = entry(&report, &request);
        assert_eq!(found.status, BatchStatus::High);
        assert_eq!(found.matches[0].match_type, MatchKind::Tokens);
    }

    /// The edit budget has to round like the score does. One substitution in an
    /// eleven-character name is 91%; at a 91% threshold the floor-based budget
    /// allowed zero edits, so the match was silently dropped.
    #[test]
    fn a_score_equal_to_the_threshold_is_not_dropped() {
        let candidates = files(&["invoice.pdf"]);
        let request = "invoicf.pdf".to_string();
        for threshold in [90, 91] {
            let report = search(&candidates, std::slice::from_ref(&request), threshold);
            let found = entry(&report, &request);
            assert_ne!(
                found.status,
                BatchStatus::NotFound,
                "the match scores 91 and must survive a {threshold}% threshold: {found:?}"
            );
            assert_eq!(found.matches[0].score, 91);
        }
    }

    /// The number shown next to a result is the file's own score. Filtering the
    /// comparison by the threshold made the same file report 85 at an 80%
    /// threshold and 90 at an 86% one.
    #[test]
    fn a_reported_score_does_not_depend_on_the_threshold() {
        let candidates = files(&["alpha-beta.pdf"]);
        let request = "alpha beta".to_string();
        let mut scores = Vec::new();
        for threshold in [50, 70, 80, 85, 86, 90] {
            let report = search(&candidates, std::slice::from_ref(&request), threshold);
            let found = entry(&report, &request);
            if found.status != BatchStatus::NotFound {
                scores.push(found.matches[0].score);
            }
        }
        assert!(!scores.is_empty(), "the file matches at the loose end");
        assert!(
            scores.windows(2).all(|pair| pair[0] == pair[1]),
            "one file, one score, whatever the slider says: {scores:?}"
        );
    }

    /// A display limit is a display concern: it must not turn a found file into
    /// a missing one, and the total must still be reported.
    #[test]
    fn a_zero_display_limit_still_reports_the_file_as_found() {
        let candidates = files(&["report.pdf"]);
        let request = "report.pdf".to_string();
        let report = search_with_limit(&candidates, std::slice::from_ref(&request), 70, 0);
        let found = entry(&report, &request);
        assert_eq!(found.status, BatchStatus::Exact);
        assert_eq!(found.total_matches, 1);
        assert!(found.matches.is_empty());
    }

    /// The shortlist is bounded, so a broad request over a large index cannot
    /// materialise a match for every file.
    #[test]
    fn only_the_display_limit_is_materialised() {
        let names: Vec<String> = (0..500).map(|i| format!("report-{i:03}.pdf")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let candidates = files(&refs);
        let request = "report".to_string();
        let report = search_with_limit(&candidates, std::slice::from_ref(&request), 60, 3);
        let found = entry(&report, &request);
        assert_eq!(found.matches.len(), 3, "the display limit is honoured");
        assert_eq!(found.total_matches, 500, "the total is still counted");
    }

    /// A dotfile has no extension, and the stem, the extension and the report
    /// have to agree about it.
    #[test]
    fn a_dotfile_has_no_extension() {
        let candidates = files(&[".bashrc"]);
        assert_eq!(candidates[0].stem, ".bashrc");
        assert_eq!(candidates[0].extension(), None);
    }

    #[test]
    fn a_one_letter_request_does_not_match_everything() {
        let candidates = files(&["annual-report.pdf", "a.pdf"]);
        let report = search(&candidates, &["a".to_string()], 50);
        // `a.pdf` is an exact stem match; the long name must not appear as a
        // containment match.
        assert_eq!(entry(&report, "a").matches.len(), 1);
        assert_eq!(entry(&report, "a").matches[0].name, "a.pdf");
    }

    #[test]
    fn arabic_variants_meet_on_the_same_file() {
        let candidates = files(&["مدرسة-الأطفال.pdf"]);
        // Typed with ha instead of ta marbuta, and without the hamza.
        let report = search(&candidates, &["مدرسه-الاطفال".to_string()], 90);
        let found = entry(&report, "مدرسه-الاطفال");
        assert_ne!(found.status, BatchStatus::NotFound);
        assert_eq!(found.matches[0].name, "مدرسة-الأطفال.pdf");
    }

    #[test]
    fn arabic_indic_digits_match_ascii_ones() {
        let candidates = files(&["تقرير-٢٠٢٦.pdf"]);
        let report = search(&candidates, &["تقرير-2026.pdf".to_string()], 90);
        assert_eq!(entry(&report, "تقرير-2026.pdf").status, BatchStatus::Exact);
    }

    #[test]
    fn a_name_that_appears_twice_reports_both() {
        let candidates = vec![
            BatchFile::new("/a/report.pdf", "report.pdf", FileKind::File, None),
            BatchFile::new("/b/report.pdf", "report.pdf", FileKind::File, None),
        ];
        let report = search(&candidates, &["report.pdf".to_string()], 70);
        let found = entry(&report, "report.pdf");
        assert_eq!(found.matches.len(), 2);
        assert_eq!(found.total_matches, 2);
        assert_eq!(report.summary.multiple, 1);
    }

    #[test]
    fn the_match_limit_truncates_but_the_total_stays_honest() {
        let candidates = files(&["report-1.pdf", "report-2.pdf", "report-3.pdf"]);
        let report = search_with_limit(&candidates, &["report".to_string()], 60, 2);
        let found = entry(&report, "report");
        assert_eq!(found.matches.len(), 2);
        assert_eq!(found.total_matches, 3);
    }

    #[test]
    fn the_summary_adds_up() {
        let candidates = files(&["alpha.pdf", "beta.pdf"]);
        let report = search(
            &candidates,
            &["alpha.pdf".to_string(), "gamma.pdf".to_string()],
            70,
        );
        assert_eq!(report.summary.total, 2);
        assert_eq!(report.summary.found, 1);
        assert_eq!(report.summary.not_found, 1);
        assert_eq!(report.summary.exact, 1);
        assert_eq!(report.summary.success_percent, 50.0);
        assert_eq!(
            report.summary.total,
            report.summary.found + report.summary.not_found + report.summary.errors
        );
    }

    #[test]
    fn a_request_with_no_searchable_characters_is_an_error_not_a_miss() {
        // Tatweel is decorative and is dropped by normalization, so nothing is
        // left to compare. Reporting it as "not found" would blame the index.
        let report = search(
            &files(&["a.pdf"]),
            &["\u{0640}\u{0640}".to_string(), "a.pdf".to_string()],
            50,
        );
        assert_eq!(
            entry(&report, "\u{0640}\u{0640}").status,
            BatchStatus::Error
        );
        assert!(entry(&report, "\u{0640}\u{0640}").matches.is_empty());
        assert_eq!(report.summary.errors, 1);
        assert_eq!(report.summary.not_found, 0);
        assert_eq!(report.summary.found, 1);
        assert_eq!(report.summary.total, 2);
    }

    #[test]
    fn a_blank_request_is_an_error_rather_than_everything() {
        // A blank line has no name to compare, so it must not be scored as a
        // miss against the index; the parser drops such lines, and a direct
        // caller gets the same answer.
        let report = search(&files(&["a.pdf"]), &["   ".to_string()], 50);
        assert_eq!(entry(&report, "   ").status, BatchStatus::Error);
        assert_eq!(report.summary.errors, 1);
        assert!(entry(&report, "   ").matches.is_empty());
    }

    /// The textbook full-matrix distance, used only to check the banded one.
    fn reference_distance(a: &[char], b: &[char]) -> usize {
        let mut previous: Vec<usize> = (0..=b.len()).collect();
        for (i, ca) in a.iter().enumerate() {
            let mut current = vec![i + 1; b.len() + 1];
            for (j, cb) in b.iter().enumerate() {
                current[j + 1] = if ca == cb {
                    previous[j]
                } else {
                    1 + previous[j].min(previous[j + 1]).min(current[j])
                };
            }
            previous = current;
        }
        previous[b.len()]
    }

    #[test]
    fn the_banded_distance_agrees_with_the_full_matrix() {
        let mut seed = 0x1234_5678u32;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        let alphabet = ['a', 'b', 'c', 'd'];
        for _ in 0..600 {
            let a: Vec<char> = (0..next() % 13)
                .map(|_| alphabet[(next() % 4) as usize])
                .collect();
            let b: Vec<char> = (0..next() % 13)
                .map(|_| alphabet[(next() % 4) as usize])
                .collect();
            let exact = reference_distance(&a, &b);
            for max in 0..=4usize {
                let expected = exact.min(max + 1);
                assert_eq!(
                    bounded_levenshtein(&a, &b, max),
                    expected,
                    "a={a:?} b={b:?} max={max} exact={exact}"
                );
            }
        }
    }

    #[test]
    fn bounded_distance_abandons_hopeless_candidates() {
        let a: Vec<char> = "abcdefghij".chars().collect();
        let b: Vec<char> = "abcdefghij".chars().collect();
        assert_eq!(bounded_levenshtein(&a, &b, 0), 0);
        let c: Vec<char> = "xxxxx".chars().collect();
        assert_eq!(bounded_levenshtein(&a, &c, 2), 3);
    }

    #[test]
    fn a_folder_has_no_extension_and_keeps_its_whole_name_as_stem() {
        let folder = BatchFile::new("/docs/Archive 2024", "Archive 2024", FileKind::Folder, None);
        assert_eq!(folder.extension(), None);
        assert_eq!(folder.stem, "Archive 2024");
    }
}
