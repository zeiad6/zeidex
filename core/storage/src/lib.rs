use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use std::collections::{HashMap, HashSet};
use std::fmt::{Display, Formatter};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use zeidex_indexing::{Candidate, CandidateKind};

mod secrets;

const SEARCH_ENGINE_DUCKDUCKGO: &str = "duckduckgo";
const SEARCH_ENGINE_GOOGLE: &str = "google";
const SEARCH_ENGINE_BING: &str = "bing";
const MAX_CANDIDATE_PREALLOC: usize = 10_000;

/// Hard cap on `url_history` rows. Retention is capacity-bound: after each write
/// the least-recently-used rows beyond this many are pruned, so the table size
/// never depends on how many distinct URLs the user has opened (see url-history
/// spec). Kept modest - history is a convenience cache, not a system of record.
const MAX_URL_HISTORY_ROWS: usize = 500;
const JOURNAL_SIZE_LIMIT_BYTES: i64 = 4 * 1024 * 1024;
/// A recall corpus, not a paste ring - but still bounded: this is text the
/// user copied.
pub const MAX_CLIPBOARD_ROWS: usize = 5_000;
/// Capped far lower than text, and separately: each row is megabytes on disk,
/// and a screenshot burst must not evict text under a shared cap.
pub const MAX_CLIPBOARD_IMAGE_ROWS: usize = 50;
/// Documented ceiling for `clipboard_retention_days` (ten years). The config
/// parser rejects anything above it, so a typo cannot become "delete everything
/// older than 100,000 days"; the setter clamps for programmatic callers, which
/// also keeps the cutoff arithmetic clear of overflow.
pub const MAX_CLIPBOARD_RETENTION_DAYS: u32 = 3_650;
/// A single clip larger than this is not stored at all.
///
/// A 40 MB log pasted into a text editor is still on the pasteboard, and
/// recording it would write 40 MB into a history database the user thinks of as
/// small, hash it on every poll, and hand it to the renderer on every list.
/// 256 KiB holds ordinary prose many times over (about 130k Arabic characters),
/// so the cap only ever rejects machine-sized payloads. Truncating instead would
/// corrupt the clip; dropping it leaves the real pasteboard untouched.
pub const MAX_CLIPBOARD_ENTRY_BYTES: usize = 256 * 1024;
/// Lower bound for a user-chosen history size: fewer than this is a paste ring,
/// not a history the launcher can search.
const MIN_CLIPBOARD_ROWS: usize = 10;

pub const CLIPBOARD_KIND_TEXT: &str = "text";
pub const CLIPBOARD_KIND_IMAGE: &str = "image";

const SETTINGS_KEY_WEB_SEARCH_ENABLED: &str = "web_search_enabled";
const SETTINGS_KEY_WEB_SEARCH_ENGINE: &str = "web_search_engine";
const SETTINGS_KEY_CLIPBOARD_PAUSED: &str = "clipboard_paused";
const SETTINGS_TRUE: &str = "true";
const SETTINGS_FALSE: &str = "false";

/// Number of rows in the demo seed inserted by `QueryEngine::demo_candidates`.
/// `is_demo_seeded` uses this as an upper bound: anything larger is real data.
/// Keep in sync with `core/engine/src/lib.rs::demo_candidates`.
const DEMO_SEED_ROW_COUNT: i64 = 6;

/// Marker ids the demo seed always contains. `is_demo_seeded` treats the
/// database as a pristine demo only if BOTH are present, so a small real
/// DB that happens to contain one isn't wiped on the next scoped refresh.
const DEMO_MARKER_SAFARI: &str = "app:safari";
const DEMO_MARKER_VSCODE: &str = "app:vscode";

#[derive(Default)]
pub struct InMemorySettingsStore {
    values: HashMap<String, String>,
}

#[derive(Debug)]
pub enum StorageError {
    Io(std::io::Error),
    Sql(rusqlite::Error),
    Data(String),
}

impl Display for StorageError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageError::Io(err) => write!(f, "io error: {err}"),
            StorageError::Sql(err) => write!(f, "sqlite error: {err}"),
            StorageError::Data(err) => write!(f, "data error: {err}"),
        }
    }
}

impl std::error::Error for StorageError {}

impl StorageError {
    /// True only for SQLite's "this file is not a database" family, which is
    /// the case where the file itself is unusable rather than merely busy.
    fn is_corruption(&self) -> bool {
        use rusqlite::ErrorCode;
        matches!(
            self,
            StorageError::Sql(rusqlite::Error::SqliteFailure(err, _))
                if err.code == ErrorCode::DatabaseCorrupt || err.code == ErrorCode::NotADatabase
        )
    }
}

/// `notes.db` + `-wal` → `notes.db-wal`, built on the OS string so a non-UTF-8
/// path is not mangled by `to_string_lossy`.
fn sibling_with_suffix(path: &Path, suffix: &str) -> std::path::PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    std::path::PathBuf::from(name)
}

/// Deletes the rows older than the `cap` newest unpinned rows of `kind` and
/// returns how many went away.
///
/// A single statement, so it is atomic on its own: used both inside the copy
/// transaction and by `set_clipboard_text_cap`, where there is no transaction to
/// join. Fewer than `cap` matching rows means the subquery is empty, the row
/// comparison yields NULL, and nothing is deleted - which is the right answer.
fn prune_clipboard(conn: &Connection, kind: &str, cap: usize) -> StorageResult<usize> {
    Ok(conn.execute(
        "DELETE FROM clipboard_entries
         WHERE kind = ?1 AND pinned = 0 AND (copied_at_unix_s, id) < (
           SELECT copied_at_unix_s, id FROM clipboard_entries
           WHERE kind = ?1 AND pinned = 0
           ORDER BY copied_at_unix_s DESC, id DESC
           LIMIT 1 OFFSET ?2
         )",
        params![kind, cap.saturating_sub(1) as i64],
    )?)
}

/// Deletes unpinned text clips last copied more than `days` days ago and returns
/// how many went away. `0` means "no age limit" and deletes nothing.
///
/// Age is measured from `copied_at_unix_s` - the last copy - so re-copying a
/// clip keeps it, which is what "keep what I use" means to a user. Pinned rows
/// are exempt for the same reason as in [`prune_clipboard`].
///
/// Text only on purpose: an image row owns a file whose lifecycle the shell
/// manages, and this layer must not delete the row and leave the bytes behind.
fn prune_clipboard_by_age(conn: &Connection, days: u32, now: i64) -> StorageResult<usize> {
    if days == 0 {
        return Ok(0);
    }
    // `days` is clamped at the setter, so this product cannot overflow.
    let cutoff = now.saturating_sub(i64::from(days) * 86_400);
    Ok(conn.execute(
        "DELETE FROM clipboard_entries
         WHERE kind = ?1 AND pinned = 0 AND copied_at_unix_s < ?2",
        params![CLIPBOARD_KIND_TEXT, cutoff],
    )?)
}

impl From<std::io::Error> for StorageError {
    fn from(value: std::io::Error) -> Self {
        StorageError::Io(value)
    }
}

impl From<rusqlite::Error> for StorageError {
    fn from(value: rusqlite::Error) -> Self {
        StorageError::Sql(value)
    }
}

pub type StorageResult<T> = Result<T, StorageError>;

/// One remembered URL the user opened through the launcher (see url-history spec).
/// `title` is reserved for a future title-search signal and is `None` today.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UrlHistoryEntry {
    pub url: String,
    pub title: Option<String>,
    pub hit_count: u64,
    pub last_used_at_unix_s: i64,
}

/// One remembered clip. `content` is the full text - recall searches it.
#[derive(Debug, Clone, PartialEq)]
pub struct ClipboardEntry {
    pub id: i64,
    pub content: String,
    pub kind: String,
    /// For an image, the hash of the pixels, which also names the file.
    pub content_hash: String,
    pub app_bundle_id: Option<String>,
    /// Last time it was copied; the list is ordered by this.
    pub copied_at_unix_s: i64,
    /// First time it was copied. Rows from before this column read as their last copy.
    pub first_copied_at_unix_s: i64,
    /// How many times it was copied, re-copies included.
    pub copy_count: i64,
    /// Pinned clips list first and are never pruned.
    pub pinned: bool,
}

/// One indexed file or folder, as Batch Search reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexedPath {
    pub path: String,
    /// The file name alone, which is what a pasted list is matched against.
    pub name: String,
    pub is_folder: bool,
    pub modified_unix_s: Option<i64>,
}

pub struct SqliteStore {
    conn: Connection,
    /// How many unpinned text clips are kept. The user's
    /// `clipboard_history_limit` lands here; the hard ceiling is
    /// `MAX_CLIPBOARD_ROWS` so a bad config cannot grow the table without bound.
    clipboard_text_cap: usize,
    /// Whether clips that look like credentials are refused. Default on: see
    /// `secrets::looks_like_secret` for the (narrow) patterns.
    clipboard_filter_secrets: bool,
    /// Delete unpinned text clips older than this many days. `0` (the default)
    /// means no age limit, so the upgrade path keeps everything the size cap
    /// already keeps.
    clipboard_retention_days: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchEngine {
    DuckDuckGo,
    Google,
    Bing,
}

impl SearchEngine {
    /// The stored spelling of this engine. Round-trips through `from_key`.
    pub fn key(self) -> &'static str {
        match self {
            SearchEngine::DuckDuckGo => SEARCH_ENGINE_DUCKDUCKGO,
            SearchEngine::Google => SEARCH_ENGINE_GOOGLE,
            SearchEngine::Bing => SEARCH_ENGINE_BING,
        }
    }

    /// Every engine, in the order the settings UI lists them.
    pub fn all() -> [SearchEngine; 3] {
        [
            SearchEngine::Google,
            SearchEngine::DuckDuckGo,
            SearchEngine::Bing,
        ]
    }

