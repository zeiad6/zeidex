# Architecture

Zeidex is a cross-platform desktop app with two halves. A **Tauri v2 shell**
(`src-tauri/`, Rust) owns the window, tray, global hotkey and every platform
API. A **React + TypeScript frontend** (`app/`) owns what the user sees. Both
sit on top of a portable **Rust core** (`core/`) that does indexing, matching,
ranking, storage, OCR and document export. There is no Objective-C and no C ABI:
the shell depends on the core crates by path.

```text
┌──────────────────────────────────────────────┐
│ Frontend (React + TS + Tailwind)             │  app/
│ launcher panel, clipboard, OCR Studio, RTL   │
└──────────────────────┬───────────────────────┘
                       │ Tauri IPC (invoke / events)
┌──────────────────────▼───────────────────────┐
│ Shell (Tauri v2, Rust)                       │  src-tauri/
│ window, tray, Alt+Space hotkey, commands,    │
│ state (engine + SQLite + OCR engine)         │
└──────────────────────┬───────────────────────┘
                       │ path dependencies
┌──────────────────────▼───────────────────────┐
│ Core (Cargo workspace)                       │  core/
│ engine · indexing · matching · ranking       │
│ storage · tools · answers · ocr_engine       │
│ exporter                                     │
└──────────────────────────────────────────────┘
```

## Repository layout

```text
core/                 Rust workspace: engine, indexing, matching, ranking, storage,
                      tools, answers, ocr_engine, exporter
src-tauri/            Tauri v2 shell: commands, tray, global hotkey, state (own workspace)
app/                  React + TypeScript + TailwindCSS frontend (Vite)
assets/               icon source, screenshots, these docs
third_party/models/   downloaded ONNX models (make models); git-ignored
legacy/               the pre-Tauri macOS SwiftUI shell and C-ABI bridge, reference only
tools/perf/           benchmarks (separate crate, never bundled)
```

The shell is its own Cargo workspace so the frontend and the desktop crate can
resolve independently; it depends on the core crates by relative path. The core
is also a standalone workspace, so `make core-test` builds and tests with no
Tauri, Node or webview dependency.

## Life of a keystroke

1. The global-shortcut plugin fires `Alt+Space`; the shell shows (or hides) the
   `main` window.
2. Every keystroke in the launcher calls the `search` command.
3. The shell takes the `QueryEngine` out of `AppState` and asks it for matches.
4. The engine fuzzy-matches candidates (`matching`), re-ranks the top ones with
   usage history (`ranking`) and returns the best `limit` results.
5. The shell serializes them with `serde`; React renders the rows.

Searches are in-memory. SQLite is touched to load the index at startup, to
rebuild it (`bootstrap_index`), and to record usage.

## Index writes

Everything that changes the index goes through one worker thread (`IndexService`
in `src-tauri/src/index_service.rs`): the startup walk, the manual Reindex
button, app-only refreshes after a shortcut changes, and the filesystem
watcher. Two full walks therefore cannot race, and the SQLite writes stay
serialized.

The watcher (`index_watch.rs`) registers the configured app and file roots with
`notify`, folds events into a delta for two seconds (the first event sets the
deadline; later ones never extend it, so a busy directory cannot starve
updates), then hands the delta to the worker. Registration happens before the
startup walk, and a root that cannot be watched is reported in the index-status
event — the launcher and Settings show it instead of leaving a desktop launch to
fail silently. The watcher keeps the failed directories with their reasons, so a
registration that fails later is reported too and a recovered one clears the
warning; a configured root that does not exist is reported rather than watching
nothing. Events that arrive during a full walk queue behind it and are applied
when it finishes, so a delta always describes a state newer than the walk's
snapshot. File paths become
`QueryEngine::refresh_paths` — targeted deletes plus single-path upserts — while
an app-root event becomes the scoped `APPS_ONLY` walk, because a menu entry is
parsed rather than walked and has no path to address. `Access` events are
ignored, and a rename is resolved by asking whether the path exists.

After every change the worker reloads the in-memory engine and emits
`zeidex://index-status` with the real count; the launcher and Settings footer
adopt that number instead of a value sampled at mount. Saving an index-affecting
setting starts a full walk immediately, so the warning in Settings is
"rebuilding", not "press Reindex again".

A full walk can be paused, resumed or cancelled. `IndexControl`
(`core/indexing/src/control.rs`) is one atomic state plus a condition variable:
the walker checks it once per directory entry (an atomic load in the normal
case) and parks on the condvar while paused, and the writer checks it between
512-row chunks. Cancelling **skips the stale-row sweep** — rows the walk never
visited are not gone, and pruning them would delete a large part of a working
index — so a cancelled walk leaves a valid union of old and new rows, and
`BootstrapOutcome` says which happened. The service only reports a pause or a
cancel when a walk is actually running, so the UI never claims a change that did
not happen.

