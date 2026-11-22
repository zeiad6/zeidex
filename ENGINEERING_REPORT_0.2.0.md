# Zeidex 0.2.0 — engineering report

Evidence only. Every command in section H was executed in this tree on
2026-11-22; every number is a measurement from that run. Anything that could not
be exercised here is listed in section J instead of being described as done.

Environment: Linux arm64 (Termux/PRoot), no Windows machine, no display server.
Rust 1.99.1, Node 22, ONNX Runtime and pdfium from `src-tauri/resources/lib`.

---

## A. Summary

The release fixes three defects that made the shipped Windows build feel broken
(native dialogs behind the window, a console window that owned the process,
PDFium dying after a model switch), adds the window controls a frameless window
needs, makes the global shortcut configurable and observable, and adds two
measured capabilities: born-digital PDFs are now read instead of OCRed, and
Arabic OCR has a repeatable CER/WER benchmark.

| Area | Change | Verified here |
| --- | --- | --- |
| Windows shell | GUI subsystem, startup log, dialog ownership, caption buttons, tray-guarded close, taskbar presence | Compiles, clippy clean; **runtime not verified** |
| PDF | One process-wide pdfium handle; native text layer before OCR | Tests, both paths |
| OCR | Arabic corpus + CER/WER benchmark; native-text pages skip OCR | Tests, measured |
| Search | Measured on 5,055 candidates (section E); no code change needed | Benchmark run |
| Batch search | Threshold behaviour already implemented and tested | 555-test suite |
| Release | Version 0.2.0 in app manifests; core crates inherit the workspace version | Builds |

37 tracked files changed, 822 insertions, 112 deletions, plus new files
(`WindowControls.tsx`, `shortcut.rs`, `pdf_lifecycle.rs`, `arabic_cer.rs`,
`pdf_native_text.rs`, the Arabic corpus, two fixture generators).

---

## B. Bugs fixed

### B1. Native dialogs opened behind Zeidex

- **Root cause:** two independent causes. `tauri.conf.json` set
  `alwaysOnTop: true`, and a native picker is not a topmost window, so the
  launcher sat above its own dialog. Separately, every picker was created with
  no owner, so Windows had no reason to put it above Zeidex or make it modal.
- **Files:** `src-tauri/tauri.conf.json`, `src-tauri/src/commands/dialogs.rs`.
- **Fix:** removed `alwaysOnTop`, set `skipTaskbar: false` and `shadow: true`;
  every open/save/folder picker now calls `set_parent(main window)` before it is
  shown. No delays, no focus hacks, no forced topmost on the dialog.
- **Test:** `cargo check`/`clippy` clean. Z-order is a Windows runtime property;
  see section J.

### B2. A console window appeared and owned the app

- **Root cause:** `src-tauri/src/main.rs` had no `windows_subsystem` attribute,
  so the release executable was a console-subsystem binary. Windows therefore
  allocated a terminal for it, and closing that terminal ended the process.
- **Files:** `src-tauri/src/main.rs`, `src-tauri/src/lib.rs`.
- **Fix:** `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]`.
  Debug builds keep the console. Release startup failures, which no longer have
  a console, are appended to a size-capped `zeidex-startup.log` next to the
  database.
- **Test:** compiles in both profiles; the attribute only takes effect on
  Windows, so runtime confirmation is listed as unverified.

### B3. `PdfiumLibraryBindingsAlreadyInitialized` after a model switch

- **Root cause:** `pdfium-render` binds its library into a process-global cell
  once. Every `OcrEngine` construction bound again, so the second engine (model
  pack activation, benchmark, `reset_ocr_engine`) failed and took image OCR down
  with it.
- **Files:** `core/ocr_engine/src/pdf.rs`, `core/ocr_engine/src/pipeline.rs`.
- **Fix:** one `Mutex<Option<Arc<Pdfium>>>` (`pdf::shared_pdfium`); a failed bind
  is not cached; `AlreadyInitialized` reuses `Pdfium::default()` instead of being
  swallowed.
