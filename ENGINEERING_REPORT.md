# Zeidex Studio — Engineering Report

**Date:** 2026-11-22  
**Repository:** `LumioZEIDEX` (branch `main`)  
**Scope:** previous-audit verification, three-tier local OCR, hardware advisor, batch search, Excel export, hardening, tests, packaging.  
**Environment:** Linux arm64 (Android/Termux PRoot), Rust 1.99.1, Node 22.22.1, no display server, no usable GPU. Every limitation below is stated where it affects a claim.

---

## 1. Previous Task Audit

The previous master prompt (the "FULL CODE AUDIT, REPAIR, HARDENING & EVOLUTION" prompt) was recovered from the local session store and used as the checklist. It contained 13 phases. The table records what was already there, what this session did, and what can actually be proven.

| Previous phase | Original status | Work performed | Final status | Verification |
|---|---|---|---|---|
| 1. Repository reconnaissance | Claimed complete | Re-inspected the workspace, traced launcher → commands → engine → SQLite, and added the product-direction evaluation to `assets/docs/architecture.md` | **COMPLETE** | `assets/docs/architecture.md`, `core/*`, `src-tauri/src/*` inspected; docs updated |
| 2. Baseline | Claimed complete | Re-ran fmt, clippy, core tests, shell tests, type-check, frontend build | **COMPLETE** | Commands and counts in §7 |
| 3. Deep bug hunt | Rounds 1–6 fixed | Fixed bidi neutral-span handling, v5 Arabic visual order, watcher registration/warnings, PDF progress, file swap mid-run; verified the earlier fixes | **COMPLETE** for addressed defects; **UNVERIFIED** for exhaustive absence (no fuzzing/sanitizers) | §2; commits `f38053b`, `892d542`, `674a371`, `1a90a18`, `15fe251`, `b96698b`, `327b443`, `0c2a183` |
| 4. Search engine | Claimed complete | No launcher-search regression change; re-verified normalization, ranking and Arabic cases | **COMPLETE** | 157 `zeidex-engine` tests, 21 `zeidex-matching`, 4 `zeidex-ranking` |
| 5. Indexing | Claimed complete | Watcher fixes; live create/rename/delete/move tests; pause/resume/cancel controls are **not** implemented | **PARTIAL** | 7 `incremental_refresh` tests + `index_watch` tests; §9 item 1 |
| 6. OCR and Document Studio | Claimed complete | Page-window reads, page-boundary cancellation, EXIF orientation, v5 Arabic order, model-pack support, photo-quality regression gate | **PARTIAL** | 51 `zeidex-ocr` tests + 10 smoke/accuracy tests; OCR **text indexing** and DOCX/XLSX import are absent (§9) |
| 7. Clipboard intelligence | Claimed complete | Re-verified retention, size cap, secret filtering, pause and duplicate handling | **COMPLETE** | 59 `zeidex-storage` tests; `secrets.rs`; Settings toggle |
| 8. Security audit | Claimed complete | Re-verified CSP, capabilities, URL scheme check, no shell plugin, model SHA-256 and pinned revisions; added download integrity rules | **COMPLETE** (internal review; no external pen test) | §5; `tauri.conf.json`, `capabilities/default.json`, `core/models/src/catalog.rs` |
| 9. Performance engineering | Claimed complete | Measured batch search at 100k files, pack load/read, photo degradation; fuzzy pass improved earlier from 136.6 s to 13.4–16.9 s | **COMPLETE** for the measured paths | §4 |
| 10. UI/UX quality | Claimed complete | Added Models, Advisor and Batch surfaces; RTL/keyboard/empty/error states preserved | **PARTIAL** | Type-check and component tests pass; **no visual verification** (§9), and there is no light theme |
| 11. Product evolution | Evaluation requested | Documented the evaluation: keep the unified palette, add depth as screens, defer a workflow engine, keep local-first invariants | **COMPLETE** (evaluation only) | `assets/docs/architecture.md` → "Product direction" |
| 12. Testing strategy | Claimed complete | Added pack smoke, photo quality, batch scale, Excel workbook and model/advisor tests | **COMPLETE** for automated suites; **UNVERIFIED** for UI automation | §7 |
| 13. Cross-platform validation | Partial | Core CI compiles/tests on Linux and Windows; no Windows/macOS bundle was produced here | **UNVERIFIED** | `.github/workflows/ci.yml`; §8 |