## Document text index

The OCR Studio stores the text it recognizes so the launcher can find a file by
a phrase inside it. Two tables in the same SQLite file (`core/storage/src/
documents.rs`): `document_text` (one row per path, replaced on re-run) and
`document_token`, an inverted index over the same normalized tokens search uses.
A query intersects the posting lists of its tokens, rarest first; name matches
rank above content hits, and content search is skipped below three characters.
The index is bounded (400k characters per document, 20,000 documents) and can be
switched off or cleared in Settings. Each row records the file's mtime when its
text was read; a named `document-index-prune` thread stats every stored path once
at startup and drops rows whose file is gone or whose mtime no longer matches,
cascading to their tokens. Rows without a recorded mtime, and rows whose stat
failed for a reason other than `NotFound`, are kept. Nothing is sent anywhere:
the body comes from the local engine and stays in the local database.

## Shell state

`AppState` holds the pieces that must survive between commands:

| Field | Purpose |
| --- | --- |
| `engine` | the in-memory `QueryEngine`, reloaded after a re-index |
| `store` | the SQLite store for usage, candidates and clipboard history |
| `paths` | the path capability policy: scan roots, minted picks, app-owned directories |
| `last_clipboard` | guards the clipboard poller against re-recording the same clip |
| `ocr` | the lazily loaded `OcrEngine` (five ONNX sessions; too slow to load per page) |
| `models_dir`, `pdfium_library`, `db_path` | resolved per platform, overridable by env |

Paths resolve per platform (`dirs`), with `ZEIDEX_DB_PATH`, `ZEIDEX_MODELS_DIR`
and `ZEIDEX_PDFIUM_LIBRARY` as overrides for development and QA.

## Path capabilities

A Tauri command that accepts a path from the webview and hands it to the OCR
engine, `std::fs` or the desktop opener is trusting a page to choose what the
process touches. Zeidex therefore treats "may I read this?" as a question with a
policy, not a formality (`src-tauri/src/commands/policy.rs`).

A path is readable only when it is one of:

1. inside a configured scan root — the folders the user asked Zeidex to index;
2. minted by a native picker, an OS drag-and-drop, or an export Zeidex just wrote;
3. an app-owned path — the database, the config, the model directories;
4. a row already in the index.

Rule 4 is what keeps the launcher working: a search result is by definition a row
the walker wrote, and application entries live in `/usr/share/applications`,
outside every scan root. Containment is decided on canonical paths, so `..` and a
symlink pointing out of a root are refused; a path that does not exist is resolved
through its canonical parent, because a leaf that existed would have taken the
canonical branch.

The mint list is a 512-entry FIFO: a drop of a thousand files cannot grow it
without bound, and the file just chosen is the one the next command reads. It
lives in memory, so a restart forgets picks — the user chooses again.

Two consequences are deliberate. The policy bounds *where* a compromised page can
reach, not *what* it may do there: a path inside a scan root is readable because
the user asked for that folder to be indexed. And `authorize_read` takes the store
lock to answer rule 4, so it must never be called while that lock is already held
— `std::Mutex` is not reentrant; `commands/documents.rs` authorizes before locking
for exactly this reason.

## Where data lives

| Data | Location |
| --- | --- |
| Settings | `~/.zeidex/config` |
| Candidates, usage, clipboard history | SQLite, in the user data dir (`~/.local/share/zeidex` on Linux, `%APPDATA%` on Windows) |
| ONNX models | `third_party/models` in a checkout, the user data dir once bundled |
| Downloaded OCR packs | `<models_root>/packs/<id>/` with a verified `manifest.json`; the active pack is `active-pack.json` beside them |

## Product direction

The 0.1 surface is deliberately one command palette plus three focused screens
(Document Studio, Batch Search, OCR Models). The evaluation of a wider
"productivity platform" concluded:

- **Keep the palette unified.** Apps, files, folders, commands, calculations,
  clipboard entries and settings already share one ranked list; adding separate
  launchers would split the ranking and the keyboard model.
- **Add depth as screens, not plugins.** Batch Search and the model manager are
  separate surfaces because they have their own state and reports, but they read
  the same index and the same OCR engine.
- **Do not ship a workflow engine yet.** Chaining arbitrary actions needs a
  permission model, a dry-run story and a durable audit trail; none of those is
  worth bolting onto a local launcher without a concrete user request. The
  building blocks (indexed paths, safe `open`/`reveal`, batch reports) stay
  available for a later design.
- **No cloud dependency is a product invariant.** Model downloads are the only
  network operation, they are user-initiated, and every file is pinned and
  hashed.

## Further reading

- [Rust core](core.md)
- [Build and release](build-and-release.md)
- [OCR Studio](ocr-studio.md)
- [Legacy macOS shell](../legacy/README.md)