    /// The engine's name as a person writes it. `key` is the storage form; this
    /// is the one that goes in a result row.
    pub fn label(self) -> &'static str {
        match self {
            SearchEngine::DuckDuckGo => "DuckDuckGo",
            SearchEngine::Google => "Google",
            SearchEngine::Bing => "Bing",
        }
    }

    /// Parses a stored key, or `None` for anything this build does not know.
    ///
    /// `from_key` deliberately falls back to Google so a config written by a
    /// newer build still boots; a settings write must not go through that
    /// fallback, because a typo would be saved as a working-looking value the
    /// user never chose.
    pub fn from_key_checked(value: &str) -> Option<Self> {
        match value {
            SEARCH_ENGINE_DUCKDUCKGO => Some(SearchEngine::DuckDuckGo),
            SEARCH_ENGINE_GOOGLE => Some(SearchEngine::Google),
            SEARCH_ENGINE_BING => Some(SearchEngine::Bing),
            _ => None,
        }
    }

    pub fn from_key(value: &str) -> Self {
        Self::from_key_checked(value).unwrap_or(SearchEngine::Google)
    }

    pub fn build_search_url(self, query: &str) -> String {
        let encoded = form_encode_query(query);
        match self {
            SearchEngine::DuckDuckGo => format!("https://duckduckgo.com/?q={encoded}"),
            SearchEngine::Google => format!("https://www.google.com/search?q={encoded}"),
            SearchEngine::Bing => format!("https://www.bing.com/search?q={encoded}"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchSettings {
    pub web_search_enabled: bool,
    pub web_search_engine: SearchEngine,
}

impl Default for SearchSettings {
    fn default() -> Self {
        Self {
            web_search_enabled: true,
            web_search_engine: SearchEngine::Google,
        }
    }
}

impl InMemorySettingsStore {
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.values.insert(key.into(), value.into());
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    pub fn search_settings(&self) -> SearchSettings {
        let enabled = self
            .get(SETTINGS_KEY_WEB_SEARCH_ENABLED)
            .map(|value| value == SETTINGS_TRUE)
            .unwrap_or(true);
        let engine = SearchEngine::from_key(
            self.get(SETTINGS_KEY_WEB_SEARCH_ENGINE)
                .unwrap_or(SEARCH_ENGINE_GOOGLE),
        );
        SearchSettings {
            web_search_enabled: enabled,
            web_search_engine: engine,
        }
    }

    pub fn set_search_settings(&mut self, settings: SearchSettings) {
        self.set(
            SETTINGS_KEY_WEB_SEARCH_ENABLED,
            if settings.web_search_enabled {
                SETTINGS_TRUE
            } else {
                SETTINGS_FALSE
            },
        );
        self.set(
            SETTINGS_KEY_WEB_SEARCH_ENGINE,
            settings.web_search_engine.key(),
        );
    }
}

impl SqliteStore {
    /// IMMEDIATE, not the default DEFERRED. Every transaction here writes, and a
    /// deferred one that opens with a read has to upgrade later: SQLite refuses
    /// that with `SQLITE_BUSY` at once, ignoring `busy_timeout`, because waiting
    /// cannot make a stale read snapshot current.
    fn write_tx(&mut self) -> StorageResult<rusqlite::Transaction<'_>> {
        Ok(self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?)
    }

    /// The same IMMEDIATE transaction for the write paths that only hold a
    /// shared borrow. `record_usage_event` and `record_url_hit` must not open
    /// DEFERRED: the URL one reads the rows it is about to prune, and a stale
    /// read snapshot cannot be upgraded - SQLite returns `SQLITE_BUSY_SNAPSHOT`
    /// immediately instead of honouring `busy_timeout`.
    fn write_tx_shared(&self) -> StorageResult<rusqlite::Transaction<'_>> {
        Ok(rusqlite::Transaction::new_unchecked(
            &self.conn,
            TransactionBehavior::Immediate,
        )?)
    }

    pub fn open(path: impl AsRef<Path>) -> StorageResult<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(path)?;
        let store = Self {
            conn,
            clipboard_text_cap: MAX_CLIPBOARD_ROWS,
            clipboard_filter_secrets: true,
            clipboard_retention_days: 0,
        };
        store.migrate()?;
        Ok(store)
    }

    /// Sets how many unpinned text clips are kept, clamped to a range that keeps
    /// the prune query sane (`10..=MAX_CLIPBOARD_ROWS`).
    ///
    /// Prunes immediately and returns how many rows went away. A retention limit
    /// that only takes effect on the next copy is not a retention limit: a user
    /// who lowers it to 10 to shrink what is stored expects the old rows gone
    /// now, not after their next Ctrl+C.
    pub fn set_clipboard_text_cap(&mut self, limit: usize) -> StorageResult<usize> {
        self.clipboard_text_cap = limit.clamp(MIN_CLIPBOARD_ROWS, MAX_CLIPBOARD_ROWS);
        prune_clipboard(&self.conn, CLIPBOARD_KIND_TEXT, self.clipboard_text_cap)
    }

    /// Turns the credential filter on or off. On by default: a clipboard history
    /// is a persistent, searchable copy of everything copied, and a leaked key
    /// copy is the one entry nobody wants to find there later.
    pub fn set_clipboard_filter_secrets(&mut self, enabled: bool) {
        self.clipboard_filter_secrets = enabled;
    }

    /// Deletes unpinned text clips older than `days` days and returns how many
    /// went away. `0` disables the age limit (the default) and deletes nothing.
    ///
    /// Applied immediately for the same reason as the size cap: a retention
    /// limit that only takes effect on the next copy is not a retention limit.
    /// Values above [`MAX_CLIPBOARD_RETENTION_DAYS`] are clamped, not obeyed as
    /// written, so the cutoff stays inside the range the arithmetic can carry.
    pub fn set_clipboard_retention_days(&mut self, days: u32) -> StorageResult<usize> {
        self.clipboard_retention_days = days.min(MAX_CLIPBOARD_RETENTION_DAYS);
        prune_clipboard_by_age(&self.conn, self.clipboard_retention_days, current_unix_s()?)
    }

    /// Opens the database, moving a corrupted file aside and starting fresh.
    ///
    /// A corrupt SQLite file used to abort startup: `open` fails, the Tauri
    /// `setup` returns that error, and the window never appears. The unreadable
    /// file is renamed rather than deleted, so anything still salvageable stays
    /// on disk, and the app comes up with an empty history that rebuilds itself.
    ///
    /// Only corruption triggers the recovery. A locked or permission-denied file
    /// is a different problem — renaming would hide it instead of reporting it.
    pub fn open_or_recover(path: impl AsRef<Path>) -> StorageResult<Self> {
        let path = path.as_ref();
        match Self::open(path) {
            Ok(store) => Ok(store),
            Err(err) if err.is_corruption() => {
                let stamp = current_unix_s()?;
                // The write-ahead log and shared-memory file belong to the
                // corrupt database. Leaving them behind would let SQLite
                // replay those pages into the new file.
                for suffix in ["", "-wal", "-shm"] {
                    let from = sibling_with_suffix(path, suffix);
                    if from.exists() {
                        std::fs::rename(
                            &from,
                            sibling_with_suffix(path, &format!("{suffix}.corrupt-{stamp}")),
                        )?;
                    }
                }
                Self::open(path)
            }
            Err(err) => Err(err),
        }
    }

    pub fn open_in_memory() -> StorageResult<Self> {
        let conn = Connection::open_in_memory()?;
        let store = Self {
            conn,
            clipboard_text_cap: MAX_CLIPBOARD_ROWS,
            clipboard_filter_secrets: true,
            clipboard_retention_days: 0,
        };
        store.migrate()?;
        Ok(store)
    }

    /// Cheap check for "is the candidates table currently just the demo seed?".
    /// Used by `bootstrap_sqlite_scoped` to decide whether to wipe the table
    /// before streaming real candidates. Point lookups on the PK index; avoids
    /// deserializing every row just to inspect the marker ids.
    ///
    /// Source of truth for the marker ids and row count is
    /// `QueryEngine::demo_candidates` in `core/engine/src/lib.rs`.
    pub fn is_demo_seeded(&self) -> StorageResult<bool> {
        let total: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM candidates", [], |row| row.get(0))?;
        // Anything larger than the demo seed is definitely real data.
        if total == 0 || total > DEMO_SEED_ROW_COUNT {
            return Ok(false);
        }
        // Demo seed always includes both markers; require BOTH families
        // present so a small real DB with only one of them doesn't trip
        // the heuristic and get wiped on the next scoped refresh.
        Ok(self.has_candidate(DEMO_MARKER_SAFARI)? && self.has_candidate(DEMO_MARKER_VSCODE)?)
    }

    fn has_candidate(&self, id: &str) -> StorageResult<bool> {
        let hit: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM candidates WHERE id = ?1",
            [id],
            |row| row.get(0),
        )?;
        Ok(hit > 0)
    }

    pub fn load_candidates(&self, limit: Option<usize>) -> StorageResult<Vec<Candidate>> {
        // One statement, not one per branch: the column list has to stay aligned
        // with the `row.get` indices below, and two copies of it means every new
        // column is two edits that can silently disagree. SQLite reads a
        // negative LIMIT as unbounded, which is exactly "no limit".
        let mut stmt = self.conn.prepare(
            "SELECT id, kind, title, subtitle, path, use_count, last_used_at_unix_s, fs_modified_at_unix_s, icon
             FROM candidates ORDER BY title ASC LIMIT ?1",
        )?;
        let mut rows = stmt.query([limit.map_or(-1, |max| max as i64)])?;

        let mut out = match limit {
            Some(max) => Vec::with_capacity(max.min(MAX_CANDIDATE_PREALLOC)),
            None => Vec::new(),
        };
        while let Some(row) = rows.next()? {
            let kind_raw: String = row.get(1)?;
            let use_count_raw: i64 = row.get(5)?;
            out.push(Candidate {
                id: row.get::<_, String>(0)?.into_boxed_str(),
                kind: parse_kind(&kind_raw)?,
                title: row.get::<_, String>(2)?.into_boxed_str(),
                subtitle: row.get::<_, Option<String>>(3)?.map(String::into_boxed_str),
                path: row.get::<_, String>(4)?.into_boxed_str(),
                use_count: to_use_count(use_count_raw)?,
                last_used_at_unix_s: row.get(6)?,
                fs_modified_at_unix_s: row.get(7)?,
                icon: row.get::<_, Option<String>>(8)?.map(String::into_boxed_str),
            });
        }

        Ok(out)
    }

    /// Every indexed file and folder, in the narrow shape Batch Search needs.
    ///
    /// Not `load_candidates`: an app, a setting and a command row have no
    /// filesystem target, and a batch matches names against files. Loading only
    /// the four columns that are used also keeps the row count (tens of
    /// thousands) from turning into a `Candidate` per row.
    pub fn load_indexed_paths(&self) -> StorageResult<Vec<IndexedPath>> {
        let mut stmt = self.conn.prepare(
            "SELECT path, title, kind, fs_modified_at_unix_s
             FROM candidates WHERE kind IN (?1, ?2) ORDER BY title ASC",
        )?;
        let mut rows = stmt.query([CandidateKind::FILE_KEY, CandidateKind::FOLDER_KEY])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let kind_raw: String = row.get(2)?;
            out.push(IndexedPath {
                path: row.get(0)?,
                name: row.get(1)?,
                is_folder: kind_raw == CandidateKind::FOLDER_KEY,
                modified_unix_s: row.get(3)?,
            });
        }
        Ok(out)
    }

    pub fn upsert_candidates(&mut self, candidates: &[Candidate]) -> StorageResult<()> {
        self.upsert_candidates_indexed(candidates, Some(current_unix_s()?))
    }

    pub fn upsert_candidates_indexed(
        &mut self,
        candidates: &[Candidate],
        indexed_at_unix_s: Option<i64>,
    ) -> StorageResult<()> {
        let tx = self.write_tx()?;
        {
            let mut stmt = tx.prepare(
                // Write reduction: the `WHERE` skips the UPDATE entirely when
                // nothing the scan controls changed, so a reindex after one
                // download/install rewrites ~1 row instead of all ~8000, and
                // the watcher's single-path upserts cost one row each. Pairs
                // with `delete_unseen_candidates` for pruning - we can no
                // longer rely on bumping `indexed_at` for every seen row.
                // List EVERY scan-owned column here; a missing one means stale data
                // silently lingers. (use_count/last_used_at are intentionally never
                // overwritten on conflict, so they're not part of change detection.)
                "INSERT INTO candidates (id, kind, title, subtitle, path, use_count, last_used_at_unix_s, indexed_at_unix_s, fs_modified_at_unix_s, icon)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(id) DO UPDATE SET
                   kind = excluded.kind,
                   title = excluded.title,
                   subtitle = excluded.subtitle,
                     path = excluded.path,
                    indexed_at_unix_s = excluded.indexed_at_unix_s,
                    fs_modified_at_unix_s = excluded.fs_modified_at_unix_s,
                    icon = excluded.icon
                 WHERE candidates.kind <> excluded.kind
                    OR candidates.title <> excluded.title
                    OR candidates.subtitle IS NOT excluded.subtitle
                    OR candidates.path <> excluded.path
                    OR candidates.fs_modified_at_unix_s IS NOT excluded.fs_modified_at_unix_s
                    OR candidates.icon IS NOT excluded.icon",
            )?;

            for candidate in candidates {
                let use_count = from_use_count(candidate.use_count)?;
                stmt.execute(params![
                    candidate.id.as_ref(),
                    kind_key(&candidate.kind),
                    candidate.title.as_ref(),
                    candidate.subtitle.as_deref(),
                    candidate.path.as_ref(),
                    use_count,
                    candidate.last_used_at_unix_s,
                    indexed_at_unix_s,
                    candidate.fs_modified_at_unix_s,
                    candidate.icon.as_deref(),
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn replace_candidates(&mut self, candidates: &[Candidate]) -> StorageResult<()> {
        let tx = self.write_tx()?;
        tx.execute("DELETE FROM usage_events", [])?;
        tx.execute("DELETE FROM candidates", [])?;

        {
            let mut stmt = tx.prepare(
                "INSERT INTO candidates (id, kind, title, subtitle, path, use_count, last_used_at_unix_s, indexed_at_unix_s, fs_modified_at_unix_s, icon)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )?;

            for candidate in candidates {
                let use_count = from_use_count(candidate.use_count)?;
                stmt.execute(params![
                    candidate.id.as_ref(),
                    kind_key(&candidate.kind),
                    candidate.title.as_ref(),
                    candidate.subtitle.as_deref(),
                    candidate.path.as_ref(),
                    use_count,
                    candidate.last_used_at_unix_s,
                    None::<i64>,
                    candidate.fs_modified_at_unix_s,
                    candidate.icon.as_deref(),
                ])?;
            }
        }

        tx.commit()?;
        Ok(())
    }

    pub fn load_search_settings(&self) -> StorageResult<SearchSettings> {
        let mut settings = SearchSettings::default();
        let mut stmt = self.conn.prepare(&format!(
            "SELECT key, value FROM settings WHERE key IN ('{}', '{}')",
            SETTINGS_KEY_WEB_SEARCH_ENABLED, SETTINGS_KEY_WEB_SEARCH_ENGINE
        ))?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let key: String = row.get(0)?;
            let value: String = row.get(1)?;
            match key.as_str() {
                SETTINGS_KEY_WEB_SEARCH_ENABLED => {
                    settings.web_search_enabled = value == SETTINGS_TRUE
                }
                SETTINGS_KEY_WEB_SEARCH_ENGINE => {
                    settings.web_search_engine = SearchEngine::from_key(&value)
                }
                _ => {}
            }
        }

        Ok(settings)
    }

    pub fn save_search_settings(&mut self, settings: SearchSettings) -> StorageResult<()> {
        let tx = self.write_tx()?;
        tx.execute(
            "INSERT INTO settings(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![
                SETTINGS_KEY_WEB_SEARCH_ENABLED,
                if settings.web_search_enabled {
                    SETTINGS_TRUE
                } else {
                    SETTINGS_FALSE
                }
            ],
        )?;
        tx.execute(
            "INSERT INTO settings(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![
                SETTINGS_KEY_WEB_SEARCH_ENGINE,
                settings.web_search_engine.key()
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn record_usage_event(&self, candidate_id: &str, action: &str) -> StorageResult<()> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|err| StorageError::Data(format!("system time error: {err}")))?
            .as_secs() as i64;

        let tx = self.write_tx_shared()?;
        tx.execute(
            "INSERT INTO usage_events(candidate_id, action, used_at_unix_s) VALUES (?1, ?2, ?3)",
            params![candidate_id, action, now],
        )?;
        tx.execute(
            "UPDATE candidates SET use_count = use_count + 1, last_used_at_unix_s = ?2 WHERE id = ?1",
            params![candidate_id, now],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Records that `url` was opened: inserts it or bumps its hit count and
    /// last-used time, then prunes the least-recently-used rows beyond the
    /// capacity cap so the table stays bounded. Best-effort at the call site -
    /// the launcher never blocks an open on this.
    pub fn record_url_hit(&self, url: &str) -> StorageResult<()> {
        let url = url.trim();
        if url.is_empty() {
            return Ok(());
        }
        let now = current_unix_s()?;
        let tx = self.write_tx_shared()?;
        tx.execute(
            "INSERT INTO url_history(url, hit_count, last_used_at_unix_s) VALUES (?1, 1, ?2)
             ON CONFLICT(url) DO UPDATE SET
                 hit_count = hit_count + 1,
                 last_used_at_unix_s = ?2",
            params![url, now],
        )?;
        // Capacity-bound retention: keep only the most-recently-used rows.
        tx.execute(
            "DELETE FROM url_history WHERE url NOT IN (
               SELECT url FROM url_history
               ORDER BY last_used_at_unix_s DESC, url ASC
               LIMIT ?1
             )",
            params![MAX_URL_HISTORY_ROWS as i64],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Up to `limit` remembered URLs matching `query` (substring, case-insensitive
    /// over the URL), most-recently-used first. An empty query returns the most
    /// recent overall. Used to suggest previously-opened URLs while typing.
    pub fn recent_urls(&self, query: &str, limit: usize) -> StorageResult<Vec<UrlHistoryEntry>> {
        let query = query.trim();
        let limit = limit as i64;
        let map_row = |row: &rusqlite::Row| -> rusqlite::Result<UrlHistoryEntry> {
            let hit_count: i64 = row.get(2)?;
            Ok(UrlHistoryEntry {
                url: row.get(0)?,
                title: row.get(1)?,
                hit_count: hit_count.max(0) as u64,
                last_used_at_unix_s: row.get(3)?,
            })
        };

        let rows = if query.is_empty() {
            let mut stmt = self.conn.prepare(
                "SELECT url, title, hit_count, last_used_at_unix_s FROM url_history
                 ORDER BY last_used_at_unix_s DESC, url ASC LIMIT ?1",
            )?;
            let mapped = stmt.query_map(params![limit], map_row)?;
            mapped.collect::<rusqlite::Result<Vec<_>>>()?
        } else {
            let pattern = format!("%{}%", like_escape(query));
            let mut stmt = self.conn.prepare(
                "SELECT url, title, hit_count, last_used_at_unix_s FROM url_history
                 WHERE url LIKE ?1 ESCAPE '\\'
                 ORDER BY last_used_at_unix_s DESC, url ASC LIMIT ?2",
            )?;
            let mapped = stmt.query_map(params![pattern, limit], map_row)?;
            mapped.collect::<rusqlite::Result<Vec<_>>>()?
        };
        Ok(rows)
    }

    /// Whether new clips are being recorded. Off by default.
    ///
    /// A stored setting rather than a process flag: pausing has to survive a
    /// restart, or the next launch quietly starts recording again while the user
    /// still believes it is off.
    pub fn clipboard_paused(&self) -> StorageResult<bool> {
        let value: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![SETTINGS_KEY_CLIPBOARD_PAUSED],
                |row| row.get(0),
            )
            .optional()?;
        Ok(value.as_deref() == Some(SETTINGS_TRUE))
    }

    pub fn set_clipboard_paused(&self, paused: bool) -> StorageResult<()> {
        self.conn.execute(
            "INSERT INTO settings(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![
                SETTINGS_KEY_CLIPBOARD_PAUSED,
                if paused {
                    SETTINGS_TRUE
                } else {
                    SETTINGS_FALSE
                }
            ],
        )?;
        Ok(())
    }

    /// Remembers a clip and returns its row id (the handle a later delete
    /// needs). Re-copying moves the row to now instead of duplicating it;
    /// oldest-first pruning keeps the table bounded.
    ///
    /// The caller must never pass a concealed or transient clip: only the
    /// shell can read the pasteboard markers that say so. Clips larger than
    /// [`MAX_CLIPBOARD_ENTRY_BYTES`] return `Ok(None)`; they are not truncated,
    /// because a half-clip is worse than no clip.
    pub fn record_clipboard_entry(
        &self,
        content: &str,
        app_bundle_id: Option<&str>,
    ) -> StorageResult<Option<i64>> {
        // Checked before hashing, not just in `insert_clip`: hashing walks every
        // byte, and a 40 MB paste is exactly the payload the cap exists for.
        if content.len() > MAX_CLIPBOARD_ENTRY_BYTES {
            return Ok(None);
        }
        // A credential is not history. Checked here rather than at the capture
        // site so the launcher's own "record this copy" path is covered too.
        if self.clipboard_filter_secrets && secrets::looks_like_secret(content) {
            return Ok(None);
        }
        let hash = content_hash(content);
        self.insert_clip(content, &hash, CLIPBOARD_KIND_TEXT, app_bundle_id)
    }

    /// `label` is what the row is listed and searched by; `image_hash` is the
    /// identity, so the same picture under a new name still lifts one row. The
    /// bytes are the shell's to write, under that hash.
    ///
    /// The Tauri shell ships text-only history and never calls this: a copied
    /// screenshot leaves no row and no file. The API stays because the contract
    /// is what a shell that does store images must honour (hash identity, the
    /// same de-duplication and retention), and it is tested here.
    pub fn record_clipboard_image(
        &self,
        label: &str,
        image_hash: &str,
        app_bundle_id: Option<&str>,
    ) -> StorageResult<Option<i64>> {
        if image_hash.trim().is_empty() {
            return Ok(None);
        }
        self.insert_clip(label, image_hash, CLIPBOARD_KIND_IMAGE, app_bundle_id)
    }

    /// The hash is passed in: an image hashes bytes this layer never sees.
    fn insert_clip(
        &self,
        content: &str,
        hash: &str,
        kind: &str,
        app_bundle_id: Option<&str>,
    ) -> StorageResult<Option<i64>> {
        if content.trim().is_empty() || content.len() > MAX_CLIPBOARD_ENTRY_BYTES {
            return Ok(None);
        }
        let now = current_unix_s()?;
        // IMMEDIATE, not the `unchecked_transaction` default (DEFERRED): this
        // transaction reads the row it replaces before it writes it. In WAL a
        // deferred transaction that read and then writes fails at once with
        // SQLITE_BUSY_SNAPSHOT if another connection committed in between, and
        // a bootstrap pass does open its own connection. Taking the write lock
        // up front lets `busy_timeout` wait instead of dropping the clip.
        let tx = rusqlite::Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        // Replaced, not updated: an in-place update keeps the old id, and
        // within the same second the id breaks the recency tie - so a re-copy
        // would sort below something older.
        // The row being replaced hands over its app, first copy time and copy count.
        let previous: Option<(Option<String>, i64, i64, bool)> = tx
            .query_row(
                "SELECT app_bundle_id, COALESCE(first_copied_at_unix_s, copied_at_unix_s), copy_count,
                        pinned
                 FROM clipboard_entries WHERE content_hash = ?1",
                params![hash],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        let (previous_app, first_copied, copy_count, pinned) = match previous {
            Some((app, first, count, pinned)) => (app, first, count.max(0) + 1, pinned),
            None => (None, now, 1, false),
        };
        tx.execute(
            "DELETE FROM clipboard_entries WHERE content_hash = ?1",
            params![hash],
        )?;
        let app = app_bundle_id.map(str::to_string).or(previous_app);
        tx.execute(
            "INSERT INTO clipboard_entries(
                 content, content_hash, kind, app_bundle_id, copied_at_unix_s,
                 first_copied_at_unix_s, copy_count, pinned)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                content,
                hash,
                kind,
                app,
                now,
                first_copied,
                copy_count,
                pinned
            ],
        )?;
        let row_id = tx.last_insert_rowid();
        let cap = if kind == CLIPBOARD_KIND_IMAGE {
            MAX_CLIPBOARD_IMAGE_ROWS
        } else {
            self.clipboard_text_cap
        };
        // Only the overflow: `WHERE id NOT IN (SELECT ... LIMIT cap)` would
        // scan the whole table on every copy, for nothing until the 5,001st.
        // Per kind, so the two histories cannot evict each other. Pinned
        // clips neither count toward the cap nor get pruned.
        prune_clipboard(&tx, kind, cap)?;
        // The age limit is text-only and global to that history, so it runs on
        // every insert rather than only on text copies: a copy is the moment
        // the user is waiting anyway, and the statement is index-backed.
        prune_clipboard_by_age(&tx, self.clipboard_retention_days, now)?;
        tx.commit()?;
        Ok(Some(row_id))
    }

    /// Up to `limit` clips matching `query` (case-insensitive substring over
    /// the content), newest first. An empty query returns the most recent.
    /// This is the keyword tier; semantic recall ranks the same rows later.
    pub fn clipboard_entries(
        &self,
        kind: &str,
        query: &str,
        limit: usize,
    ) -> StorageResult<Vec<ClipboardEntry>> {
        let query = query.trim();
        let limit = limit as i64;
        let map_row = |row: &rusqlite::Row| -> rusqlite::Result<ClipboardEntry> {
            Ok(ClipboardEntry {
                id: row.get(0)?,
                content: row.get(1)?,
                kind: row.get(2)?,
                content_hash: row.get(3)?,
                app_bundle_id: row.get(4)?,
                copied_at_unix_s: row.get(5)?,
                first_copied_at_unix_s: row.get(6)?,
                copy_count: row.get(7)?,
                pinned: row.get(8)?,
            })
        };
        let select = "SELECT id, content, kind, content_hash, app_bundle_id, copied_at_unix_s,
                             COALESCE(first_copied_at_unix_s, copied_at_unix_s), copy_count,
                             pinned
                      FROM clipboard_entries WHERE kind = ?1";
        let rows = if query.is_empty() {
            let mut stmt = self.conn.prepare(&format!(
                "{select} ORDER BY pinned DESC, copied_at_unix_s DESC, id DESC LIMIT ?2"
            ))?;
            let mapped = stmt.query_map(params![kind, limit], map_row)?;
            mapped.collect::<rusqlite::Result<Vec<_>>>()?
        } else {
            let pattern = format!("%{}%", like_escape(query));
            let mut stmt = self.conn.prepare(&format!(
                "{select} AND content LIKE ?2 ESCAPE '\\'
                 ORDER BY pinned DESC, copied_at_unix_s DESC, id DESC LIMIT ?3"
            ))?;
            let mapped = stmt.query_map(params![kind, pattern, limit], map_row)?;
            mapped.collect::<rusqlite::Result<Vec<_>>>()?
        };
        Ok(rows)
    }

    /// What the shell sweeps its image directory against: a file with no hash
    /// here is bytes the user believes they deleted.
    pub fn clipboard_image_hashes(&self) -> StorageResult<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT content_hash FROM clipboard_entries WHERE kind = ?1")?;
        let mapped = stmt.query_map(params![CLIPBOARD_KIND_IMAGE], |row| row.get(0))?;
        Ok(mapped.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Pins or unpins a clip. Returns whether the row exists.
    pub fn set_clipboard_pinned(&self, id: i64, pinned: bool) -> StorageResult<bool> {
        let changed = self.conn.execute(
            "UPDATE clipboard_entries SET pinned = ?2 WHERE id = ?1",
            params![id, pinned],
        )?;
        Ok(changed > 0)
    }

    pub fn delete_clipboard_entry(&self, id: i64) -> StorageResult<bool> {
        let changed = self
            .conn
            .execute("DELETE FROM clipboard_entries WHERE id = ?1", params![id])?;
        Ok(changed > 0)
    }

    /// Forget everything copied. The user-facing promise behind persisting
    /// clipboard history at all, so it must be one call, not a per-row loop.
    pub fn clear_clipboard_entries(&self) -> StorageResult<usize> {
        Ok(self.conn.execute("DELETE FROM clipboard_entries", [])?)
    }

    pub fn delete_stale_candidates(&mut self, older_than_unix_s: i64) -> StorageResult<usize> {
        let tx = self.write_tx()?;
        tx.execute(
            "DELETE FROM usage_events
             WHERE candidate_id IN (
               SELECT id FROM candidates
               WHERE indexed_at_unix_s IS NULL OR indexed_at_unix_s < ?1
             )",
            params![older_than_unix_s],
        )?;
        let removed = tx.execute(
            "DELETE FROM candidates
             WHERE indexed_at_unix_s IS NULL OR indexed_at_unix_s < ?1",
            params![older_than_unix_s],
        )?;
        tx.commit()?;
        Ok(removed)
    }

    /// Seen-set pruning, the counterpart to the change-detecting upsert
    /// (see specs/indexing-scale.md). Because the upsert no longer bumps
    /// `indexed_at` on unchanged rows, "was this seen in the latest scan?" can't
    /// be inferred from `indexed_at` anymore - we delete rows in scope that are
    /// NOT in `seen_ids`. The `indexed_at < older_than_unix_s` guard preserves the
    /// `i64::MAX` pinned/seeded rows (e.g. UWP), exactly like `delete_stale_*`.
    /// `prefixes` empty = whole table (ALL-scope refresh); otherwise restrict to
    /// rows whose id starts with one of the prefixes (scoped refresh).
    ///
    /// `seen_ids` is staged in an in-memory temp table (PRAGMA temp_store=MEMORY),
    /// so this adds no persistent writes beyond the rows it actually deletes.
    pub fn delete_unseen_candidates(
        &mut self,
        seen_ids: &HashSet<Box<str>>,
        older_than_unix_s: i64,
        prefixes: &[&str],
    ) -> StorageResult<usize> {
        let like_clause = if prefixes.is_empty() {
            String::new()
        } else {
            let ors = (0..prefixes.len())
                .map(|i| format!("id LIKE ?{} ESCAPE '\\'", i + 2))
                .collect::<Vec<_>>()
                .join(" OR ");
            format!(" AND ({ors})")
        };
        let escaped: Vec<String> = prefixes
            .iter()
            .map(|p| format!("{}%", like_escape(p)))
            .collect();

        let tx = self.write_tx()?;
        tx.execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS _seen_ids (id TEXT PRIMARY KEY);
             DELETE FROM _seen_ids;",
        )?;
        {
            let mut stmt = tx.prepare("INSERT OR IGNORE INTO _seen_ids (id) VALUES (?1)")?;
            for id in seen_ids {
                stmt.execute(params![id.as_ref()])?;
            }
        }

        let mut bindings: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(1 + escaped.len());
        bindings.push(&older_than_unix_s);
        for s in &escaped {
            bindings.push(s);
        }

        let where_unseen = format!(
            "(indexed_at_unix_s IS NULL OR indexed_at_unix_s < ?1)
             AND id NOT IN (SELECT id FROM _seen_ids){like_clause}"
        );

        tx.execute(
            &format!(
                "DELETE FROM usage_events WHERE candidate_id IN (
                   SELECT id FROM candidates WHERE {where_unseen}
                 )"
            ),
            bindings.as_slice(),
        )?;
        let removed = tx.execute(
            &format!("DELETE FROM candidates WHERE {where_unseen}"),
            bindings.as_slice(),
        )?;
        tx.commit()?;
        Ok(removed)
    }

    /// Every `(id, path)` pair in the candidate table.
    ///
    /// The watcher needs this to turn a removed filesystem path into candidate
    /// ids. Path containment is decided in Rust (`path_is_same_or_child`)
    /// instead of SQL, so the one tested implementation of separator, trailing
    /// slash and Windows case rules decides what "under this folder" means; a
    /// `LIKE` clause would be a second, subtly different one.
    pub fn candidate_paths(&self) -> StorageResult<Vec<(String, String)>> {
        let mut stmt = self.conn.prepare("SELECT id, path FROM candidates")?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Deletes candidates by id, and their usage history with them.
    ///
    /// One transaction: the `usage_events.candidate_id` foreign key would make
    /// the candidate delete fail if a usage row survived it. Used by the
    /// incremental watcher refresh, where the exact ids are already known and a
    /// full `seen` set is not needed.
    pub fn delete_candidates_by_ids(&mut self, ids: &[String]) -> StorageResult<usize> {
        if ids.is_empty() {
            return Ok(0);
        }
        let tx = self.write_tx()?;
        tx.execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS _dead_ids (id TEXT PRIMARY KEY);
             DELETE FROM _dead_ids;",
        )?;
        {
            let mut stmt = tx.prepare("INSERT OR IGNORE INTO _dead_ids (id) VALUES (?1)")?;
            for id in ids {
                stmt.execute(params![id])?;
            }
        }
        tx.execute(
            "DELETE FROM usage_events WHERE candidate_id IN (SELECT id FROM _dead_ids)",
            [],
        )?;
        let removed = tx.execute(
            "DELETE FROM candidates WHERE id IN (SELECT id FROM _dead_ids)",
            [],
        )?;
        tx.commit()?;
        Ok(removed)
    }

    /// Scoped variant of `delete_stale_candidates`: prunes only rows whose id begins
    /// with one of `prefixes`. Used by partial reindex paths that re-walk a subset
    /// of sources (e.g. apps-only) and must not prune candidates from sources that
    /// were not part of this run.
    pub fn delete_stale_candidates_with_prefixes(
        &mut self,
        older_than_unix_s: i64,
        prefixes: &[&str],
    ) -> StorageResult<usize> {
        if prefixes.is_empty() {
            return Ok(0);
        }
        let escaped: Vec<String> = prefixes
            .iter()
            .map(|p| format!("{}%", like_escape(p)))
            .collect();
        let like_clause = (0..escaped.len())
            .map(|i| format!("id LIKE ?{} ESCAPE '\\'", i + 2))
            .collect::<Vec<_>>()
            .join(" OR ");

        let tx = self.write_tx()?;

        let mut bindings: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(1 + escaped.len());
        bindings.push(&older_than_unix_s);
        for s in &escaped {
            bindings.push(s);
        }

        let del_usage_sql = format!(
            "DELETE FROM usage_events
             WHERE candidate_id IN (
               SELECT id FROM candidates
               WHERE (indexed_at_unix_s IS NULL OR indexed_at_unix_s < ?1)
                 AND ({like_clause})
             )"
        );
        tx.execute(&del_usage_sql, bindings.as_slice())?;

        let del_cand_sql = format!(
            "DELETE FROM candidates
             WHERE (indexed_at_unix_s IS NULL OR indexed_at_unix_s < ?1)
               AND ({like_clause})"
        );
        let removed = tx.execute(&del_cand_sql, bindings.as_slice())?;
        tx.commit()?;
        Ok(removed)
    }

    /// Deletes every candidate whose `id` starts with `prefix` and is NOT in `keep_ids`,
    /// along with its usage_events rows. Returned value is the number of candidate rows
    /// removed. Used by the UWP seed path (bridge/ffi/src/seed_api.rs) to age out apps
    /// that disappeared from `shell:AppsFolder` between runs - those rows can't be
    /// pruned by `delete_stale_candidates` because they're written with
    /// `indexed_at_unix_s = i64::MAX` to survive the regular index-refresh sweep.
    pub fn delete_candidates_by_prefix_except(
        &mut self,
        prefix: &str,
        keep_ids: &HashSet<&str>,
    ) -> StorageResult<usize> {
        let tx = self.write_tx()?;
        let like_pattern = format!("{}%", like_escape(prefix));

        let stale_ids: Vec<String> = {
            let mut stmt = tx.prepare("SELECT id FROM candidates WHERE id LIKE ?1 ESCAPE '\\'")?;
            let rows = stmt.query_map(params![like_pattern], |row| row.get::<_, String>(0))?;
            let mut out = Vec::new();
            for row in rows {
                let id = row?;
                if !keep_ids.contains(id.as_str()) {
                    out.push(id);
                }
            }
            out
        };

        for id in &stale_ids {
            tx.execute(
                "DELETE FROM usage_events WHERE candidate_id = ?1",
                params![id],
            )?;
            tx.execute("DELETE FROM candidates WHERE id = ?1", params![id])?;
        }
        tx.commit()?;
        Ok(stale_ids.len())
    }

    pub fn prune_usage_events_older_than(&mut self, cutoff_unix_s: i64) -> StorageResult<usize> {
        let removed = self.conn.execute(
            "DELETE FROM usage_events WHERE used_at_unix_s < ?1",
            params![cutoff_unix_s],
        )?;
        Ok(removed)
    }

    pub fn prune_usage_events_to_max(&mut self, max_rows: usize) -> StorageResult<usize> {
        let total: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM usage_events", [], |row| row.get(0))?;
        let total = total.max(0) as usize;
        if total <= max_rows {
            return Ok(0);
        }

        let overflow = total - max_rows;
        let removed = self.conn.execute(
            "DELETE FROM usage_events
             WHERE id IN (
               SELECT id FROM usage_events ORDER BY used_at_unix_s ASC, id ASC LIMIT ?1
             )",
            params![overflow as i64],
        )?;
        Ok(removed)
    }

    fn migrate(&self) -> StorageResult<()> {
        self.conn
            .pragma_update(None, "journal_size_limit", JOURNAL_SIZE_LIMIT_BYTES)?;
        self.conn.execute_batch(
            // No `busy_timeout`: rusqlite already sets 5s on every connection
            // it opens, before any of this runs.
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             -- Keep the seen-id temp table used by delete_unseen_candidates in RAM.
             PRAGMA temp_store = MEMORY;

             CREATE TABLE IF NOT EXISTS settings (
                 key TEXT PRIMARY KEY,
                 value TEXT NOT NULL
             );

             CREATE TABLE IF NOT EXISTS candidates (
                 id TEXT PRIMARY KEY,
                 kind TEXT NOT NULL,
                 title TEXT NOT NULL,
                 subtitle TEXT,
                 path TEXT NOT NULL,
                 use_count INTEGER NOT NULL DEFAULT 0,
                 last_used_at_unix_s INTEGER,
                 indexed_at_unix_s INTEGER
              );

             CREATE INDEX IF NOT EXISTS idx_candidates_title ON candidates(title);
             CREATE INDEX IF NOT EXISTS idx_candidates_kind ON candidates(kind);

             CREATE TABLE IF NOT EXISTS usage_events (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 candidate_id TEXT NOT NULL,
                 action TEXT NOT NULL,
                 used_at_unix_s INTEGER NOT NULL,
                 FOREIGN KEY(candidate_id) REFERENCES candidates(id)
             );

              CREATE TABLE IF NOT EXISTS index_state (
                  source TEXT PRIMARY KEY,
                  last_indexed_at_unix_s INTEGER NOT NULL
              );

              -- URLs opened through the launcher, for fast re-open later. Keyed
              -- on the resolved/normalized URL so github.com and https://github.com
              -- collapse to one row. Independent of `candidates` (never touched by
              -- reindex prune); capacity-bound via MAX_URL_HISTORY_ROWS.
              CREATE TABLE IF NOT EXISTS url_history (
                  url                 TEXT PRIMARY KEY,
                  title               TEXT,
                  hit_count           INTEGER NOT NULL DEFAULT 0,
                  last_used_at_unix_s INTEGER NOT NULL
              );

              CREATE INDEX IF NOT EXISTS idx_url_history_last_used
                  ON url_history(last_used_at_unix_s);

              -- Clipboard history, persisted so it survives a restart and can
              -- later be searched semantically (Recall, see docs/ai-vision.md).
              -- `content_hash` dedupes re-copying the same text: the row moves
              -- to the top instead of piling up. Concealed/transient clips are
              -- never written - that gate is at the capture site, since only
              -- the shell can see the pasteboard's type markers.
              CREATE TABLE IF NOT EXISTS clipboard_entries (
                  id                INTEGER PRIMARY KEY AUTOINCREMENT,
                  content           TEXT NOT NULL,
                  content_hash      TEXT NOT NULL UNIQUE,
                  kind              TEXT NOT NULL DEFAULT 'text',
                  app_bundle_id     TEXT,
                  copied_at_unix_s  INTEGER NOT NULL
              );

              CREATE INDEX IF NOT EXISTS idx_clipboard_copied_at
                  ON clipboard_entries(copied_at_unix_s);",
        )?;

        ensure_column_exists(&self.conn, "candidates", "indexed_at_unix_s", "INTEGER")?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_candidates_indexed_at ON candidates(indexed_at_unix_s)",
            [],
        )?;

        // Filesystem mtime, captured at index time, powers the recent view's
        // "newly added/changed" signal. Additive column on existing DBs.
        ensure_column_exists(&self.conn, "candidates", "fs_modified_at_unix_s", "INTEGER")?;

        // What a declared source row asked to be drawn as. Only source rows set
        // it, so it is NULL for almost every row. Additive on existing DBs.
        ensure_column_exists(&self.conn, "candidates", "icon", "TEXT")?;

        // Clip details shown in the clipboard preview. Additive on existing DBs:
        // old rows have no first copy time (read as their last) and count as one copy.
        ensure_column_exists(
            &self.conn,
            "clipboard_entries",
            "first_copied_at_unix_s",
            "INTEGER",
        )?;
        ensure_column_exists(
            &self.conn,
            "clipboard_entries",
            "copy_count",
            "INTEGER NOT NULL DEFAULT 1",
        )?;
        ensure_column_exists(
            &self.conn,
            "clipboard_entries",
            "pinned",
            "INTEGER NOT NULL DEFAULT 0",
        )?;

        Ok(())
    }
}

fn ensure_column_exists(
    conn: &Connection,
    table: &str,
    column: &str,
    column_type: &str,
) -> StorageResult<()> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name == column {
            return Ok(());
        }
    }

    conn.execute(
        &format!("ALTER TABLE {table} ADD COLUMN {column} {column_type}"),
        [],
    )?;
    Ok(())
}