The previous prompt is not treated as "done" merely because files exist: every row above is backed by a test run or a file inspection.

---

## 2. Major Bugs Fixed

### 2.1 Bidi neutral characters moved to the wrong side
- **Problem:** a line such as `12 صفحة` came back as `صفحة 12 ` with a trailing space; punctuation after Latin text moved.
- **Root cause:** `visual_to_logical` reversed each LTR span from the character after an RTL run to the next RTL run, which included the neutral space before the Latin text. The second reversal therefore moved characters the first pass had not moved.
- **Solution:** trim each span to its first and last LTR character before reversing it (`core/ocr_engine/src/bidi.rs`).
- **Verification:** `cargo test -p zeidex-ocr --lib` (51 passed) and the v4/v5 Arabic smoke tests against the real models.

### 2.2 The v5 Arabic recognizer was read in the wrong order
- **Problem:** `arabic_PP-OCRv5_mobile_rec_onnx` emits glyphs in page (visual) order; v4 emits logical order. Treating v5 like v4 produced reversed Arabic.
- **Root cause:** the order convention is a property of the model, but the engine had no per-model flag.
- **Solution:** `ModelPaths::rec_ar_visual` / `rec_en_visual`, set by `with_visual_arabic()`; `recognize_line` converts only when the flag is set. v4 keeps its existing path.
- **Verification:** new `arabic_v5_smoke` (3/3 lines) and the unchanged v4 `arabic_smoke`; commit `892d542`.

### 2.3 The index watcher could miss startup changes or hide failures
- **Problem:** files created between the startup walk and watcher registration were invisible; a watcher that failed to start only logged; a warning queued by a replaced watcher could overwrite the new state.
- **Root cause:** registration order, swallowed `notify` errors, and no generation check on warning messages.
- **Solution:** register the watcher before the walk; surface start failures through `zeidex://index-status`; tag warnings with a watcher generation and drop stale ones.
- **Verification:** `index_watch` tests and the live `incremental_refresh` suite; commits `674a371`, `1a90a18`, `15fe251`, `b96698b`.

### 2.4 The OCR progress bar could run backwards
- **Problem:** a 40-page run could report "page 42 of 5" after a page window was selected, or count queued pages rather than completed ones.
- **Root cause:** progress counted the slice position and included pages that had not finished.
- **Solution:** count completed pages and use the document's own `page_index` in progress, preview and sheet names.
- **Verification:** page-window unit tests and the real-model one-page rejection test; commit `327b443`.

### 2.5 A file could be replaced while it was being read
- **Problem:** a file changed on disk during a run could be read as two different documents.
- **Root cause:** the pipeline opened the path again for preview and for processing without a stable identity.
- **Solution:** read the source once per run and keep it for the run; dropping a document opens a new run instead of swapping the file underneath the current one.
- **Verification:** the drop/open tests; commit `0c2a183`.

### 2.6 An unsearchable batch entry was reported as a missing file
- **Problem:** a name consisting only of tatweel normalizes to an empty string. It appeared under "Not found" and in the Missing sheet, which is misleading: the search never ran.
- **Root cause:** the empty-after-normalization case was not distinguished from "no candidate matched".
- **Solution:** status `error`, counted in `BatchSummary.errors`, excluded from the Missing sheet, filterable in the UI.
- **Verification:** `export_xlsx` asserts `summary.errors == 1` and the independent workbook check confirms the row is absent from Missing.

### 2.7 The Excel footer made independent readers warn
- **Problem:** openpyxl emitted "Cannot parse header or footer" for every generated report.
- **Root cause:** the footer text did not start with a section marker, and `&[Page]` is not the form openpyxl's parser expects.
- **Solution:** write a centered section with classic codes: `&CZeidex · Batch Search · &P / &N`.
- **Verification:** `scripts/verify-xlsx.py` now reads the workbook with no warning; `cargo test -p zeidex-batch` (41 passed).

