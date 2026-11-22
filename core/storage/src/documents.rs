//! Text extracted from documents, indexed so the launcher can find a file by
//! what is *inside* it.
//!
//! This is the other half of local OCR: the Studio already turns a scan into
//! text, and until now that text was exported and forgotten. Storing it here
//! makes a document searchable by a phrase it contains, offline, with no second
//! service.
//!
//! The index is deliberately small and boring:
//!
//! * `document_text` holds one row per file: the text, its page count, and
//!   where it came from (`ocr`, later `docx`/`xlsx`/`text`).
//! * `document_token` is an inverted index. A query is split into the same
//!   tokens the writer used and the candidate set is the intersection of their
//!   posting lists, so a search costs one index lookup per query token instead
//!   of a scan of every stored page.
//!
//! No FTS5 dependency: the token table is a plain table with a covering primary
//! key, which is enough for the scale this feature targets (tens of thousands
//! of documents, each one a page or two of text) and keeps the SQLite build the
//! app ships unchanged.
//!
//! Both sides call [`normalize_for_search`] from `zeidex-matching`, the same
//! fold the launcher uses, so `صفحة` and `صفحه` (or `CAFÉ` and `cafe`) meet in
//! the index instead of becoming two vocabularies.

use rusqlite::{OptionalExtension, params};
use zeidex_matching::normalize_for_search;

use crate::{SqliteStore, StorageResult};

/// One stored document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentText {
    /// Absolute path of the file the text came from. Unique: re-running OCR on
    /// the same file replaces its text instead of adding a second row.
    pub path: String,
    pub body: String,
    pub page_count: usize,
    /// `ocr`, `docx`, `xlsx`, `odt` or `text`.
    pub source: String,
    /// Filesystem mtime at the time the text was read, when known. Lets a later
    /// pass notice that the file changed and the text is stale.
    pub modified_at_unix_s: Option<i64>,
}

/// One search hit, with a readable excerpt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentHit {
    pub path: String,
    pub snippet: String,
    pub page_count: usize,
    pub source: String,
}

/// What one staleness pass changed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PrunedDocuments {
    /// Files that no longer exist.
    pub missing: usize,
    /// Files whose mtime no longer matches the indexed one.
    pub changed: usize,
}

impl PrunedDocuments {
    pub fn total(&self) -> usize {
        self.missing + self.changed
    }
}

/// The most text one document may contribute. A 400-page scan would otherwise
/// put megabytes into one row and make every write slow; 400k characters is far
/// beyond what a search result needs and still covers a long report.
pub const MAX_DOCUMENT_BODY_CHARS: usize = 400_000;

/// How many documents the index keeps. Past this, the oldest indexed document
/// is dropped: a bounded index beats an unbounded one on a laptop.
pub const MAX_DOCUMENT_ROWS: usize = 20_000;

/// Longest token kept. A single "word" longer than this is a base64 blob or a
/// broken line, not something a user searches for.
const MAX_TOKEN_CHARS: usize = 64;

/// Characters of context shown around a hit.
const SNIPPET_CHARS: usize = 160;

/// Setting key: whether a finished OCR run stores its text for search.
const SETTINGS_KEY_DOCUMENT_INDEXING: &str = "document_indexing_enabled";

/// Schema for the document index. Called from the store's migration so an
/// existing database gains the tables on next open.
pub(crate) fn migrate_document_text(conn: &rusqlite::Connection) -> StorageResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS document_text (
             id                 INTEGER PRIMARY KEY AUTOINCREMENT,
             path               TEXT NOT NULL UNIQUE,
             body               TEXT NOT NULL,
             page_count         INTEGER NOT NULL DEFAULT 1,
             source             TEXT NOT NULL,
             indexed_at_unix_s  INTEGER NOT NULL,
             modified_at_unix_s INTEGER
         );

         CREATE TABLE IF NOT EXISTS document_token (
             token       TEXT NOT NULL,
             document_id INTEGER NOT NULL REFERENCES document_text(id) ON DELETE CASCADE,
             PRIMARY KEY (token, document_id)
         ) WITHOUT ROWID;",
    )?;
    Ok(())
}