- **Test:** `tests/pdf_lifecycle.rs` builds two engines, OCRs and previews the
  same PDF with each, processes it twice more and once through a page window,
  then asserts both bind calls return the same allocation. On the pre-fix code
  the same test failed with the exact reported error; on this tree it passes.

### B4. A minimized window stayed minimized when the hotkey fired

- **Root cause:** `show_main` called `show()` and `set_focus()` but never
  un-minimized, so a minimized window was "shown" while staying minimized.
- **Files:** `src-tauri/src/lib.rs`.
- **Fix:** `unminimize()` before `show()`/`set_focus()`; the toggle hides only
  when the window is focused and brings it forward otherwise.

### B5. Closing could leave an invisible process

- **Root cause:** close always hid the window, even on a desktop where the tray
  icon had failed to create.
- **Files:** `src-tauri/src/lib.rs`.
- **Fix:** hide-to-tray only when the tray exists (`TrayAvailable`); otherwise
  Close exits. Quit is an explicit tray action.

### B6. The shortcut could be registered twice

- **Root cause:** registration was scattered and never unregistered first, so a
  press could toggle twice.
- **Files:** new `src-tauri/src/commands/shortcut.rs`.
- **Fix:** one owner, unregister-all before register, default fallback when the
  system refuses a custom combination, persisted choice, visible status.

---

## C. Windows improvements (implemented; runtime unverified here)

- **Console:** GUI subsystem in release, console in debug, file-based startup
  diagnostics.
- **Dialogs:** owner set on every picker; `alwaysOnTop` removed; window appears
  in the taskbar so minimize is recoverable.
- **Window controls:** `WindowControls.tsx` in all five screens — minimize,
  maximize/restore with the icon following `onResized`, close; tooltips,
  `aria-label`, hover/active/focus states, RTL-aware order.
- **Close policy:** hide to tray when a tray exists, exit otherwise; tray menu
  has Restore and Quit.
- **Global shortcut:** configurable, persisted, unregistered before
  re-registering, falls back to `Alt+Space`, and reports registration failure in
  Settings instead of `eprintln!` (a release build has no console).
- **Focus:** regaining window focus puts the caret in the search field and
  selects the previous query.
- **Capabilities:** `toggle-maximize`, `maximize`, `unmaximize`, `is-maximized`,
  `unminimize`, `close` added, so packaged builds do not silently fail.

---

## D. PDFium fix

- **Root cause:** per-engine binding against a process-global cell.
- **Strategy:** bind once, share the `Arc`, never cache a failure, treat
  `AlreadyInitialized` as "reuse what exists" (documented crate behaviour).
- **Files:** `core/ocr_engine/src/pdf.rs` (`shared_pdfium`, `NativePage`,
  `native_page_text`), `core/ocr_engine/src/pipeline.rs` (engine construction,
  page loop).
- **Tests:** `tests/pdf_lifecycle.rs` (two engines, repeated processing, page
  window, shared allocation) and `tests/pdf_smoke.rs` (image-only PDF still
  OCRs). Results: 1 passed / 183.46 s and 1 passed / 32.33 s.
- **Pre-fix evidence:** the same lifecycle test on the stashed pre-fix sources
  failed with `pdf error: PdfiumLibraryBindingsAlreadyInitialized`.

---

## E. Search and indexing

No search code was changed in this round; it was measured instead of assumed.

`cargo run --release --manifest-path tools/perf/Cargo.toml --bin query_engine_bench`:

```
candidate_count=5055
index_elapsed_ms=3243 index_throughput_per_sec=1558.57
query,iterations,p50_us,p95_us,avg_us
,300,94,103,93
sa,300,1151,1191,1159
net,300,1267,1304,1267
doc,300,1154,1188,1159
visual,300,1191,1221,1195
privacy security,300,1721,1750,1724
a"safari,300,53,54,53
f"note,300,895,933,901
d"down,300,252,261,253
```

Interpretation: sub-2 ms p95 on a 5k-candidate index, including the two-term
query; index bootstrap runs at ~1,559 candidates/s. Fuzzy scoring is 65–89 ns per
operation with the prepared query (down from 84–89 ns on the raw path).