/// Stable dedupe id. FNV-1a over the trimmed text - stable and cheap, not
/// cryptographic.
fn content_hash(content: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in content.trim().as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    format!("{hash:016x}")
}

/// Escapes LIKE metacharacters so a typed `%` or `_` is literal. Every LIKE
/// query goes through this; three hand-written copies had forgotten `_`.
fn like_escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn current_unix_s() -> StorageResult<i64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| StorageError::Data(format!("system time error: {err}")))
        .map(|d| d.as_secs() as i64)
}

fn to_use_count(value: i64) -> StorageResult<u64> {
    u64::try_from(value)
        .map_err(|_| StorageError::Data(format!("invalid use_count in sqlite: {value}")))
}

fn from_use_count(value: u64) -> StorageResult<i64> {
    i64::try_from(value)
        .map_err(|_| StorageError::Data(format!("use_count overflow for sqlite: {value}")))
}

/// RFC 3986 percent-encoding: unreserved characters are passed through,
/// everything else (including spaces) is encoded as `%XX`.
const HEX_CHARS: &[u8; 16] = b"0123456789ABCDEF";

pub fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push(HEX_CHARS[(byte >> 4) as usize] as char);
            out.push(HEX_CHARS[(byte & 0x0F) as usize] as char);
        }
    }
    out
}