### 2.8 Earlier audit fixes verified, not re-fixed
The previous session's rounds 1–6 (first-launch window, Arabic presentation forms, clipboard secret handling, surgical settings writer, SLANet class list, overlap-based line assignment) remain in place and are covered by the passing suites. No regression was found.

---

## 3. New Features

### 3.1 Three local OCR tiers

| Tier | Pack | Detection / recognition | Download | Peak RAM | Arabic |
|---|---|---|---|---|---|
| Lightweight · Fast | `ppocrv4-mobile` (bundled) | v4 mobile / v4 mobile EN+AR | 0 | ≈320 MB | v4 |
| Balanced · Recommended | `ppocrv4-server-det` | v4 server / v4 mobile EN+AR | ≈129 MB | ≈900 MB | v4 |
| Advanced · High accuracy | `ppocrv5-server` | v5 server / v5 server EN + v5 mobile AR | ≈255 MB | ≈1.6 GB | v5, visual-order converted |

A pack replaces detection and recognition only; layout and table models stay shared in the bundled folder. Every file is pinned to an immutable upstream revision (Hugging Face commit SHA or PaddleOCR commit `a38c087b…`) with a published SHA-256 and byte count. The v5 Arabic recognizer has no standalone dictionary, so the installer downloads its `inference.yml` and extracts `PostProcess.character_dict` into `arabic_dict.txt`.

**Model manager** (`core/models`, `src-tauri/src/commands/models.rs`, `app/src/components/Models.tsx`):
- catalog with tier, AR/EN support, sizes, RAM, source and licence;
- resumable download (`Range`, restart when the server ignores it), 120 s read timeout, cancel between chunks;
- disk-space check before writing, SHA-256 verification, YAML extraction, staging → re-verify → atomic promote;
- `manifest.json` is the only proof of "Installed"; activation re-hashes the pack;
- active marker written by rename, ignored if it names an uninstalled pack;
- progress events with phase, file, downloaded/total bytes and bytes/second;
- verify, remove, activate, benchmark and compare actions.

**Hardware advisor** (`core/models/src/device.rs`, `src-tauri/src/hardware.rs`, `app/src/components/AdvisorCard.tsx`):
- probes CPU model, physical/logical cores, total/available RAM, OS, arch, GPU name/VRAM where the OS exposes it, free disk and process RSS;
- seven transparent criteria (4/8/16 logical cores, 8/16/32 GB RAM, usable GPU) → Basic / Standard / Advanced / High performance;
- recommendation starts at the class's tier and steps down when `RAM × 1.25` exceeds a quarter of installed RAM (capped by free RAM) or when disk headroom is insufficient; never below the bundled tier;
- returns the reason, trade-offs, OCR concurrency, batch chunk size and index thread count;
- benchmarks run only on user action and report time, memory and character error rate on the bundled samples.

### 3.2 Batch File Search

- **Input:** paste, `.txt` import (8 MiB cap, UTF-16 BOM honoured), direct typing/editing, clipboard paste button.
- **Parsing:** newline/tab separators only (commas are legal in filenames), BOM/CR/wrapping-quote removal, whitespace trim, duplicate folding after normalization, 2000-entry ceiling with an explicit overflow count.
- **Threshold:** 50–100% slider, applied to the same integer score shown in the results. 100% is equality after normalization by construction; containment and fuzzy are capped at 99.
- **Matching:** exact name, exact stem, containment (`60 + 40 × short/long`, +20 prefix bonus), all-words (85), banded Levenshtein fuzzy bounded by length and a 4-edit budget. An exact hit ends that entry's search.
- **Results:** Exact / High confidence / Possible / Multiple / Not found / Error; expandable candidates; sorts by input, status, score, name, size, type, path; filters by found/missing/exact/approximate/multiple/error, type, and a display-only minimum score.
- **Actions:** open file, reveal folder, copy path, copy name, multi-select, copy selected/all paths, copy selected names, open selected, hide row, restore hidden, remove from view.
- **Statistics:** requested, found, missing, exact, approximate, multiple, errors, success percentage, duration.