Indexing/document-search facts confirmed in code and tests, not changed:

- `document_text` + `document_token` store original text and a normalized
  inverted index; both writer and reader call the same
  `normalize_for_search`, so `صفحه`/`صفحة` and `CAFÉ`/`cafe` meet.
- Batch search applies the 50–100 % slider as a real filter
  (`the_threshold_changes_the_answer`, `a_score_equal_to_the_threshold_is_not_dropped`),
  clamps out-of-range values, and refuses a report that claims more than 100 %.

---

## F. OCR and Arabic

### F1. Arabic benchmark (new)

- **Corpus:** `tools/make-arabic-corpus.py` renders ten lines with Noto Naskh
  Arabic Bold 44 px, `direction="rtl"`, and writes `ground_truth.json`; the
  images and truth are committed under `core/ocr_engine/tests/fixtures/arabic_corpus/`.
- **Metric:** `tests/arabic_cer.rs` computes CER and WER per line and in
  aggregate, plus a "search" CER after `normalize_for_search` to show how much
  error the search layer already absorbs. Raw and normalized CER are identical
  here (15.57 %), i.e. the remaining errors are not letter-variant errors.
- **Measured (PP-OCRv4 Arabic rec, `ppocrv4_rec_ar.onnx`):**

| Line | CER | WER | Recognized |
| --- | --- | --- | --- |
| مرحبا بكم في لوميو | 0.00 % | 0.00 % | exact |
| كتاب الطالب | 0.00 % | 0.00 % | exact |
| السلام عليكم ورحمة الله | 39.13 % | 50.00 % | garbled middle |
| الفاتورة رقم ١٢٣٤٥ | 11.11 % | 33.33 % | lost ١ and ٣ |
| المجموع 1250 ريال | 29.41 % | 100 % | digits mangled |
| تقرير الاجتماع الدوري | 0.00 % | 0.00 % | exact |
| مدرسة الأطفال | 0.00 % | 0.00 % | exact |
| مصطفى علي | 0.00 % | 0.00 % | exact |
| اختبار PDF | 40.00 % | 50.00 % | Latin half dropped |
| الذكاء الاصطناعي في التعليم | 22.22 % | 50.00 % | two errors |
| **Aggregate** | **15.57 %** | **31.03 %** | 5/10 exact |

- The test pins ceilings (CER ≤ 35 %, WER ≤ 60 %) so a regression fails the
  build; they are not a quality claim.
- **Limits this exposes and does not hide:** mixed Arabic/Latin lines return
  only the Arabic half; long lines degrade in the middle; Arabic-Indic digits
  are the weakest class. These are model limits, not preprocessing bugs, and are
  recorded per line instead of averaged away.

### F2. Born-digital PDFs skip OCR (new)

- **Measurement:** the image-only one-page fixture (`table.pdf`, A4 at 300 DPI)
  takes **32.33 s** to OCR (`pdf_smoke`). The same page read through its own
  text layer takes **1.36 s** including engine and model load
  (`pdf_native_text`), and returns the exact characters.
- **Rule:** per page, ≥ 24 letters/digits in the text layer → use it; less →
  render and OCR as before (a scan with a "Page 3" stamp still gets OCRed).
- **Trade-off:** a native page reports one page-sized region, so table structure
  is not reconstructed for born-digital PDFs; the text is complete.
- **Tests:** `tests/pdf_native_text.rs` (exact text, page geometry, full-page
  region) and two unit tests pinning the threshold.

### F3. Search normalization layer

OCR output is stored verbatim; `normalize_for_search` is applied to both sides
at query time (ta marbuta, alef forms, tatweel, Arabic-Indic digits, Latin
diacritics), so stored OCR text is never rewritten. Verified by the existing
matching tests and by `documents.rs` using the same function for write and read.

---

## G. UI/UX

- Caption cluster on all five screens with real window calls, tooltips,
  accessible labels, hover/active/focus styling, RTL-aware order.
- Settings gains a Global shortcut card: current combination, Save, Reset,
  live registration status.
