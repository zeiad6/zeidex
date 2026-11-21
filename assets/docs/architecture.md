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

## Shell state

`AppState` holds the pieces that must survive between commands:

| Field | Purpose |
| --- | --- |
| `engine` | the in-memory `QueryEngine`, reloaded after a re-index |
| `store` | the SQLite store for usage, candidates and clipboard history |
| `last_clipboard` | guards the clipboard poller against re-recording the same clip |
| `ocr` | the lazily loaded `OcrEngine` (five ONNX sessions; too slow to load per page) |
| `models_dir`, `pdfium_library`, `db_path` | resolved per platform, overridable by env |

Paths resolve per platform (`dirs`), with `ZEIDEX_DB_PATH`, `ZEIDEX_MODELS_DIR`
and `ZEIDEX_PDFIUM_LIBRARY` as overrides for development and QA.

## Where data lives

| Data | Location |
| --- | --- |
| Settings | `~/.zeidex/config` |
| Candidates, usage, clipboard history | SQLite, in the user data dir (`~/.local/share/zeidex` on Linux, `%APPDATA%` on Windows) |
| ONNX models | `third_party/models` in a checkout, the user data dir once bundled |

## Further reading

- [Rust core](core.md)
- [Build and release](build-and-release.md)
- [OCR Studio](ocr-studio.md)
- [Legacy macOS shell](../legacy/README.md)