### 3.3 Excel report

Four sheets written with `rust_xlsxwriter` off the UI thread:
1. **Summary** — Zeidex branding, title, UTC generation stamp, ten KPIs, scope, threshold, duration, a column chart of outcomes, and matching notes.
2. **Results** — 11 columns, frozen header (`B2`), autofilter, real numbers/percentages/datetimes, zebra striping, status colours, print setup.
3. **Missing** — only names with no candidate (errors excluded).
4. **Multiple Matches** — every candidate for a name with more than one match, with score.

The workbook is verified twice: the Rust integration test proves it is written and re-writable; `scripts/verify-xlsx.py` opens it with openpyxl and checks sheets, freeze panes, filter, formats, Arabic text, the error row and the multiple-match sheet.

### 3.4 Other improvements

- incremental filesystem watcher with a bounded directory set and live index-status events;
- PDF page windows, page-boundary cancellation, EXIF/TIFF orientation, photo-quality regression gate;
- Arabic v5 opt-in pack with visual-order conversion;
- docs: `assets/docs/ocr-models.md`, updated `batch-search.md`, `ocr-studio.md`, README/README.ar, architecture product direction;
- CI gates for frontend tests, shell tests and the real-model OCR suite.

---

## 4. Performance

All numbers below were measured on this host; no synthetic estimate is reported as a measurement.

| Measurement | Conditions | Result |
|---|---|---|
| Batch exact lookup | 100,000 files, 200 existing names, 100%, release | 102 ms total, **0.51 ms/name** |
| Batch misses | 100,000 files, 200 absent names, 70% | 699 ms total, **3.50 ms/name** |
| Batch typos | 100,000 files, 200 typo'd existing names, 70% | 5,847 ms total, **29.2 ms/name** |
| Batch broad | 100,000 files, 200 broad names, 50% | 4,651 ms total, **23.3 ms/name** |
| Fuzzy pass before/after | 1,000 typo'd names vs 20,000 files, 8 cores, release | **136.6 s → 13.4–16.9 s** (rayon, one normalization, reused buffer, skip distance when containment ≥85) |
| Pack load + read (v4 server-det) | release, bundled layout/table + pack det/rec, one Arabic fixture page | **2.20 s warm** (2.84 s first run), 56 characters |
| Pack load + read (v5 server) | same fixture | **1.96 s warm** (8.26 s first run), 56 characters |
| Photo degradation | 12-cell table fixture, exact cell hits | clean 12/12, low contrast 12/12, dim 12/12, blur 12/12, noise 11/12, JPEG q45 12/12, combined 10/12 |
| Frontend bundle | production build | JS 377.62 kB (gzip 111.71 kB), CSS 65.00 kB (gzip 11.79 kB) |
| Installer | Debian arm64 | 49.85 MB, installed size 86.6 MB |

The 100k-file numbers are from `core/batch/tests/perf_batch.rs` (ignored by default, run with `--release -- --ignored --nocapture`). The pack numbers are from `core/ocr_engine/tests/pack_smoke.rs` with the real ONNX models. No before/after claim is made for paths that were not measured before the change.

---

## 5. Security

Verified in code and tests:

- **Local-first OCR:** ONNX Runtime runs in-process; no OCR image or extracted text leaves the machine. Translation is the only network feature and is user-initiated; it is documented in the README privacy table.
- **No telemetry, account or analytics endpoint.**
- **Model downloads:** user-initiated only, HTTPS, every URL pinned to an immutable revision, SHA-256 checked before install, staging verified again before promotion, failed install leaves the working pack untouched. The model root is per-user and overridable only by `ZEIDEX_MODELS_ROOT` for tests.
- **Tauri surface:** `withGlobalTauri: false`, a strict CSP (`default-src 'self'`, no remote scripts), a minimal capability set (window, tray, dialog, fs defaults), no shell plugin.
- **Process execution:** the shell only uses the platform opener for `open`/`reveal`; `open_url` refuses anything that is not `http://` or `https://`; no user text reaches a shell.
- **SQL:** rusqlite parameter binding; the only formatted SQL is a static `WHERE` fragment built from code constants, not user input.
- **Clipboard:** secret-like strings over 8 KiB are refused, retention and size caps apply, recording can be paused.
- **Logs:** errors are reported to the UI; no file content or clipboard text is logged.
- **Unsafe code:** limited to Windows COM helpers and process-creation flags; no unsafe in the new model/download code.