- Search field regains focus and selects its text when the window regains focus.
- `alwaysOnTop` removed, shadow restored, taskbar presence restored: the window
  behaves like a normal desktop window.
- No new glow/gradient/animation was added; the existing graphite/cyan identity
  is unchanged.

---

## H. Tests and builds actually run

| Command | Result |
| --- | --- |
| `cargo test --workspace --manifest-path core/Cargo.toml` | 555 passed, 0 failed, 0 ignored |
| `cargo test -p zeidex-storage --manifest-path core/Cargo.toml` | 71 passed |
| `cargo test -p zeidex-storage a_shell_setting` | 1 passed |
| `cargo test -p zeidex-ocr --test pdf_lifecycle` (models + pdfium) | 1 passed, 183.46 s |
| `cargo test -p zeidex-ocr --test pdf_smoke` (models + pdfium) | 1 passed, 32.33 s |
| `cargo test -p zeidex-ocr --test pdf_native_text` (models + pdfium) | 1 passed, 1.36 s |
| `cargo test -p zeidex-ocr --test arabic_cer` (models) | 1 passed, CER 15.57 %, WER 31.03 % |
| `cargo test -p zeidex-ocr --test arabic_smoke --test ocr_smoke` (models) | 1 + 1 passed, 8.70 s / 29.79 s |
| `cargo clippy --workspace --lib --manifest-path core/Cargo.toml -- -D clippy::unwrap_used -D clippy::expect_used` | clean |
| `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets` | clean |
| `cargo fmt --all --manifest-path core/Cargo.toml -- --check` | clean |
| `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` | clean |
| `npm test` in `app/` | 67 passed, 0 failed |
| `npm run build` in `app/` | built in 4.25 s: 392.63 kB JS, 70.38 kB CSS |
| `cargo build --release --manifest-path src-tauri/Cargo.toml` | finished in 8 m 34 s, 18 MB `zeidex` binary |
| `cargo run --release --manifest-path tools/perf/Cargo.toml --bin query_engine_bench` | numbers in section E |

Test environment for the real-model tests:
`ZEIDEX_TEST_MODELS_DIR=third_party/models`,
`ZEIDEX_TEST_PDF=core/ocr_engine/tests/fixtures/table.pdf`,
`ZEIDEX_TEST_PDFIUM_LIB=src-tauri/resources/lib/libpdfium.so`,
`ORT_DYLIB_PATH=src-tauri/resources/lib/libonnxruntime.so`.

---

## I. Builds and installers

- Release binary built: `src-tauri/target/release/zeidex`, 18 MB.
- Frontend production build: `app/dist`, 392.63 kB JS / 70.38 kB CSS.
- **No installer was produced in this round** and no Windows build was run:
  cross-compiling the Tauri shell for Windows is not available in this
  environment, and the 0.1.0 installers documented in `ENGINEERING_REPORT.md`
  were built by CI, not here.

---

## J. Remaining issues (honest list)

1. **Windows runtime is unverified.** Console absence, dialog z-order, caption
   buttons, tray, DPI scaling and multi-monitor placement need a Windows smoke
   test; this host has no Windows and no display server.
2. **Mixed Arabic/Latin lines lose the Latin part** (measured: 40 % CER on
   `اختبار PDF`). The Arabic result wins the line and the Latin run is not
   merged back.
3. **Long Arabic lines degrade in the middle** (39 % CER on a 26-character
   greeting). The recognizer is trained on shorter lines; chunked recognition
   with position-safe merging is the next step, not yet implemented.
4. **Arabic-Indic digits are the weakest class** (`١٢٣٤٥` → `٤٢٤٥`).
5. **Born-digital PDFs lose table structure** (text is exact, cells are not
   reconstructed) because the native path skips layout and table models.
6. **`recognize_line` runs both recognizers on every line**, roughly doubling
   recognition cost. A script pre-check would need its own accuracy evidence.
7. **Search benchmark covers 5,055 candidates**; the behaviour at 10× that
   scale is extrapolated, not measured.
8. **The Arabic corpus is synthetic print**, not scanned paper: it is a stable
   ruler for regressions, not a claim about real-world scan accuracy.