/// Split text into the tokens the index stores.
///
/// Punctuation is a separator here, unlike in the filename matcher where
/// `main.go` is one name. A document's text is prose; `,` and `؛` are not part
/// of a word.
pub fn document_tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut length = 0usize;
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            if length < MAX_TOKEN_CHARS {
                current.push(ch);
                length += 1;
            }
        } else if !current.is_empty() {
            out.push(std::mem::take(&mut current));
            length = 0;
        }
    }
    if !current.is_empty() {
        out.push(current);
    }

    // Normalize after splitting: the fold can turn one character into two
    // (`Đ` → `d`), which would otherwise desynchronize the two sides.
    for token in &mut out {
        *token = normalize_for_search(token);
    }
    out.retain(|token| !token.is_empty());
    out.sort();
    out.dedup();
    out
}

/// The nearest character boundary at or before `index`. `str::find` returns a
/// byte offset into the lowercased copy, and lowercasing can change byte
/// lengths, so the offset is not always a boundary of the original.
fn floor_char_boundary(text: &str, index: usize) -> usize {
    let mut index = index.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

/// A short excerpt around the query, in the document's own spelling.
fn snippet(body: &str, query_tokens: &[String]) -> String {
    let collapsed = body.split_whitespace().collect::<Vec<_>>().join(" ");

    // Prefer a window around the first query token as the user typed it. The
    // stored text is not normalized, so an Arabic query matches literally; a
    // query that only matches after folding (an alef variant, a diacritic)
    // falls back to the opening, which is still a fair preview.
    let lowered = collapsed.to_lowercase();
    let anchor = query_tokens
        .iter()
        .filter_map(|token| lowered.find(token.as_str()))
        .min()
        .unwrap_or(0);
    let anchor = floor_char_boundary(&collapsed, anchor);
    let start = collapsed[..anchor]
        .char_indices()
        .rev()
        .nth(SNIPPET_CHARS)
        .map(|(index, _)| index)
        .unwrap_or(0);
    let window: String = collapsed[start..].chars().take(SNIPPET_CHARS).collect();
    let mut out = window.trim().to_string();
    if start > 0 {
        out.insert(0, '…');
    }
    if collapsed.chars().count() > start + SNIPPET_CHARS {
        out.push('…');
    }
    out
}

impl SqliteStore {
    /// Store or replace the text of one document. An empty body removes the row
    /// instead: a page with no recognised text must not shadow an older, useful
    /// index entry for the same path.
    pub fn upsert_document_text(&mut self, document: &DocumentText) -> StorageResult<bool> {
        let body: String = document
            .body
            .chars()
            .take(MAX_DOCUMENT_BODY_CHARS)
            .collect();
        if body.trim().is_empty() {
            return self.remove_document_text(&document.path);
        }

        let tokens = document_tokens(&body);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs() as i64)
            .unwrap_or(0);

        let tx = self.write_tx()?;
        let document_id: i64 = tx.query_row(
            "INSERT INTO document_text (path, body, page_count, source, indexed_at_unix_s, modified_at_unix_s)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(path) DO UPDATE SET
                 body = excluded.body,
                 page_count = excluded.page_count,
                 source = excluded.source,
                 indexed_at_unix_s = excluded.indexed_at_unix_s,
                 modified_at_unix_s = excluded.modified_at_unix_s
             RETURNING id",
            params![
                document.path,
                body,
                document.page_count.max(1) as i64,
                document.source,
                now,
                document.modified_at_unix_s,
            ],
            |row| row.get(0),
        )?;

        tx.execute(
            "DELETE FROM document_token WHERE document_id = ?1",
            params![document_id],
        )?;
        {
            let mut insert = tx.prepare(
                "INSERT OR IGNORE INTO document_token (token, document_id) VALUES (?1, ?2)",
            )?;
            for token in &tokens {
                insert.execute(params![token, document_id])?;
            }
        }

        // Bound the index. Oldest first, and the cascade removes its tokens.
        let total: i64 =
            tx.query_row("SELECT COUNT(*) FROM document_text", [], |row| row.get(0))?;
        if total > MAX_DOCUMENT_ROWS as i64 {
            tx.execute(
                "DELETE FROM document_text WHERE id IN (
                     SELECT id FROM document_text
                     ORDER BY indexed_at_unix_s ASC, id ASC
                     LIMIT ?1
                 )",
                params![total - MAX_DOCUMENT_ROWS as i64],
            )?;
        }

        tx.commit()?;
        Ok(true)
    }

    /// Drop one document's text. Returns whether a row existed.
    pub fn remove_document_text(&mut self, path: &str) -> StorageResult<bool> {
        let tx = self.write_tx()?;
        let removed = tx.execute("DELETE FROM document_text WHERE path = ?1", params![path])?;
        tx.commit()?;
        Ok(removed > 0)
    }

    /// How many documents have searchable text.
    pub fn document_text_count(&self) -> StorageResult<usize> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM document_text", [], |row| row.get(0))?;
        Ok(count.max(0) as usize)
    }

    /// Forget every stored document text. The user's files are untouched.
    pub fn clear_document_text(&mut self) -> StorageResult<usize> {
        let tx = self.write_tx()?;
        let removed = tx.execute("DELETE FROM document_text", [])?;
        tx.commit()?;
        Ok(removed)
    }

    /// Drop the text of files that were deleted or rewritten since it was read.
    ///
    /// A search hit that opens a file whose text no longer matches is worse than
    /// no hit at all, and the row would otherwise live until the cap evicted it.
    /// This is a startup pass, not a per-query check: a stat per stored row on
    /// every keystroke would be the slowest thing the launcher does.
    ///
    /// A row with no recorded mtime is kept. Without a timestamp there is
    /// nothing to compare, and deleting a possibly-valid entry is worse than
    /// keeping a possibly-stale one. A stat that fails for any reason other than
    /// `NotFound` (permission denied, a dropped network mount) keeps the row
    /// too: "I could not look" is not "it is gone".
    pub fn prune_stale_document_text(&mut self) -> StorageResult<PrunedDocuments> {
        let rows: Vec<(i64, String, Option<i64>)> = self
            .conn
            .prepare("SELECT id, path, modified_at_unix_s FROM document_text")?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        if rows.is_empty() {
            return Ok(PrunedDocuments::default());
        }

        // Stat outside the write transaction: a slow mount must not hold the
        // store's only write lock while the filesystem answers.
        let mut pruned = PrunedDocuments::default();
        let mut stale: Vec<i64> = Vec::new();
        for (id, path, recorded) in rows {
            let Some(recorded) = recorded else {
                continue;
            };
            match std::fs::metadata(&path) {
                Ok(metadata) => {
                    let on_disk = metadata
                        .modified()
                        .ok()
                        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|duration| duration.as_secs() as i64);
                    if on_disk != Some(recorded) {
                        stale.push(id);
                        pruned.changed += 1;
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    stale.push(id);
                    pruned.missing += 1;
                }
                Err(_) => {}
            }
        }
        if stale.is_empty() {
            return Ok(pruned);
        }

        // `&mut self` means the caller holds the store's lock, so no row can be
        // re-indexed between the stat above and this delete.
        let tx = self.write_tx()?;
        {
            let mut delete = tx.prepare("DELETE FROM document_text WHERE id = ?1")?;
            for id in &stale {
                delete.execute(params![id])?;
            }
        }
        tx.commit()?;
        Ok(pruned)
    }

    /// Documents whose text contains every token of `query`, newest index entry
    /// first. An empty query matches nothing rather than everything.
    pub fn search_document_text(
        &self,
        query: &str,
        limit: usize,
    ) -> StorageResult<Vec<DocumentHit>> {
        let query_tokens = document_tokens(query);
        if query_tokens.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }

        // The rarest token drives the scan. A common word ("the", "في") would
        // otherwise build a huge intermediate set for every keystroke.
        let mut ordered = Vec::with_capacity(query_tokens.len());
        for token in &query_tokens {
            let count: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM document_token WHERE token = ?1",
                params![token],
                |row| row.get(0),
            )?;
            if count == 0 {
                return Ok(Vec::new());
            }
            ordered.push((count, token.clone()));
        }
        ordered.sort();

        let mut sql = String::from(
            "SELECT d.id, d.path, d.body, d.page_count, d.source
             FROM document_text d
             WHERE d.id IN (SELECT document_id FROM document_token WHERE token = ?1)",
        );
        for index in 1..ordered.len() {
            sql.push_str(&format!(
                " AND d.id IN (SELECT document_id FROM document_token WHERE token = ?{})",
                index + 1
            ));
        }
        sql.push_str(" ORDER BY d.indexed_at_unix_s DESC, d.id DESC LIMIT ?");
        sql.push_str(&(ordered.len() + 1).to_string());

        let mut statement = self.conn.prepare(&sql)?;
        let mut bindings: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(ordered.len() + 1);
        for (_, token) in &ordered {
            bindings.push(token);
        }
        let limit_i64 = limit as i64;
        bindings.push(&limit_i64);

        let mut rows = statement.query(bindings.as_slice())?;
        let mut hits = Vec::new();
        while let Some(row) = rows.next()? {
            let body: String = row.get(2)?;
            hits.push(DocumentHit {
                path: row.get(1)?,
                snippet: snippet(&body, &query_tokens),
                page_count: row.get::<_, i64>(3)?.max(1) as usize,
                source: row.get(4)?,
            });
        }
        Ok(hits)
    }

    /// Whether finished OCR runs should store their text. On by default: the
    /// text never leaves the machine, and a search that finds a scanned invoice
    /// by its contents is the point of the feature. The switch exists so a user
    /// who wants the OCR text to stay in memory only can say so.
    pub fn document_indexing_enabled(&self) -> StorageResult<bool> {
        let value: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![SETTINGS_KEY_DOCUMENT_INDEXING],
                |row| row.get(0),
            )
            .optional()?;
        Ok(value.as_deref() != Some("false"))
    }

    pub fn set_document_indexing_enabled(&mut self, enabled: bool) -> StorageResult<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![
                SETTINGS_KEY_DOCUMENT_INDEXING,
                if enabled { "true" } else { "false" }
            ],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> SqliteStore {
        SqliteStore::open_in_memory().expect("in-memory store")
    }

    fn doc(path: &str, body: &str) -> DocumentText {
        DocumentText {
            path: path.to_string(),
            body: body.to_string(),
            page_count: 1,
            source: "ocr".to_string(),
            modified_at_unix_s: None,
        }
    }

    #[test]
    fn tokens_fold_arabic_and_split_on_punctuation() {
        let tokens = document_tokens("صفحة 12، الفاتورة TOTAL: 4,500.00");
        assert!(tokens.contains(&"صفحه".to_string()), "{tokens:?}");
        assert!(tokens.contains(&"الفاتوره".to_string()), "{tokens:?}");
        assert!(tokens.contains(&"total".to_string()), "{tokens:?}");
        // A separator on both sides: the query tokenizes the same way, so the
        // two still meet even though `4,500.00` is not one token.
        assert!(tokens.contains(&"500".to_string()), "{tokens:?}");
        assert!(!tokens.iter().any(|token| token.contains('،')));
    }

    #[test]
    fn a_stored_document_is_found_by_a_word_inside_it() {
        let mut store = store();
        store
            .upsert_document_text(&doc("/docs/report.pdf", "Quarterly revenue grew by 12%."))
            .expect("store text");
        store
            .upsert_document_text(&doc("/docs/other.pdf", "Meeting notes about hiring."))
            .expect("store text");

        let hits = store.search_document_text("revenue", 10).expect("search");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "/docs/report.pdf");
        assert!(hits[0].snippet.contains("revenue"), "{}", hits[0].snippet);
    }

    #[test]
    fn a_query_needs_every_token_and_arabic_folds_both_ways() {
        let mut store = store();
        store
            .upsert_document_text(&doc("/docs/ar.pdf", "فاتورة شهر يناير"))
            .expect("store text");

        // The stored text uses a ta marbuta; the query uses a ha. Both fold to
        // the same token, so the document is found.
        let hits = store.search_document_text("فاتوره", 10).expect("search");
        assert_eq!(hits.len(), 1, "the folded query must match");

        // An extra token the document does not contain excludes it.
        let hits = store
            .search_document_text("فاتوره ديسمبر", 10)
            .expect("search");
        assert!(hits.is_empty(), "all tokens are required: {hits:?}");
    }

    #[test]
    fn re_storing_a_path_replaces_its_text_and_an_empty_body_removes_it() {
        let mut store = store();
        store
            .upsert_document_text(&doc("/docs/a.pdf", "alpha"))
            .expect("store");
        store
            .upsert_document_text(&doc("/docs/a.pdf", "beta"))
            .expect("replace");

        assert_eq!(store.document_text_count().expect("count"), 1);
        assert!(
            store
                .search_document_text("alpha", 5)
                .expect("search")
                .is_empty(),
            "the old text must not linger in the token table"
        );
        assert_eq!(
            store.search_document_text("beta", 5).expect("search").len(),
            1
        );

        store
            .upsert_document_text(&doc("/docs/a.pdf", "   "))
            .expect("blank removes");
        assert_eq!(store.document_text_count().expect("count"), 0);
    }

    #[test]
    fn the_switch_defaults_on_and_survives_a_write() {
        let mut store = store();
        assert!(store.document_indexing_enabled().expect("default"));
        store
            .set_document_indexing_enabled(false)
            .expect("turn off");
        assert!(!store.document_indexing_enabled().expect("read back"));
    }

    /// The mtime the way the command records it, so the test and the writer
    /// cannot drift on precision.
    fn mtime(path: &std::path::Path) -> i64 {
        std::fs::metadata(path)
            .expect("metadata")
            .modified()
            .expect("mtime")
            .duration_since(std::time::UNIX_EPOCH)
            .expect("after epoch")
            .as_secs() as i64
    }

    /// Pin a file's mtime so a rewrite is guaranteed to look like a change: two
    /// writes inside the same second would otherwise compare equal.
    fn set_mtime(path: &std::path::Path, seconds: u64) {
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .expect("open");
        file.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(seconds))
            .expect("set mtime");
    }

    fn indexed(path: &std::path::Path, body: &str, modified: Option<i64>) -> DocumentText {
        DocumentText {
            path: path.to_string_lossy().into_owned(),
            body: body.to_string(),
            page_count: 1,
            source: "text".to_string(),
            modified_at_unix_s: modified,
        }
    }

    #[test]
    fn a_row_whose_file_is_unchanged_survives_the_prune() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("report.txt");
        std::fs::write(&file, "alpha").expect("write");
        let mut store = store();
        store
            .upsert_document_text(&indexed(&file, "alpha", Some(mtime(&file))))
            .expect("store");

        let pruned = store.prune_stale_document_text().expect("prune");
        assert_eq!(pruned, PrunedDocuments::default());
        assert_eq!(store.document_text_count().expect("count"), 1);
        assert_eq!(
            store
                .search_document_text("alpha", 5)
                .expect("search")
                .len(),
            1
        );
    }

    #[test]
    fn a_row_whose_file_changed_loses_its_text_and_tokens() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("report.txt");
        std::fs::write(&file, "alpha").expect("write");
        set_mtime(&file, 1_600_000_000);
        let indexed_at = mtime(&file);
        assert_eq!(indexed_at, 1_600_000_000);
        let mut store = store();
        store
            .upsert_document_text(&indexed(&file, "alpha", Some(indexed_at)))
            .expect("store");

        // A real edit: the file on disk no longer holds the text that was read.
        std::fs::write(&file, "beta").expect("rewrite");
        set_mtime(&file, 1_600_000_500);
        let pruned = store.prune_stale_document_text().expect("prune");
        assert_eq!(pruned.changed, 1);
        assert_eq!(pruned.missing, 0);
        assert_eq!(pruned.total(), 1);
        assert_eq!(store.document_text_count().expect("count"), 0);
        assert!(
            store
                .search_document_text("alpha", 5)
                .expect("search")
                .is_empty(),
            "the stale tokens must go with the row"
        );
    }

    #[test]
    fn a_row_whose_file_was_deleted_is_pruned() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("gone.txt");
        std::fs::write(&file, "alpha").expect("write");
        let mut store = store();
        store
            .upsert_document_text(&indexed(&file, "alpha", Some(mtime(&file))))
            .expect("store");

        std::fs::remove_file(&file).expect("remove");
        let pruned = store.prune_stale_document_text().expect("prune");
        assert_eq!(pruned.missing, 1);
        assert_eq!(pruned.changed, 0);
        assert_eq!(store.document_text_count().expect("count"), 0);
    }

    #[test]
    fn a_row_without_a_recorded_mtime_is_kept() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("unknown.txt");
        std::fs::write(&file, "alpha").expect("write");
        let mut store = store();
        store
            .upsert_document_text(&indexed(&file, "alpha", None))
            .expect("store");

        std::fs::remove_file(&file).expect("remove");
        let pruned = store.prune_stale_document_text().expect("prune");
        assert_eq!(pruned, PrunedDocuments::default());
        assert_eq!(
            store.document_text_count().expect("count"),
            1,
            "without a timestamp there is nothing to compare"
        );
    }
}
