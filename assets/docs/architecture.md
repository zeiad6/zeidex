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