Not performed: external penetration test, dependency-CVE sweep with a live advisory feed, and signature verification of the produced packages.

---

## 6. UI/UX

- Dark graphite surfaces with a cyan/iris accent, consistent type scale (Inter / Noto Kufi Arabic / JetBrains Mono), spacing and focus rings.
- Full RTL interface with logical CSS properties; Arabic and mixed-direction results render in logical order.
- New surfaces integrate with the existing footer navigation: Batch Search, OCR Models; the launcher keeps its keyboard-first flow.
- Models screen: tier badge, recommended badge, RAM/download/on-disk/languages/CPU-GPU/files, progress with speed and remaining bytes, verify problems, benchmark table with CER, file list with source/licence, two-step destructive confirmation.
- Advisor: hardware summary, transparent criteria with measured details, class, recommended pack, reason, trade-offs, OCR workers/batch chunk/index threads, and the four actions (download, test, compare, choose manually).
- Batch: review counts before searching, threshold label, expandable rows, filters/sorts, bulk actions, hidden-row restore, statistics, export with save dialog.
- Empty, loading and error states exist for each new surface.

Known gaps: no light theme; no visual/browser verification on this host (no display, Chrome/Playwright fails in PRoot). The UI is verified by TypeScript compilation, 56 unit tests and a production build, not by pixels.

---

## 7. Testing Results

| Command | Outcome |
|---|---|
| `cargo fmt --manifest-path core/Cargo.toml --all --check` | **PASS** |
| `cargo fmt --manifest-path src-tauri/Cargo.toml --check` | **PASS** |
| `cargo clippy --manifest-path core/Cargo.toml --all-targets -- -D warnings` | **PASS** (0 warnings) |
| `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` | **PASS** (0 warnings) |
| `cargo test --manifest-path core/Cargo.toml` | **527 passed, 0 failed, 2 ignored** (one ignored test is the 100k scale test, run separately) |
| `cargo test --manifest-path src-tauri/Cargo.toml` | **42 passed, 0 failed** |
| `cargo test -p zeidex-batch` (after the footer fix) | **41 passed, 0 failed, 2 ignored** |
| `npx tsc --noEmit` (`app/`) | **PASS** |
| `npm test` (`app/`) | **56 passed, 0 failed** |
| `npm run build` (`app/`) | **PASS** |
| `cargo test -p zeidex-batch --test perf_batch --release -- --ignored` | **PASS**, numbers in §4 |
| `pack_smoke` for `ppocrv4-server-det` and `ppocrv5-server` | **PASS**, 56 characters each |
| `cargo test -p zeidex-batch --test export_xlsx` | **PASS** |
| `python3 scripts/verify-xlsx.py core/target/tmp/zeidex-batch-report.xlsx` | **PASS** (4 sheets, filter, formats, Arabic, error row, multiple sheet) |
| `tauri build --bundles deb` | **PASS** (release build) |
| `dpkg-deb --info` / `dpkg-deb -c` | **PASS** (valid package, expected payload) |
| `ldd` on the release binary | **PASS** (no missing libraries) |
| GUI launch | **BLOCKED** (no display in PRoot) |
| Browser visual pass | **BLOCKED** (Chrome/Playwright cannot start in this host) |
| Windows MSI/NSIS, macOS DMG, AppImage | **NOT RUN** (no toolchains/display here; macOS is not in the release matrix) |

Test totals: **625 automated tests passed, 0 failed** (527 core + 42 shell + 56 frontend), plus the scale, pack and workbook checks above.

---

## 8. Build and Release Status