/// Form-style encoding for search query parameters: same as [`percent_encode`]
/// but encodes spaces as `+` instead of `%20`.
fn form_encode_query(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else if byte == b' ' {
            out.push('+');
        } else {
            out.push('%');
            out.push(HEX_CHARS[(byte >> 4) as usize] as char);
            out.push(HEX_CHARS[(byte & 0x0F) as usize] as char);
        }
    }
    out
}

fn kind_key(kind: &CandidateKind) -> &'static str {
    kind.as_str()
}

fn parse_kind(value: &str) -> StorageResult<CandidateKind> {
    CandidateKind::from_key(value)
        .ok_or_else(|| StorageError::Data(format!("unknown candidate kind: {value}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Clipboard history is persisted BECAUSE it is meant to outlive the app,
    /// so the properties that make it a corpus rather than a paste ring are
    /// pinned here: dedupe, recency order, bounded growth, and a real wipe.
    #[test]
    fn clipboard_history_dedupes_orders_and_stays_bounded() {
        let store = SqliteStore::open_in_memory().unwrap();
        store
            .record_clipboard_entry("first clip", Some("com.apple.Safari"))
            .unwrap();
        store.record_clipboard_entry("second clip", None).unwrap();
        // Re-copying moves the row to the top instead of adding a duplicate.
        store.record_clipboard_entry("first clip", None).unwrap();

        let all = store
            .clipboard_entries(CLIPBOARD_KIND_TEXT, "", 10)
            .unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].content, "first clip");
        // The source app is kept from the first capture rather than nulled.
        assert_eq!(all[0].app_bundle_id.as_deref(), Some("com.apple.Safari"));
        // Re-copying counts, and keeps the time of the first copy.
        assert_eq!(all[0].copy_count, 2);
        assert!(all[0].first_copied_at_unix_s <= all[0].copied_at_unix_s);
        assert_eq!(all[1].copy_count, 1);
        assert_eq!(all[1].first_copied_at_unix_s, all[1].copied_at_unix_s);

        // Substring search, case-insensitive, newest first.
        let found = store
            .clipboard_entries(CLIPBOARD_KIND_TEXT, "SECOND", 10)
            .unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].content, "second clip");
        assert!(
            store
                .clipboard_entries(CLIPBOARD_KIND_TEXT, "nothing here", 10)
                .unwrap()
                .is_empty()
        );

        // Empty and whitespace-only clips are not worth remembering.
        store.record_clipboard_entry("   ", None).unwrap();
        assert_eq!(
            store
                .clipboard_entries(CLIPBOARD_KIND_TEXT, "", 10)
                .unwrap()
                .len(),
            2
        );

        // The row id comes back from record, so a freshly captured clip has a
        // handle to delete by - without it a "deleted" clip returns on the
        // next launch.
        let fresh = store
            .record_clipboard_entry("third clip", None)
            .unwrap()
            .expect("row id");
        assert!(store.delete_clipboard_entry(fresh).unwrap());
        assert!(
            store
                .clipboard_entries(CLIPBOARD_KIND_TEXT, "third", 10)
                .unwrap()
                .is_empty()
        );
        // Nothing stored means no id to hand back.
        assert_eq!(store.record_clipboard_entry("  ", None).unwrap(), None);

        let id = all[0].id;
        assert!(store.delete_clipboard_entry(id).unwrap());
        assert!(!store.delete_clipboard_entry(id).unwrap());
        assert_eq!(store.clear_clipboard_entries().unwrap(), 1);
        assert!(
            store
                .clipboard_entries(CLIPBOARD_KIND_TEXT, "", 10)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn an_oversized_clip_is_not_stored() {
        let store = SqliteStore::open_in_memory().unwrap();
        // One byte over the ceiling: nothing is written, and nothing is
        // truncated into a half-clip.
        let huge = "x".repeat(MAX_CLIPBOARD_ENTRY_BYTES + 1);
        assert_eq!(store.record_clipboard_entry(&huge, None).unwrap(), None);
        assert!(
            store
                .clipboard_entries(CLIPBOARD_KIND_TEXT, "", 10)
                .unwrap()
                .is_empty()
        );

        // The ceiling itself is a normal clip.
        let at_limit = "y".repeat(MAX_CLIPBOARD_ENTRY_BYTES);
        assert!(
            store
                .record_clipboard_entry(&at_limit, None)
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn the_configured_history_limit_caps_the_table() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        store.set_clipboard_text_cap(10).unwrap();
        for i in 0..25 {
            store
                .record_clipboard_entry(&format!("clip {i:02}"), None)
                .unwrap();
        }

        let all = store
            .clipboard_entries(CLIPBOARD_KIND_TEXT, "", 100)
            .unwrap();
        assert_eq!(all.len(), 10);
        assert_eq!(all[0].content, "clip 24");
        assert_eq!(all[9].content, "clip 15");

        // A nonsensical value is clamped, never treated as "keep nothing".
        store.set_clipboard_text_cap(0).unwrap();
        for i in 0..12 {
            store
                .record_clipboard_entry(&format!("extra {i}"), None)
                .unwrap();
        }
        assert_eq!(
            store
                .clipboard_entries(CLIPBOARD_KIND_TEXT, "", 100)
                .unwrap()
                .len(),
            MIN_CLIPBOARD_ROWS
        );
    }

    /// A retention limit has to apply to what is already stored. Leaving 5,000
    /// rows in place until the next copy is not a limit the user can rely on.
    #[test]
    fn lowering_the_history_limit_prunes_immediately() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        for i in 0..30 {
            store
                .record_clipboard_entry(&format!("clip {i:02}"), None)
                .unwrap();
        }
        assert_eq!(
            store
                .clipboard_entries(CLIPBOARD_KIND_TEXT, "", 100)
                .unwrap()
                .len(),
            30
        );

        let removed = store.set_clipboard_text_cap(10).unwrap();
        assert_eq!(removed, 20);
        let kept = store
            .clipboard_entries(CLIPBOARD_KIND_TEXT, "", 100)
            .unwrap();
        assert_eq!(kept.len(), 10);
        assert_eq!(kept[0].content, "clip 29");
        assert_eq!(kept[9].content, "clip 20");

        // Lowering again with nothing to prune is a no-op, not an error.
        assert_eq!(store.set_clipboard_text_cap(10).unwrap(), 0);
    }

    /// The age limit is a privacy control, so the two things that must never
    /// happen are: deleting a pinned clip, and deleting anything while the
    /// feature is off (its default, and what every existing install has).
    #[test]
    fn clipboard_age_retention_deletes_only_old_unpinned_text() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        store.record_clipboard_entry("old clip", None).unwrap();
        store.record_clipboard_entry("fresh clip", None).unwrap();
        let pinned_id = store
            .record_clipboard_entry("pinned old clip", None)
            .unwrap()
            .expect("clip id");
        store
            .record_clipboard_image("screenshot", "hash-old-image", None)
            .unwrap();
        assert!(store.set_clipboard_pinned(pinned_id, true).unwrap());

        let now = current_unix_s().unwrap();
        let old = now - 30 * 86_400;
        store
            .conn
            .execute(
                "UPDATE clipboard_entries SET copied_at_unix_s = ?1
                 WHERE content IN ('old clip', 'pinned old clip', 'screenshot')",
                params![old],
            )
            .unwrap();

        // Off is the default: it must delete nothing even when old rows exist.
        assert_eq!(store.set_clipboard_retention_days(0).unwrap(), 0);
        assert_eq!(
            store
                .clipboard_entries(CLIPBOARD_KIND_TEXT, "", 10)
                .unwrap()
                .len(),
            3
        );

        assert_eq!(store.set_clipboard_retention_days(7).unwrap(), 1);
        let texts: Vec<String> = store
            .clipboard_entries(CLIPBOARD_KIND_TEXT, "", 10)
            .unwrap()
            .into_iter()
            .map(|entry| entry.content)
            .collect();
        assert!(texts.iter().any(|text| text == "fresh clip"));
        assert!(
            texts.iter().any(|text| text == "pinned old clip"),
            "a pinned clip is never deleted by age"
        );
        assert!(!texts.iter().any(|text| text == "old clip"));

        // An image row owns a file this layer does not manage. Deleting the row
        // would leave the bytes behind with nothing pointing at them.
        assert_eq!(
            store
                .clipboard_entries(CLIPBOARD_KIND_IMAGE, "", 10)
                .unwrap()
                .len(),
            1
        );
    }

    /// "Older than N days" is measured from the last copy, so using a clip
    /// again keeps it. Otherwise retention would delete the clips the user
    /// reaches for most.
    #[test]
    fn recopying_a_clip_resets_its_age() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        store.record_clipboard_entry("reused", None).unwrap();
        let old = current_unix_s().unwrap() - 30 * 86_400;
        store
            .conn
            .execute(
                "UPDATE clipboard_entries SET copied_at_unix_s = ?1",
                params![old],
            )
            .unwrap();

        store.record_clipboard_entry("reused", None).unwrap();
        assert_eq!(store.set_clipboard_retention_days(7).unwrap(), 0);
        assert_eq!(
            store
                .clipboard_entries(CLIPBOARD_KIND_TEXT, "", 10)
                .unwrap()
                .len(),
            1
        );
    }

    /// The setter is the last line of defence for the cutoff arithmetic: a
    /// caller passing `u32::MAX` must not overflow it, and the documented
    /// ceiling is what gets honoured.
    #[test]
    fn clipboard_retention_days_is_clamped_and_cannot_overflow() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        store.record_clipboard_entry("ancient", None).unwrap();
        let now = current_unix_s().unwrap();
        // A minute past the ceiling, not a second: the setter reads the clock
        // again, and the assertion must not depend on both reads landing in the
        // same second.
        let beyond_ceiling = now - (i64::from(MAX_CLIPBOARD_RETENTION_DAYS) * 86_400) - 60;
        store
            .conn
            .execute(
                "UPDATE clipboard_entries SET copied_at_unix_s = ?1",
                params![beyond_ceiling],
            )
            .unwrap();

        assert_eq!(store.set_clipboard_retention_days(u32::MAX).unwrap(), 1);
        assert_eq!(
            store
                .clipboard_entries(CLIPBOARD_KIND_TEXT, "", 10)
                .unwrap()
                .len(),
            0
        );
    }

    /// Credentials must never become searchable history. Ordinary Arabic text
    /// (including digits) must still be stored.
    #[test]
    fn credential_like_clips_are_refused_by_default() {
        let mut store = SqliteStore::open_in_memory().unwrap();
        assert!(
            store
                .record_clipboard_entry(
                    "-----BEGIN RSA PRIVATE KEY-----\nMIIEow\n-----END RSA PRIVATE KEY-----",
                    None
                )
                .unwrap()
                .is_none()
        );
        assert!(
            store
                .record_clipboard_entry("AKIAIOSFODNN7EXAMPLE", None)
                .unwrap()
                .is_none()
        );
        assert_eq!(store.clear_clipboard_entries().unwrap(), 0);

        assert!(
            store
                .record_clipboard_entry("فاتورة ٢٠٢٦ بمبلغ 1250 ريالاً", None)
                .unwrap()
                .is_some()
        );

        // The filter is a policy, not a hard rule: turning it off restores the
        // previous behaviour for users who want the key in their history.
        store.set_clipboard_filter_secrets(false);
        assert!(
            store
                .record_clipboard_entry("AKIAIOSFODNN7EXAMPLE", None)
                .unwrap()
                .is_some()
        );
    }

    /// The pause has to outlive the process: a restart must not silently resume
    /// recording behind a user who believes it is off.
    #[test]
    fn clipboard_pause_survives_a_reopen() {
        let dir = std::env::temp_dir().join(format!(
            "zeidex-pause-{}-{}",
            std::process::id(),
            current_unix_s().expect("clock")
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join("zeidex.db");

        {
            let store = SqliteStore::open(&path).expect("open store");
            assert!(!store.clipboard_paused().expect("default"));
            store.set_clipboard_paused(true).expect("pause");
            assert!(store.clipboard_paused().expect("paused"));
        }
        {
            let store = SqliteStore::open(&path).expect("reopen store");
            assert!(store.clipboard_paused().expect("still paused"));
            store.set_clipboard_paused(false).expect("resume");
            assert!(!store.clipboard_paused().expect("resumed"));
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pinned_clips_list_first_and_survive_pruning() {
        let store = SqliteStore::open_in_memory().unwrap();
        let pinned = store
            .record_clipboard_entry("keep me", None)
            .unwrap()
            .expect("row id");
        assert!(store.set_clipboard_pinned(pinned, true).unwrap());
        for i in 0..(MAX_CLIPBOARD_ROWS + 5) {
            store
                .record_clipboard_entry(&format!("clip {i}"), None)
                .unwrap();
        }
        let all = store
            .clipboard_entries(CLIPBOARD_KIND_TEXT, "", MAX_CLIPBOARD_ROWS + 100)
            .unwrap();
        assert_eq!(all.len(), MAX_CLIPBOARD_ROWS + 1);
        assert_eq!(all[0].content, "keep me");
        assert!(all[0].pinned);

        // A re-copy keeps the pin.
        let recopied = store
            .record_clipboard_entry("keep me", None)
            .unwrap()
            .unwrap();
        let first = &store.clipboard_entries(CLIPBOARD_KIND_TEXT, "", 1).unwrap()[0];
        assert_eq!(first.id, recopied);
        assert!(first.pinned);
    }

    #[test]
    fn clipboard_history_prunes_oldest_beyond_the_cap() {
        let store = SqliteStore::open_in_memory().unwrap();
        for i in 0..(MAX_CLIPBOARD_ROWS + 25) {
            store
                .record_clipboard_entry(&format!("clip {i}"), None)
                .unwrap();
        }
        let all = store
            .clipboard_entries(CLIPBOARD_KIND_TEXT, "", MAX_CLIPBOARD_ROWS + 100)
            .unwrap();
        assert_eq!(all.len(), MAX_CLIPBOARD_ROWS);
        // The oldest went, not the newest: this is a recall corpus.
        assert!(
            store
                .clipboard_entries(CLIPBOARD_KIND_TEXT, "clip 0", 10)
                .unwrap()
                .is_empty()
        );
        assert!(
            !store
                .clipboard_entries(
                    CLIPBOARD_KIND_TEXT,
                    &format!("clip {}", MAX_CLIPBOARD_ROWS + 24),
                    10
                )
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn image_clips_and_text_clips_cannot_see_each_other() {
        let store = SqliteStore::open_in_memory().unwrap();
        store.record_clipboard_entry("a note", None).unwrap();
        store
            .record_clipboard_image("Screenshot.png", "aaaa", Some("com.apple.Safari"))
            .unwrap();

        let text = store
            .clipboard_entries(CLIPBOARD_KIND_TEXT, "", 10)
            .unwrap();
        assert_eq!(text.len(), 1);
        assert_eq!(text[0].content, "a note");

        let images = store
            .clipboard_entries(CLIPBOARD_KIND_IMAGE, "", 10)
            .unwrap();
        assert_eq!(images.len(), 1);
        // The path to the bytes is rebuilt from this.
        assert_eq!(images[0].content_hash, "aaaa");
        assert_eq!(images[0].app_bundle_id.as_deref(), Some("com.apple.Safari"));

        // The label is what search matches, since the content is a name.
        assert_eq!(
            store
                .clipboard_entries(CLIPBOARD_KIND_IMAGE, "screenshot", 10)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(store.clipboard_image_hashes().unwrap(), vec!["aaaa"]);
    }

    #[test]
    fn re_copying_an_image_lifts_the_row_it_already_has() {
        let store = SqliteStore::open_in_memory().unwrap();
        store
            .record_clipboard_image("Screenshot.png", "aaaa", None)
            .unwrap();
        store
            .record_clipboard_image("Other.png", "bbbb", None)
            .unwrap();
        // Same pixels, different name: one row, back on top.
        store
            .record_clipboard_image("Screenshot copy.png", "aaaa", None)
            .unwrap();

        let images = store
            .clipboard_entries(CLIPBOARD_KIND_IMAGE, "", 10)
            .unwrap();
        assert_eq!(images.len(), 2);
        assert_eq!(images[0].content, "Screenshot copy.png");

        // No bytes, no row: the hash is the whole identity of an image clip.
        assert_eq!(
            store
                .record_clipboard_image("Empty.png", "  ", None)
                .unwrap(),
            None
        );
    }

    #[test]
    fn image_pruning_is_capped_separately_from_text() {
        let store = SqliteStore::open_in_memory().unwrap();
        store.record_clipboard_entry("a note", None).unwrap();
        for i in 0..(MAX_CLIPBOARD_IMAGE_ROWS + 5) {
            store
                .record_clipboard_image(&format!("shot {i}.png"), &format!("hash{i}"), None)
                .unwrap();
        }
        let images = store
            .clipboard_entries(CLIPBOARD_KIND_IMAGE, "", MAX_CLIPBOARD_IMAGE_ROWS + 100)
            .unwrap();
        assert_eq!(images.len(), MAX_CLIPBOARD_IMAGE_ROWS);
        // A burst of screenshots must not evict the text the user still wants.
        assert_eq!(
            store
                .clipboard_entries(CLIPBOARD_KIND_TEXT, "", 10)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn a_typed_wildcard_is_searched_literally() {
        let store = SqliteStore::open_in_memory().unwrap();
        store.record_clipboard_entry("100% done", None).unwrap();
        store
            .record_clipboard_entry("nothing special", None)
            .unwrap();
        // Without escaping, "%" would match every row.
        let found = store
            .clipboard_entries(CLIPBOARD_KIND_TEXT, "100%", 10)
            .unwrap();
        assert_eq!(found.len(), 1);
    }

    fn candidate(id: &str, title: &str, path: &str) -> Candidate {
        Candidate {
            id: id.into(),
            kind: CandidateKind::App,
            title: title.into(),
            subtitle: Some("test subtitle".into()),
            path: path.into(),
            ..Default::default()
        }
    }

    #[test]
    fn percent_encode_leaves_unreserved_chars_intact() {
        assert_eq!(percent_encode("hello"), "hello");
        assert_eq!(percent_encode("a-b_c.d~e"), "a-b_c.d~e");
        assert_eq!(percent_encode("ABC123"), "ABC123");
    }

    #[test]
    fn percent_encode_encodes_spaces_and_special_chars() {
        assert_eq!(percent_encode("hello world"), "hello%20world");
        assert_eq!(percent_encode("a&b=c"), "a%26b%3Dc");
        assert_eq!(percent_encode("foo/bar"), "foo%2Fbar");
    }

    #[test]
    fn percent_encode_handles_unicode() {
        let encoded = percent_encode("café");
        assert!(encoded.starts_with("caf%"));
        assert!(!encoded.contains('é'));
    }

    #[test]
    fn percent_encode_handles_empty_string() {
        assert_eq!(percent_encode(""), "");
    }

    #[test]
    fn form_encode_query_encodes_spaces_as_plus() {
        assert_eq!(form_encode_query("hello world"), "hello+world");
        assert_eq!(form_encode_query("a&b"), "a%26b");
    }

    #[test]
    fn open_in_memory_runs_migrations() {
        let store = SqliteStore::open_in_memory().expect("open sqlite in memory");

        let mut stmt = store
            .conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name=?1")
            .expect("prepare sqlite_master query");

        for name in ["settings", "candidates", "usage_events", "index_state"] {
            let mut rows = stmt.query([name]).expect("query table name");
            assert!(rows.next().expect("fetch table row").is_some());
        }
    }

    /// A file SQLite cannot read must not stop the app from starting. The
    /// unreadable bytes are kept under a `.corrupt-*` name, never deleted.
    #[test]
    fn a_corrupt_database_is_kept_and_replaced() {
        let dir = std::env::temp_dir().join(format!(
            "zeidex-corrupt-{}-{}",
            std::process::id(),
            current_unix_s().expect("clock")
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join("zeidex.db");
        let garbage = b"not a sqlite database, just bytes".to_vec();
        std::fs::write(&path, &garbage).expect("write garbage");

        let store = SqliteStore::open_or_recover(&path).expect("recover from corruption");
        // The recovered store is usable straight away.
        store
            .record_clipboard_entry("after recovery", None)
            .expect("write to the recovered store")
            .expect("a row id");
        assert_eq!(
            store
                .clipboard_entries(CLIPBOARD_KIND_TEXT, "", 10)
                .expect("read")
                .len(),
            1
        );

        let backups: Vec<_> = std::fs::read_dir(&dir)
            .expect("list temp dir")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains(".corrupt-"))
            .collect();
        assert_eq!(
            backups.len(),
            1,
            "the corrupt file must be kept, not deleted"
        );
        assert_eq!(
            std::fs::read(backups[0].path()).expect("read backup"),
            garbage,
            "the backup must hold the original bytes untouched"
        );

        drop(store);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Busy and permission failures are not corruption: renaming the file would
    /// hide a problem the user can still fix.
    #[test]
    fn only_corruption_counts_as_recoverable() {
        let locked = StorageError::Sql(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
            Some("database is locked".to_string()),
        ));
        assert!(!locked.is_corruption());

        let corrupt = StorageError::Sql(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_NOTADB),
            Some("file is not a database".to_string()),
        ));
        assert!(corrupt.is_corruption());
    }

    #[test]
    fn upsert_and_load_candidates_round_trip() {
        let mut store = SqliteStore::open_in_memory().expect("open sqlite in memory");
        let first = candidate("app:test", "Test App", "/Applications/Test.app");

        store.upsert_candidates(&[first]).expect("insert candidate");

        let loaded = store.load_candidates(None).expect("load candidates");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id.as_ref(), "app:test");
        assert_eq!(loaded[0].title.as_ref(), "Test App");
        assert_eq!(loaded[0].path.as_ref(), "/Applications/Test.app");
        assert_eq!(loaded[0].use_count, 0);
        assert_eq!(loaded[0].last_used_at_unix_s, None);
    }

    #[test]
    fn batch_rows_are_files_and_folders_only() {
        let mut store = SqliteStore::open_in_memory().expect("open sqlite in memory");
        let mut file = candidate("file:/docs/report.pdf", "report.pdf", "/docs/report.pdf");
        file.kind = CandidateKind::File;
        file.fs_modified_at_unix_s = Some(1_700_000_000);
        let mut folder = candidate("folder:/docs/Archive", "Archive", "/docs/Archive");
        folder.kind = CandidateKind::Folder;
        // An app row is indexed but has no name to find on disk.
        let app = candidate("app:test", "Test App", "/Applications/Test.app");

        store
            .upsert_candidates(&[file, folder, app])
            .expect("insert candidates");

        let rows = store.load_indexed_paths().expect("load indexed paths");
        assert_eq!(rows.len(), 2);
        let report = rows
            .iter()
            .find(|row| row.name == "report.pdf")
            .expect("file row");
        assert_eq!(report.path, "/docs/report.pdf");
        assert!(!report.is_folder);
        assert_eq!(report.modified_unix_s, Some(1_700_000_000));
        let archive = rows
            .iter()
            .find(|row| row.name == "Archive")
            .expect("folder row");
        assert!(archive.is_folder);
    }

    #[test]
    fn a_declared_row_icon_survives_the_index() {
        // Source rows are persisted like every other candidate and re-read from
        // here on the next launch. An icon that did not round-trip would show
        // once, at index time, and be gone after a restart.
        let mut store = SqliteStore::open_in_memory().expect("open sqlite in memory");
        let mut row = candidate("src:history:https://github.com", "GitHub", "");
        row.icon = Some("/tmp/fav/github.com.png".into());
        store.upsert_candidates(&[row]).expect("insert candidate");

        let loaded = store.load_candidates(None).expect("load candidates");
        assert_eq!(loaded[0].icon.as_deref(), Some("/tmp/fav/github.com.png"));

        // A re-index that changes only the icon must still write: it is in the
        // change-detection list, so a moved favicon is not stale forever.
        let mut moved = candidate("src:history:https://github.com", "GitHub", "");
        moved.icon = Some("/tmp/fav2/github.com.png".into());
        store
            .upsert_candidates(&[moved])
            .expect("update candidate icon");
        let after = store.load_candidates(None).expect("reload candidates");
        assert_eq!(after[0].icon.as_deref(), Some("/tmp/fav2/github.com.png"));
    }

    #[test]
    fn usage_recording_and_upsert_preserves_usage_metrics() {
        let mut store = SqliteStore::open_in_memory().expect("open sqlite in memory");
        let first = candidate("app:test", "Test App", "/Applications/Test.app");

        store.upsert_candidates(&[first]).expect("insert candidate");

        store
            .record_usage_event("app:test", "open_app")
            .expect("record usage event");

        let after_usage = store
            .load_candidates(None)
            .expect("load candidates after usage");
        assert_eq!(after_usage[0].use_count, 1);
        assert!(after_usage[0].last_used_at_unix_s.is_some());

        let updated = Candidate {
            id: "app:test".into(),
            kind: CandidateKind::App,
            title: "Renamed App".into(),
            subtitle: Some("updated subtitle".into()),
            path: "/Applications/Renamed.app".into(),
            ..Default::default()
        };

        store
            .upsert_candidates(&[updated])
            .expect("upsert updated candidate");

        let final_rows = store.load_candidates(None).expect("load final candidates");
        assert_eq!(final_rows.len(), 1);
        assert_eq!(final_rows[0].title.as_ref(), "Renamed App");
        assert_eq!(final_rows[0].path.as_ref(), "/Applications/Renamed.app");
        assert_eq!(final_rows[0].use_count, 1);
        assert!(final_rows[0].last_used_at_unix_s.is_some());

        let usage_count: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM usage_events", [], |row| row.get(0))
            .expect("count usage events");
        assert_eq!(usage_count, 1);
    }

    #[test]
    fn delete_stale_candidates_removes_older_rows() {
        let mut store = SqliteStore::open_in_memory().expect("open sqlite in memory");
        let old = candidate("app:old", "Old", "/Applications/Old.app");
        let fresh = candidate("app:fresh", "Fresh", "/Applications/Fresh.app");

        store
            .upsert_candidates_indexed(&[old], Some(100))
            .expect("insert old");
        store
            .upsert_candidates_indexed(&[fresh], Some(200))
            .expect("insert fresh");

        let removed = store
            .delete_stale_candidates(150)
            .expect("delete stale candidates");
        assert_eq!(removed, 1);

        let loaded = store.load_candidates(None).expect("load candidates");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id.as_ref(), "app:fresh");
    }

    #[test]
    fn delete_stale_candidates_also_cleans_null_indexed_rows() {
        let mut store = SqliteStore::open_in_memory().expect("open sqlite in memory");
        let old_style = candidate("app:null", "Null Indexed", "/Applications/Null.app");
        store
            .upsert_candidates_indexed(&[old_style], None)
            .expect("insert null-indexed row");

        let removed = store
            .delete_stale_candidates(1)
            .expect("delete stale including null");
        assert_eq!(removed, 1);

        let loaded = store.load_candidates(None).expect("load candidates");
        assert!(loaded.is_empty());
    }

    #[test]
    fn upsert_skips_write_when_row_unchanged() {
        let mut store = SqliteStore::open_in_memory().expect("open sqlite in memory");
        let c = candidate("file:a", "a.txt", "/x/a.txt");
        store
            .upsert_candidates_indexed(std::slice::from_ref(&c), Some(100))
            .expect("first insert");
        // Re-upsert identical content on a later run: the WHERE guard should skip
        // the UPDATE, so indexed_at stays at the original value (no rewrite).
        store
            .upsert_candidates_indexed(&[c], Some(200))
            .expect("no-op upsert");
        let indexed: i64 = store
            .conn
            .query_row(
                "SELECT indexed_at_unix_s FROM candidates WHERE id = 'file:a'",
                [],
                |row| row.get(0),
            )
            .expect("read indexed_at");
        assert_eq!(indexed, 100, "unchanged row must not be rewritten");
    }

    #[test]
    fn upsert_writes_when_content_changes() {
        let mut store = SqliteStore::open_in_memory().expect("open sqlite in memory");
        store
            .upsert_candidates_indexed(&[candidate("file:a", "a.txt", "/x/a.txt")], Some(100))
            .expect("first insert");
        store
            .upsert_candidates_indexed(&[candidate("file:a", "renamed.txt", "/x/a.txt")], Some(200))
            .expect("changed upsert");
        let indexed: i64 = store
            .conn
            .query_row(
                "SELECT indexed_at_unix_s FROM candidates WHERE id = 'file:a'",
                [],
                |row| row.get(0),
            )
            .expect("read indexed_at");
        assert_eq!(indexed, 200, "changed row must be rewritten");
        let loaded = store.load_candidates(None).expect("load");
        assert_eq!(loaded[0].title.as_ref(), "renamed.txt");
    }

    #[test]
    fn delete_unseen_removes_unseen_keeps_seen_and_pinned() {
        let mut store = SqliteStore::open_in_memory().expect("open sqlite in memory");
        store
            .upsert_candidates_indexed(
                &[
                    candidate("file:a", "a", "/x/a"),
                    candidate("file:b", "b", "/x/b"),
                ],
                Some(100),
            )
            .expect("insert files");
        // Pinned/seeded row uses i64::MAX indexed_at to survive sweeps.
        store
            .upsert_candidates_indexed(&[candidate("uwp:pinned", "Pinned", "/p")], Some(i64::MAX))
            .expect("insert pinned");

        // The latest scan only saw file:a.
        let mut seen: HashSet<Box<str>> = HashSet::new();
        seen.insert("file:a".into());
        let removed = store
            .delete_unseen_candidates(&seen, 200, &[])
            .expect("prune unseen");
        assert_eq!(removed, 1);

        let ids: Vec<String> = store
            .load_candidates(None)
            .expect("load")
            .iter()
            .map(|c| c.id.to_string())
            .collect();
        assert!(ids.contains(&"file:a".to_string()), "seen row kept");
        assert!(ids.contains(&"uwp:pinned".to_string()), "pinned row kept");
        assert!(!ids.contains(&"file:b".to_string()), "unseen row removed");
    }

    #[test]
    fn delete_unseen_respects_prefixes() {
        let mut store = SqliteStore::open_in_memory().expect("open sqlite in memory");
        store
            .upsert_candidates_indexed(
                &[
                    candidate("app:x", "X", "/x"),
                    candidate("file:y", "Y", "/y"),
                ],
                Some(100),
            )
            .expect("insert");
        // Scoped (apps-only) refresh saw nothing: only app:* may be pruned.
        let seen: HashSet<Box<str>> = HashSet::new();
        let removed = store
            .delete_unseen_candidates(&seen, 200, &["app:"])
            .expect("scoped prune");
        assert_eq!(removed, 1);
        let ids: Vec<String> = store
            .load_candidates(None)
            .expect("load")
            .iter()
            .map(|c| c.id.to_string())
            .collect();
        assert!(ids.contains(&"file:y".to_string()), "out-of-scope row kept");
        assert!(
            !ids.contains(&"app:x".to_string()),
            "in-scope unseen removed"
        );
    }

    #[test]
    fn delete_candidates_by_ids_removes_usage_rows_first() {
        let mut store = SqliteStore::open_in_memory().expect("open sqlite in memory");
        store
            .upsert_candidates_indexed(
                &[
                    candidate("file:gone", "gone", "/x/gone"),
                    candidate("file:kept", "kept", "/x/kept"),
                ],
                Some(100),
            )
            .expect("insert");
        store
            .record_usage_event("file:gone", "open")
            .expect("record usage");

        let removed = store
            .delete_candidates_by_ids(&["file:gone".to_string()])
            .expect("delete by id");
        assert_eq!(removed, 1);

        let ids: Vec<String> = store
            .load_candidates(None)
            .expect("load")
            .iter()
            .map(|c| c.id.to_string())
            .collect();
        assert_eq!(ids, vec!["file:kept".to_string()]);
        // The foreign key would have refused the candidate delete if the usage
        // row had survived, so reaching here already proves order; assert the
        // row is gone explicitly instead of trusting that.
        let usage: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM usage_events", [], |row| row.get(0))
            .expect("count usage");
        assert_eq!(usage, 0);
    }

    #[test]
    fn delete_candidates_by_ids_is_a_noop_for_an_empty_list() {
        let mut store = SqliteStore::open_in_memory().expect("open sqlite in memory");
        store
            .upsert_candidates_indexed(&[candidate("file:a", "a", "/x/a")], Some(100))
            .expect("insert");
        assert_eq!(store.delete_candidates_by_ids(&[]).expect("noop"), 0);
        assert_eq!(store.load_candidates(None).expect("load").len(), 1);
    }

    #[test]
    fn candidate_paths_exposes_the_id_and_path_pairs() {
        let mut store = SqliteStore::open_in_memory().expect("open sqlite in memory");
        store
            .upsert_candidates_indexed(
                &[
                    candidate("file:a", "a", "/x/a.txt"),
                    candidate("folder:b", "b", "/x/b"),
                ],
                Some(100),
            )
            .expect("insert");

        let mut pairs = store.candidate_paths().expect("candidate paths");
        pairs.sort();
        assert_eq!(
            pairs,
            vec![
                ("file:a".to_string(), "/x/a.txt".to_string()),
                ("folder:b".to_string(), "/x/b".to_string()),
            ]
        );
    }

    #[test]
    fn is_demo_seeded_recognizes_demo_rows() {
        let mut store = SqliteStore::open_in_memory().expect("open sqlite in memory");
        let safari = candidate("app:safari", "Safari", "/Applications/Safari.app");
        let vscode = candidate("app:vscode", "VS Code", "/Applications/Code.app");
        store
            .upsert_candidates_indexed(&[safari, vscode], Some(100))
            .expect("seed demo rows");

        assert!(store.is_demo_seeded().expect("query"));
    }

    #[test]
    fn is_demo_seeded_returns_false_for_empty_table() {
        let store = SqliteStore::open_in_memory().expect("open sqlite in memory");
        assert!(!store.is_demo_seeded().expect("query"));
    }

    #[test]
    fn is_demo_seeded_returns_false_when_real_data_present() {
        let mut store = SqliteStore::open_in_memory().expect("open sqlite in memory");
        // > 6 rows means we're past the demo seed regardless of contents.
        let rows: Vec<Candidate> = (0..10)
            .map(|i| {
                candidate(
                    &format!("app:user-app-{i}"),
                    &format!("App {i}"),
                    &format!("/Applications/App{i}.app"),
                )
            })
            .collect();
        store
            .upsert_candidates_indexed(&rows, Some(100))
            .expect("seed");
        assert!(!store.is_demo_seeded().expect("query"));
    }

    #[test]
    fn is_demo_seeded_returns_false_for_partial_demo_match() {
        let mut store = SqliteStore::open_in_memory().expect("open sqlite in memory");
        // Only one demo marker - not the full seed.
        let only_safari = candidate("app:safari", "Safari", "/Applications/Safari.app");
        store
            .upsert_candidates_indexed(&[only_safari], Some(100))
            .expect("seed");
        assert!(!store.is_demo_seeded().expect("query"));
    }

    #[test]
    fn delete_stale_candidates_with_prefixes_is_noop_on_empty_prefix_list() {
        let mut store = SqliteStore::open_in_memory().expect("open sqlite in memory");
        let old = candidate("app:old", "Old", "/Applications/Old.app");
        store
            .upsert_candidates_indexed(&[old], Some(100))
            .expect("insert");

        let removed = store
            .delete_stale_candidates_with_prefixes(200, &[])
            .expect("noop");
        assert_eq!(removed, 0);
        let loaded = store.load_candidates(None).expect("load");
        assert_eq!(loaded.len(), 1, "row must be preserved");
    }

    #[test]
    fn delete_stale_candidates_with_prefixes_accepts_multiple_prefixes() {
        let mut store = SqliteStore::open_in_memory().expect("open sqlite in memory");
        let old_app = candidate("app:old", "Old App", "/Applications/Old.app");
        let old_file = candidate("file:old", "Old File", "/Users/demo/old.txt");
        let old_folder = candidate("folder:old", "Old Folder", "/Users/demo/old");
        let old_setting = candidate("setting:old", "Old Setting", "settings://old");

        store
            .upsert_candidates_indexed(&[old_app, old_file, old_folder, old_setting], Some(100))
            .expect("seed");

        // Sweep apps + files (mirrors what BootstrapScope::FILES_ONLY produces
        // via id_prefixes() - file + folder together - plus apps).
        let removed = store
            .delete_stale_candidates_with_prefixes(200, &["app:", "file:", "folder:"])
            .expect("sweep");
        assert_eq!(removed, 3);

        let ids: Vec<String> = store
            .load_candidates(None)
            .expect("load")
            .into_iter()
            .map(|c| c.id.as_ref().to_string())
            .collect();
        assert_eq!(ids, vec!["setting:old"]);
    }

    #[test]
    fn delete_stale_candidates_with_prefixes_purges_matching_usage_events() {
        let mut store = SqliteStore::open_in_memory().expect("open sqlite in memory");
        let old_app = candidate("app:old", "Old App", "/Applications/Old.app");
        let old_file = candidate("file:old", "Old File", "/Users/demo/old.txt");
        store
            .upsert_candidates_indexed(&[old_app.clone(), old_file.clone()], Some(100))
            .expect("seed");
        store
            .record_usage_event(old_app.id.as_ref(), "open_app")
            .expect("record app usage");
        store
            .record_usage_event(old_file.id.as_ref(), "open_file")
            .expect("record file usage");

        let removed = store
            .delete_stale_candidates_with_prefixes(200, &["app:"])
            .expect("sweep apps");
        assert_eq!(removed, 1);

        // app:old's usage_events row must be gone; file:old's must remain.
        let app_usage: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM usage_events WHERE candidate_id = ?1",
                params!["app:old"],
                |row| row.get(0),
            )
            .expect("count app usage");
        assert_eq!(app_usage, 0);
        let file_usage: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM usage_events WHERE candidate_id = ?1",
                params!["file:old"],
                |row| row.get(0),
            )
            .expect("count file usage");
        assert_eq!(file_usage, 1);
    }

    #[test]
    fn delete_stale_candidates_with_prefixes_only_touches_matching_kinds() {
        let mut store = SqliteStore::open_in_memory().expect("open sqlite in memory");
        let old_app = candidate("app:old", "Old App", "/Applications/Old.app");
        let old_file = candidate("file:old", "Old File", "/Users/demo/old.txt");
        let fresh_app = candidate("app:fresh", "Fresh App", "/Applications/Fresh.app");
        let fresh_file = candidate("file:fresh", "Fresh File", "/Users/demo/fresh.txt");

        store
            .upsert_candidates_indexed(&[old_app, old_file], Some(100))
            .expect("insert old rows");
        store
            .upsert_candidates_indexed(&[fresh_app, fresh_file], Some(200))
            .expect("insert fresh rows");

        let removed = store
            .delete_stale_candidates_with_prefixes(150, &["app:"])
            .expect("prune stale apps");
        assert_eq!(removed, 1);

        let ids: Vec<String> = store
            .load_candidates(None)
            .expect("load")
            .into_iter()
            .map(|c| c.id.as_ref().to_string())
            .collect();
        assert!(!ids.contains(&"app:old".to_string()));
        assert!(ids.contains(&"app:fresh".to_string()));
        // file:old is older than the cutoff but must be untouched because we
        // restricted the sweep to the `app:` prefix.
        assert!(ids.contains(&"file:old".to_string()));
        assert!(ids.contains(&"file:fresh".to_string()));
    }

    #[test]
    fn delete_candidates_by_prefix_except_removes_vanished_rows_and_preserves_kept() {
        let mut store = SqliteStore::open_in_memory().expect("open sqlite in memory");
        let kept = candidate(
            "app:uwp:Microsoft.WindowsTerminal_8wekyb3d8bbwe!App",
            "Terminal",
            "shell:AppsFolder\\Microsoft.WindowsTerminal_8wekyb3d8bbwe!App",
        );
        let stale = candidate(
            "app:uwp:Old.PackageThatGotUninstalled_abc!App",
            "Old App",
            "shell:AppsFolder\\Old.PackageThatGotUninstalled_abc!App",
        );
        let win32 = candidate(
            "app:edge_c:/programdata/microsoft/windows/start menu/programs/microsoft edge.lnk",
            "Microsoft Edge",
            "C:/ProgramData/Microsoft/Windows/Start Menu/Programs/Microsoft Edge.lnk",
        );

        store
            .upsert_candidates_indexed(&[kept, stale.clone(), win32], Some(i64::MAX))
            .expect("seed candidates");

        // Record usage on the stale row so we can verify usage_events are also removed.
        store
            .record_usage_event(stale.id.as_ref(), "open_app")
            .expect("record stale usage");

        let mut keep_set = HashSet::new();
        keep_set.insert("app:uwp:Microsoft.WindowsTerminal_8wekyb3d8bbwe!App");

        let removed = store
            .delete_candidates_by_prefix_except("app:uwp:", &keep_set)
            .expect("prune");
        assert_eq!(removed, 1);

        let loaded = store.load_candidates(None).expect("load");
        let ids: Vec<&str> = loaded.iter().map(|c| c.id.as_ref()).collect();
        assert!(ids.contains(&"app:uwp:Microsoft.WindowsTerminal_8wekyb3d8bbwe!App"));
        assert!(!ids.contains(&"app:uwp:Old.PackageThatGotUninstalled_abc!App"));
        // The non-uwp `app:` row must be untouched even though it shares the broader
        // `app:` prefix - only `app:uwp:` rows are eligible for this sweep.
        assert!(ids.contains(
            &"app:edge_c:/programdata/microsoft/windows/start menu/programs/microsoft edge.lnk"
        ));

        // usage_events for the deleted candidate should also be cleaned up.
        let usage_count: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM usage_events WHERE candidate_id = ?1",
                params![stale.id.as_ref()],
                |row| row.get(0),
            )
            .expect("count usage events");
        assert_eq!(usage_count, 0);
    }

    /// Passes on rusqlite's own 5s busy timeout, so it guards against that being
    /// taken away rather than proving anything the store does. The WAL
    /// conversion in `migrate` is the exclusive-lock step it blocks on.
    #[test]
    fn an_open_waits_for_a_database_someone_else_is_holding() {
        /// Long enough to be blocked when released, far under the 5s timeout.
        const HOLD: std::time::Duration = std::time::Duration::from_millis(200);

        let dir = std::env::temp_dir().join(format!("zeidex-store-busy-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("contended.db");
        let _ = std::fs::remove_file(&path);

        // A plain connection, so the file exists and is NOT yet in WAL.
        let blocker = Connection::open(&path).expect("blocker connection");
        blocker
            .execute_batch("BEGIN EXCLUSIVE; CREATE TABLE probe(x);")
            .expect("take the lock");

        let opening = {
            let path = path.clone();
            std::thread::spawn(move || SqliteStore::open(&path).is_ok())
        };

        std::thread::sleep(HOLD);
        blocker.execute_batch("COMMIT").expect("release the lock");

        assert!(
            opening.join().expect("the opening thread"),
            "an open must wait for the other writer rather than fail"
        );

        drop(blocker);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn delete_candidates_by_prefix_except_is_noop_when_all_kept() {
        let mut store = SqliteStore::open_in_memory().expect("open sqlite in memory");
        let cand = candidate(
            "app:uwp:Microsoft.Notepad_8wekyb3d8bbwe!App",
            "Notepad",
            "shell:AppsFolder\\Microsoft.Notepad_8wekyb3d8bbwe!App",
        );
        store
            .upsert_candidates_indexed(&[cand], Some(i64::MAX))
            .expect("seed");

        let mut keep_set = HashSet::new();
        keep_set.insert("app:uwp:Microsoft.Notepad_8wekyb3d8bbwe!App");

        let removed = store
            .delete_candidates_by_prefix_except("app:uwp:", &keep_set)
            .expect("prune");
        assert_eq!(removed, 0);
    }

    #[test]
    fn prune_usage_events_applies_age_and_count_limits() {
        let mut store = SqliteStore::open_in_memory().expect("open sqlite in memory");
        let first = candidate("app:test", "Test App", "/Applications/Test.app");
        store.upsert_candidates(&[first]).expect("insert candidate");

        store
            .conn
            .execute(
                "INSERT INTO usage_events(candidate_id, action, used_at_unix_s) VALUES (?1, ?2, ?3)",
                params!["app:test", "open", 100_i64],
            )
            .expect("insert old usage");
        store
            .conn
            .execute(
                "INSERT INTO usage_events(candidate_id, action, used_at_unix_s) VALUES (?1, ?2, ?3)",
                params!["app:test", "open", 200_i64],
            )
            .expect("insert usage 2");
        store
            .conn
            .execute(
                "INSERT INTO usage_events(candidate_id, action, used_at_unix_s) VALUES (?1, ?2, ?3)",
                params!["app:test", "open", 300_i64],
            )
            .expect("insert usage 3");

        let removed_old = store
            .prune_usage_events_older_than(150)
            .expect("prune old usage");
        assert_eq!(removed_old, 1);

        let removed_overflow = store
            .prune_usage_events_to_max(1)
            .expect("prune usage overflow");
        assert_eq!(removed_overflow, 1);

        let remaining: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM usage_events", [], |row| row.get(0))
            .expect("count remaining usage");
        assert_eq!(remaining, 1);
    }

    #[test]
    fn url_history_records_dedups_and_matches() {
        let store = SqliteStore::open_in_memory().expect("open sqlite in memory");

        store
            .record_url_hit("https://github.com")
            .expect("record 1");
        store
            .record_url_hit("https://github.com")
            .expect("record 2");
        store
            .record_url_hit("https://crates.io")
            .expect("record other");

        // Repeat opens collapse to one row and accumulate hit_count.
        let all = store.recent_urls("", 10).expect("recent all");
        assert_eq!(all.len(), 2);
        let gh = all
            .iter()
            .find(|e| e.url == "https://github.com")
            .expect("github entry");
        assert_eq!(gh.hit_count, 2);

        // Substring match over the URL, ignoring the scheme.
        let matched = store.recent_urls("github", 10).expect("recent github");
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].url, "https://github.com");

        // A typed LIKE metacharacter is treated literally, not as a wildcard.
        assert!(store.recent_urls("%", 10).expect("recent pct").is_empty());
    }

    #[test]
    fn url_history_prunes_to_capacity() {
        let store = SqliteStore::open_in_memory().expect("open sqlite in memory");
        // One past the cap, each with a distinct, increasing last-used time.
        for i in 0..=MAX_URL_HISTORY_ROWS {
            store
                .conn
                .execute(
                    "INSERT INTO url_history(url, hit_count, last_used_at_unix_s)
                     VALUES (?1, 1, ?2)",
                    params![format!("https://site{i}.com"), i as i64],
                )
                .expect("seed url");
        }
        // A fresh record triggers the capacity prune inside record_url_hit.
        store.record_url_hit("https://newest.com").expect("record");

        let count: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM url_history", [], |row| row.get(0))
            .expect("count");
        assert_eq!(count, MAX_URL_HISTORY_ROWS as i64);
        // The oldest (site0) was evicted; the newest survives.
        assert!(store.recent_urls("site0.com", 5).expect("q").is_empty());
        assert_eq!(store.recent_urls("newest", 5).expect("q").len(), 1);
    }

    #[test]
    fn search_engine_keys_round_trip_and_unknown_keys_are_reported() {
        for engine in SearchEngine::all() {
            assert_eq!(SearchEngine::from_key_checked(engine.key()), Some(engine));
        }
        // `from_key` is the tolerant read used at startup: an unknown stored
        // value falls back instead of failing the whole settings load.
        assert_eq!(SearchEngine::from_key("nope"), SearchEngine::Google);
        // The settings write uses the strict read: a typo must be refused, not
        // silently saved as Google.
        assert_eq!(SearchEngine::from_key_checked("nope"), None);
    }

    #[test]
    fn saving_search_settings_round_trips_every_engine() {
        let mut store = SqliteStore::open_in_memory().expect("open");
        assert!(
            store
                .load_search_settings()
                .expect("load")
                .web_search_enabled
        );
        for engine in SearchEngine::all() {
            store
                .save_search_settings(SearchSettings {
                    web_search_enabled: false,
                    web_search_engine: engine,
                })
                .expect("save");
            let loaded = store.load_search_settings().expect("load");
            assert!(!loaded.web_search_enabled);
            assert_eq!(loaded.web_search_engine, engine);
        }
    }
}