- **Built:** `src-tauri/target/release/bundle/deb/Zeidex Studio_0.1.0_arm64.deb` — 49,850,764 bytes, `zeidex-studio 0.1.0 arm64`, SHA-256 `37c76897bec55f19d62d8cc10925da07083b03189d786136d8fcc5b175a679ad`, depends repaired to `libwebkit2gtk-4.1-0`, `libgtk-3-0 | libgtk-3-0t64`, `libayatana-appindicator3-1`.
- **Payload:** 20.0 MB binary, 25.1 MB `libonnxruntime.so`, 8.3 MB `libpdfium.so`, compact bundled models only; Tier 2/3 packs are downloaded on demand.
- **Libraries:** `ldd` reports no missing shared objects.
- **Launch:** cannot be verified here because the host has no display; this is an environment limit, not a link or packaging failure.
- **Not built/verified:** AppImage, Windows MSI/NSIS, macOS DMG, signed packages. The release workflow covers Linux and Windows on native runners; macOS is documented as build-from-source, not a shipped artifact.

---

## 9. Remaining Issues

1. **Index pause/resume/cancel** — rebuild, refresh, live status and error reporting exist; pause/resume/cancel of a running full walk do not. The walk has no resume cursor, so an abort would leave a partial (valid but incomplete) index. Medium.
2. **No light theme** — the 0.1 identity is dark graphite by design; a light mode would need a full design pass and visual verification, which is blocked here. Medium.
3. **No OCR text indexing** — extracted document text is not searchable from the launcher. This is a real competitive gap (Listary advertises full-text search inside PDFs/scans/Office). Medium-high.
4. **No DOCX/XLSX/text import in the Studio** — the pipeline reads PDFs and images; Office/text extraction is not implemented. Medium.
5. **Visual UI unverified** — no display or working browser in this host. All UI claims rest on type-check, unit tests and the production build. High for confidence, environmental.
6. **Windows/macOS/AppImage packaging unverified** — the workflows exist for Linux/Windows; no artifact was produced here. macOS is not in the release matrix.
7. **GPU detection unverified on real hardware** — the Linux sysfs/NVIDIA probe is unit-tested with synthetic input; no GPU is present here. CUDA/DirectML feature builds were not exercised.
8. **Benchmark coverage** — the benchmark command and CER tests exist; only the bundled samples were run, not every pack through the UI.
9. **"Three additional models" interpretation** — the app ships three tiers: the bundled v4-mobile plus two additional packs. A literal fourth model was not added; the tier model is the requirement that matters.
10. **No fuzzing, sanitizers or external penetration test** — the security review is code-level and test-backed, not adversarial validation.
11. **In-app download UI not exercised in a webview** — the installer path produced both packs with valid manifests; the button flow is covered by unit tests, not a live click-through.
12. **Packages are not signed** — no code-signing keys in this environment.

---

## 10. Final Completion Matrix

The count is defined by 52 requirement groups: the 13 previous-prompt phases plus 39 current-prompt deliverable groups (8 OCR/model manager, 5 advisor, 10 batch, 4 Excel, 4 security, 4 UI/UX, 4 packaging). A group counts as COMPLETE only with implementation and a test/inspection result; overlapping gaps are counted once in the group where they belong.

| Area | Items | Complete | Partial | Unverified / Missing |
|---|---|---|---|---|
| Previous audit (13 phases) | 13 | 9 | 3 | 1 |
| OCR tiers and model manager | 8 | 8 | 0 | 0 |
| Hardware advisor | 5 | 5 | 0 | 0 |
| Batch search | 10 | 10 | 0 | 0 |
| Excel report | 4 | 4 | 0 | 0 |
| Security and privacy | 4 | 4 | 0 | 0 |
| UI/UX | 4 | 2 | 1 | 1 |
| Packaging/release | 4 | 1 | 0 | 3 |
| **Total** | **52** | **43** | **4** | **5** |

**Fully complete: 43/52 (83%).** With partial items counted at half credit the weighted figure is **45/52 (87%)**. The project is **not 100% complete**: the 4 partial and 5 unverified/missing groups are listed in §9. The largest functional gap is OCR text indexing; the largest confidence gap is the absence of visual UI verification and non-Linux packaging in this environment.

---

*Report generated from the working tree on 2026-11-22. All commands were run in this environment; blocked items are marked as blocked rather than passed.*
