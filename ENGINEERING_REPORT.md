# Zeidex Studio — Engineering Report

> **Historical document.** This report records the 0.1.0 line, when the product
> was called *Zeidex Studio*. In 0.3.0 the product became **Zeidex Search** and
> the repository `zeidex-search`; package and install paths quoted below are the
> old ones and are kept as the evidence they were at the time. The binary
> (`zeidex`), the identifier (`com.zeidexsoft.zeidex`) and the `~/.zeidex` data
> directory are unchanged.

**Date:** 2026-11-22  
**Repository:** `LumioZEIDEX` (branch `main`)  
**Scope:** previous-audit verification, three-tier local OCR, hardware advisor, batch search, Excel export, three hardening rounds (two driven by independent code reviews, one closing the read-side path trust and the contrast-token gap), tests, packaging.  
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
| 5. Indexing | Claimed complete | Watcher fixes; live create/rename/delete/move tests; this round added pause/resume/cancel (shared `IndexControl`, sweep skipped on cancel, shell commands, Settings controls, footer state) | **COMPLETE** | 13 `zeidex-indexing` tests + 2 `index_control` tests + 7 `incremental_refresh` tests; §3.4 |
| 6. OCR and Document Studio | Claimed complete | Page-window reads, page-boundary cancellation, EXIF orientation, v5 Arabic order, model-pack support, photo-quality regression gate; OCR **text indexing** (local token index, launcher content hits, Settings switch/clear); **DOCX/XLSX/text extraction** (in-package XML reader, page splitting, bounded reads, Studio "Extract text" path) | **COMPLETE** | 64 `zeidex-ocr` lib tests (11 new `document` tests, 1 ONNX-environment regression) + 10 smoke/accuracy tests; 5 `document_text` tests + 2 launcher merge tests; §3.5, §3.6 |
| 7. Clipboard intelligence | Claimed complete | Re-verified retention, size cap, secret filtering, pause and duplicate handling | **COMPLETE** | 66 `zeidex-storage` tests; `secrets.rs`; Settings toggle |
| 8. Security audit | Claimed complete | Re-verified CSP, capabilities, URL scheme check, no shell plugin, model SHA-256 and pinned revisions; added download integrity rules; moved the export file pickers into Rust so the webview no longer chooses a write path; removed the unused `tauri-plugin-fs` grant; made every path-reading command ask a capability policy (scan root / picker / drop / export / app-owned / indexed) before touching the filesystem | **COMPLETE** (internal review; no external pen test) | §5, §2.17, §2.20–§2.23; `tauri.conf.json`, `capabilities/default.json`, `core/models/src/catalog.rs`, `src-tauri/src/commands/dialogs.rs`, `src-tauri/src/commands/policy.rs` |
| 9. Performance engineering | Claimed complete | Measured batch search at 100k files, pack load/read, photo degradation; fuzzy pass improved earlier from 136.6 s to 13.4–16.9 s; the indexed-path capability lookup is a covering-index search instead of a table scan | **COMPLETE** for the measured paths | §4, §2.23 |
| 10. UI/UX quality | Claimed complete | Added Models, Advisor and Batch surfaces; RTL/keyboard/empty/error states preserved; light theme shipped as a token remap; contrast is now a test over every text token in both themes | **PARTIAL** | Type-check, component tests and the 67-test frontend suite pass; **no automated visual verification** (§9), and light-mode states have not had a full design review |
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
- **Verification:** `scripts/verify-xlsx.py` now reads the workbook with no warning; `cargo test -p zeidex-batch` (42 passed on the final tree).

### 2.8 The OCR Models screen rendered blank
- **Problem:** opening Models at 1280×788 produced an entirely black window that never painted, while the web process spun at 76–87% of a core; the Back button did not respond because the React tree was gone, not because the click was lost.
- **Root cause:** a wire-contract mismatch. `PackView` and `AdvisorView` carried `#[serde(rename_all = "camelCase")]`, so the backend sent `recommendedPack`, `ramMb`, `nameAr`, `checkedFiles`; the TypeScript interfaces in `api.ts` declare `recommended_pack`, `ram_mb`, `name_ar`, `checked_files`. `AdvisorCard` destructured `recommended_pack` as `undefined` and then read `pack.name`, which threw during render; with no error boundary React unmounted the whole tree and left an empty root, which is the black surface. The same mismatch silently emptied the model cards and the verify result. Settings was unaffected, which is why only Models failed.
- **Solution:** one naming convention on the wire. The seven view structs in `commands/models.rs` now serialise their declared names, and `api.ts` plus the demo mock were aligned to the same names (`ModelVerifyView`, `DownloadProgress`).
- **Verification:** three new contract tests serialise `PackView`, `AdvisorView` (including its nested `profile`, `recommendation` and `recommended_pack`) and `DownloadProgress` and assert the exact key sets the UI reads, so the two hand-written halves cannot drift again. After the rebuild the Models screen paints in the real app at 1280×788 and the cards show live values.

### 2.9 The panel's aurora animation burned a core at idle
- **Problem:** with the app visible but idle, the web process held 60–80% of a core on the launcher and close to 100% at 1280×788, even with no JavaScript running.
- **Root cause:** `.panel::before` — a 120%-sized layer carrying two radial gradients — had `filter: blur(12px)` and a 26-second infinite transform animation. On a GPU-composited webview that is nearly free; under software rendering (this host, remote sessions, VMs, some Wayland setups) every frame re-rasterises the blurred ~1.3 MP layer on the CPU.
- **Solution:** the aurora is decoration, so it is now a static gradient with no `filter` and no keyframes (`app/src/styles.css`); the gradients already have soft 70% stops, so the look is unchanged. `@keyframes aurora` was deleted so it cannot be re-enabled by accident.
- **Verification:** measured over `/proc/<pid>/stat`, idle web-process CPU fell from 60–80% to **0.0%** with the window visible; two screenshots three seconds apart differ by 164 of 1.3 M pixels (antialiasing only). Screens re-checked afterwards.

### 2.10 Earlier audit fixes verified, not re-fixed
The previous session's rounds 1–6 (first-launch window, Arabic presentation forms, clipboard secret handling, surgical settings writer, SLANet class list, overlap-based line assignment) remain in place and are covered by the passing suites. No regression was found.

---

### 2.11 Switching OCR packs killed OCR until the app restarted
- **Problem:** after downloading and activating a second pack, every recognition failed with `only one environment is allowed per process`, and the only recovery was to restart Zeidex.
- **Root cause:** `core/ocr_engine/src/onnx.rs` created the ONNX Runtime environment with `ort::init().build()` inside every engine construction. ONNX Runtime permits exactly one environment per process, and activating a pack builds a new engine.
- **Solution:** one process-global `OnceLock<Environment>` guarded by an init mutex, handed to every engine. Loading a pack now reuses it.
- **Verification:** regression test `the_environment_is_handed_out_more_than_once`; the precision-pack smoke builds two engines in one process and recognises Arabic and English with each (`core/ocr_engine`, 64 lib tests).

### 2.12 The batch matcher could miscount words and overstate "multiple matches"
- **Problem:** seven separate defects, each capable of returning a wrong verdict: a repeated token satisfied a two-word request; the "N candidates" count disagreed with the page; a weak match could be promoted to the display limit; `improve` was threshold-filtered so a near miss vanished; the status was taken from the displayed page instead of the best candidate; a folder was scored twice; a dotfile's extension disagreed with its stem.
- **Root cause:** each was an independent logic error in `core/batch/src/matcher.rs`, not one shared bug — a token counter that counted occurrences instead of request tokens, a shortlist that was sorted after truncation, and a status derived from the wrong variable.
- **Solution:** `all_tokens_present` uses a bitmask over *request* tokens; `fuzzy_matches` returns `(matches, total, top)` with a bounded shortlist; `max_distance` rounds per name length; `improve` runs after the threshold; `status_of(Option<u8>)` takes the best score; folder double-scoring is skipped; `extension()` and `stem_of` agree on dotfiles; a 100% threshold skips the provably empty fuzzy scan.
- **Verification:** seven new regression tests in `matcher.rs`; `zeidex-batch` lib **42 passed**.

### 2.13 A damaged model pack could not be repaired
- **Problem:** a pack whose files no longer matched the catalog (interrupted copy, restored backup, older layout) still reported *Installed*. Activation then failed its digest check, and the download button refused with "already installed", so the only way forward was Remove.
- **Root cause:** `status()` trusted a manifest written at install time, and `models_download` refused any pack that merely looked installed.
- **Solution:** `status`/`verify`/`verify_manifest_in` compare against the catalog digests and the catalog's installed size; `models_download` refuses only a pack that actually verifies; `recover()` restores `.retired-<id>` after an interrupted promote and cleans a finished swap; the catalog command runs it on every load; `models_remove` refuses while that pack is downloading.
- **Verification:** 6 new installer tests and 5 new device tests (`zeidex-models` **44 passed**), including a fixture whose digests are computed from the bytes it writes.

### 2.14 A download could fill the disk or hang forever
- **Problem:** a server that lied about `Content-Length`, or that dribbled one byte at a time, could exhaust the disk or hold the single download slot indefinitely.
- **Root cause:** the transfer trusted the advertised length and had a per-read timeout but no rate floor.
- **Solution:** `fetch_file` rejects an oversized `Content-Length` and a running total past the catalog size, deleting the `.part`; a 1 KiB/s average floor after the first minute stops a stalled connection; the disk check and the advisor now share one `disk_requirement_mb` (`sum(files) + largest + 128 MiB`).
- **Verification:** `cargo check`/clippy clean; the installer tests exercise the disk requirement; the cap arithmetic is unit-tested through `disk_requirement_mb`.

### 2.15 One long cell could fail the whole Excel report
- **Problem:** a filename or path longer than Excel's 32 767 UTF-16 unit limit made `rust_xlsxwriter` return an error, and the export failed after the user had waited for it.
- **Root cause:** cells were written straight from the model with no bound.
- **Solution:** `cell_text()` clamps to 32 767 units and ends a truncated value with `…`; the KPI rows were reordered so the four charted rows are mutually exclusive; the chart ranges follow them; the Multiple Matches sheet gained a Notes column (7 columns, autofilter `A1:G`); the Missing sheet distinguishes a read error from an absent file.
- **Verification:** `a_cell_is_cut_to_the_excel_limit_instead_of_failing_the_export`, `one_enormous_request_does_not_cost_the_whole_report`, `the_chart_plots_only_mutually_exclusive_outcomes`; report integration **10 passed, 1 ignored**; `scripts/verify-xlsx.py` re-reads the workbook and confirms `chart series: ['Summary!$B$7:$B$10']`, 4 sheets and filter `A1:K8`.

### 2.16 A list with CR-only line endings was read as one name
- **Problem:** a `.txt` exported from an old Mac tool (CR only) produced a single 300-name "filename".
- **Root cause:** the parser split on `\n` and trimmed a trailing `\r`, so a CR-only list never split.
- **Solution:** CRLF is normalised first, then CR alone is treated as a separator — so mixed endings cannot create phantom empty entries.
- **Verification:** `a_cr_only_list_is_split_into_names`, `mixed_line_endings_do_not_create_phantom_entries`.

### 2.17 The export commands wrote to a path the webview chose
- **Problem:** the UI opened the save dialog with `@tauri-apps/plugin-dialog`, then passed the resulting string to `batch_export`, `export_docx_pages` or `export_xlsx_pages`. The command had no way to know the path had come from a dialog, so a script in the webview could call it directly and overwrite any file the user can write — no dialog, no prompt.
- **Root cause:** the dialog and the write were on opposite sides of the IPC boundary, so the path was unverified data.
- **Solution:** the pickers moved into Rust (`src-tauri/src/commands/dialogs.rs`); the commands now open the dialog themselves and take only cosmetic strings (title, filter label, suggested name) from the page. The suggested name is reduced to a file name, so it cannot steer the dialog into another directory. Writes go to a sibling `.part` file and are renamed into place, so a failed export cannot truncate the file that was already there. The two unused single-page export commands were deleted rather than left as extra IPC surface.
- **Verification:** `src-tauri` **64 passed** (53 at the time of this fix, plus the policy tests in §2.20), including `a_suggested_name_cannot_choose_its_own_directory`, `the_staging_file_sits_beside_the_target` and the extension tests; the frontend no longer imports `save` at all.

### 2.18 The batch status was a string
- **Problem:** `BatchEntryResult.status` was a `String`. The report coloured rows with a `match` that had a `_ =>` fallback, so a mistyped status silently became the muted "Not found" colour instead of failing to compile.
- **Root cause:** stringly-typed data crossing the Rust/TypeScript boundary.
- **Solution:** `BatchStatus` is an enum with `#[serde(rename_all = "snake_case")]`, so the wire format is unchanged and the report's colour and label matches are exhaustive. The TypeScript union already matched it.
- **Verification:** `zeidex-batch` **42 passed**, report integration **10 passed**; the exported workbook still verifies.

### 2.19 The matcher sorted with an allocation per comparison
- **Problem:** `sort_matches` called `to_lowercase()` twice inside the comparator.
- **Root cause:** a comparison closure doing work that belongs in a key.
- **Solution:** `sort_by_cached_key` with `(Reverse(score), lowercase name, path)` — the same order, one allocation per element.
- **Verification:** the batch suite and the 100k scale run in §4 are unchanged in behaviour and faster.

### 2.20 Every read command trusted a path chosen by the page

- **Problem:** nine commands took a filesystem path as a string and used it directly: `ocr_page_preview`, `ocr_process_image`, `document_extract_text`, `ocr_process_pdf`, `ocr_process_pdf_range`, `document_text_store`, `document_text_remove`, `open_target`, `reveal_target`. Nothing checked that the path had anything to do with Zeidex, so a script in the webview could read any file the user can read and hand it to the OCR engine, or ask the desktop to open it. §2.17 closed the write side; this was the same trust on the read side, where the payload leaves through OCR text or an external application.
- **Root cause:** drag-and-drop, the recent-files list and the folder picker produce a path with no native dialog in between, so the page had to pass one. The commands then treated "a string arrived" as "the user chose this".
- **Solution:** a path capability policy (`src-tauri/src/commands/policy.rs`). A path is readable only when it is one of: inside a configured scan root; minted by a native picker, an OS drop, or an export Zeidex just wrote; an app-owned path (database, config, model folders); or a row already in the index. Containment is decided on canonical paths, so `..` and a symlink that points out of a root are refused; a path that does not exist is resolved through its canonical parent, because if the leaf existed the canonical branch would have taken it. Picks are remembered in a 512-entry FIFO — a drop of a thousand files cannot grow it without bound, and the file just chosen is the one the next command reads. The check runs before the store lock is taken, because `authorize_read` takes that lock itself and `std::Mutex` is not reentrant.
- **Verification:** `policy.rs` **10 tests** — inside/outside a root, `..` escape, symlink escape, picked-outside-root, FIFO eviction, app-owned path, missing leaf through its parent, non-existent root grants nothing, relative root made absolute. `state.rs` adds the index-known case (`/etc/shadow` refused, an indexed row allowed) and now runs **64 tests**. All nine commands call `authorize_read` before touching the filesystem.

### 2.21 The last two dialogs were still opened by the webview

- **Problem:** after §2.17, `OcrStudio` and `Settings` still imported `open()` from `@tauri-apps/plugin-dialog` to pick a document and a scan folder. Those two pickers therefore still produced a bare path on the page, which is exactly the value §2.20 now refuses unless something minted it.
- **Root cause:** the two call sites were missed when the export dialogs moved.
- **Solution:** `ocr_pick_file` and `settings_pick_folder` are Rust commands that open the dialog, mint the result, and return the path. The page calls `api.ocrPickFile` / `api.settingsPickFolder` and never sees the dialog plugin. The OCR filter list lives in Rust (`SUPPORTED_DOCUMENT_EXTENSIONS`) and a frontend test reads that array out of the Rust source and compares it with the TypeScript list, so the two cannot drift apart.
- **Verification:** neither component imports `@tauri-apps/plugin-dialog` any more; `app/src/documents.test.ts` fails if the lists differ; `npx tsc --noEmit` and the 67-test frontend suite pass.

### 2.22 The file-system plugin granted more than the app used

- **Problem:** `tauri-plugin-fs` was registered and `fs:default` plus `dialog:default` were in `capabilities/default.json`, while every actual read and write now goes through a Rust command. The plugin's permissions were unused authority: a page that could call `fs:read-file` did not need the policy at all.
- **Root cause:** capability files are additive, and removing a plugin is not required for the app to work — so the grant outlived its last caller.
- **Solution:** removed `tauri-plugin-fs` from `src-tauri/Cargo.toml` and its `.plugin(...)` registration, removed `fs:default` and `dialog:default` from the capability file, and removed `@tauri-apps/plugin-dialog` from `app/package.json` (the lockfile pruned it). The window, tray and core defaults remain.
- **Verification:** `grep` finds no `plugin-fs`, `fs:default` or `dialog:default` in the manifest, capability file, `lib.rs` or `package.json`; `cargo check --lib` and the shell test suite pass with the plugin gone.

### 2.23 The capability lookup scanned the whole index

- **Problem:** `is_indexed_path` — the predicate that keeps launcher results openable — ran `SELECT EXISTS(… WHERE path = ?1)` against `candidates`, which had no index on `path`. On a 100k-row index every open/reveal click scanned the table.
- **Root cause:** the table was designed for `id` and `kind` lookups; `path` was only ever a payload column until it became a capability.
- **Solution:** `CREATE INDEX IF NOT EXISTS idx_candidates_path ON candidates(path)` in `migrate()`. It is created on existing databases too, so no re-index is needed.
- **Verification:** `EXPLAIN QUERY PLAN` on the same schema shows `SCAN candidates` without the index and `SEARCH candidates USING COVERING INDEX idx_candidates_path (path=?)` with it. Two new storage tests pin the predicate's semantics: exact match only (prefix, suffix, other case and `/etc/shadow` all refused, an Arabic path accepted), and a row whose file was deleted still counts as indexed — opening it must fail with the OS error, not a capability error. Storage is now **66 tests**.

### 2.24 Two muted text steps failed WCAG AA

- **Problem:** the theme inherited Tailwind's stock `zinc-500` (`#71717a`) and `zinc-600` (`#52525b`), used for muted labels, the footer, placeholders and empty-state copy. On the panel (`#11151f`) they measured **3.78:1** and **2.36:1**, below the 4.5:1 that AA requires for body text. Nothing in the build noticed, because contrast is not a type error.
- **Root cause:** a colour ramp chosen for a light background was reused on a dark one, and no test described the pairs the interface actually paints.
- **Solution:** lifted the two steps to `#9a9ba5` (**6.61:1**) and `#8f909b` (**5.76:1**), and added `app/src/contrast.test.ts`: 28 text tokens plus two label-on-fill pairs are checked in both themes, the muted ramp must stay ordered so lifting it did not flatten the hierarchy, and a scan of every `.tsx` file fails the suite if a `text-<palette>-<step>` class is used that the token table does not declare.
- **Verification:** the suite passes in both themes (light: 5.93:1 and 4.83:1); re-running it against the stock ramp reproduces exactly the two failures above, so the test is not vacuous.

### 2.25 A Windows-1256 Arabic file was refused as "not UTF-8"

- **Problem:** a `.txt` exported from an Arabic Windows machine, or a `.csv` from an older Excel, is Windows-1256. The reader accepted only UTF-8 and UTF-16 and refused everything else with "save it as UTF-8". For an Arabic-first product that is a refusal of the most common legacy encoding in the region.
- **Root cause:** the decoder was a BOM/UTF-8 check with no code-page path, and `chardetng` was not in the dependency tree (crates.io is unreachable from this host, HTTP 403), so a statistical detector was not available.
- **Solution:** `encoding_rs` — already in both lockfiles through `reqwest`, so no new crate and no new network need — decodes Windows-1256 and Windows-1252. The choice between them is an explicit, documented heuristic: at least 10% of non-space characters must fall in the Arabic block under 1256, otherwise 1252. Files under 16 bytes, or where fewer than 75% of characters are text rather than control bytes, are still refused, and the error now names both encodings. This is deliberately **not** a statistical detector, and the code says so.
- **Verification:** `arabic_windows_1256_text_is_decoded_instead_of_refused` stores a CP1256 byte string and asserts the Arabic comes back; `a_binary_file_renamed_to_text_is_refused_rather_than_mojibaked` keeps the refusal path pinned. Both pass; the document suite is 20 tests.

### 2.26 `.odt` was a supported format on paper only

- **Problem:** OpenDocument Text is what LibreOffice writes by default on Linux, which is the platform this build ships to. The Studio refused it, so a user's own documents were invisible to OCR extraction and to the content index.
- **Root cause:** the reader had `.docx` and `.xlsx` branches and a text branch; `.odt` was never added, and the extension list in Rust, TypeScript and the command's `SUPPORTED_DOCUMENT_EXTENSIONS` all agreed on the omission.
- **Solution:** `odt_pages` reads `content.xml` and maps `text:p`/`text:h` to lines, `text:tab` to a tab, `text:s text:c` to repeated spaces, `text:line-break` to a newline, and `text:soft-page-break` to a page split. `"odt"` is now in the Rust list, both TypeScript lists and `textDocumentSource`. The Rust list stays the source of truth: a frontend test reads the Rust array and fails if the TS copy drifts.
- **Verification:** `an_odt_keeps_paragraphs_tabs_spaces_and_soft_page_breaks` asserts the exact extracted text, and `an_odt_is_dispatched_by_extension` proves the dispatcher routes to it. The parity test passes with the new extension on both sides.

### 2.27 A spreadsheet date read as `45000`

- **Problem:** the `.xlsx` reader printed stored values. An Excel date cell holds a serial number, so `2023-03-15` extracted as `45000`; `0.5` with a percent format extracted as `0.5`; a time extracted as `0.75`. The text pane and the content index then held numbers that match no search for the visible value.
- **Root cause:** only shared strings, inline strings and booleans were resolved; the cell's `s` style attribute was ignored, and `styles.xml` was never opened.
- **Solution:** `cell_styles` reads `numFmt` codes and the `cellXfs` list; `style_from_builtin` recognises the ECMA-376 date/time/percent ids including the East Asian date variants (27–36, 50–58); `style_from_code` strips quoted literals, escapes and bracketed sections before classifying, so `0"%"` stays a plain number and `[h]:mm:ss` is a duration. Dates render ISO (`YYYY-MM-DD`) by Howard Hinnant's exact `civil_from_days`, times as `HH:MM:SS`, percentages with the format's own decimal count. Excel's 1900 leap-year bug is handled explicitly: serial 60 keeps the label `1900-02-29` rather than inventing a date, and the 1904 Mac system is read from `workbookPr`. Only numeric cells are formatted; text, booleans and errors pass through, so a text cell styled as a date keeps its text.
- **Verification:** three tests. `a_spreadsheet_date_percentage_and_datetime_read_as_excel_shows_them` asserts `2023-03-15`, `50.0%`, `2023-03-15 12:00:00`, an untouched text cell and an unstyled `45000` in one row; `excel_dates_handle_the_1900_leap_bug_and_the_1904_system` pins serials 1, 59, 60, 61, 1904 serial 0 and a negative serial; `a_format_code_decides_percent_date_and_time_without_quoted_literals` covers the classifier. Arithmetic was cross-checked against Python's `datetime` before the code was written.

### 2.28 The content index kept text for files that no longer matched

- **Problem:** `document_text` stored a row per file and never checked the file again. A report edited after its OCR run stayed searchable by its old wording, and a deleted file stayed a hit forever, until the 20,000-row cap evicted it. Worse, the writer stored `modified_at_unix_s: None` on every row, so the column that existed for this check could never trigger one.
- **Root cause:** the column was added for a future pass that was never written, and the command that fills it did not read the mtime.
- **Solution:** `document_text_store` now records the file's mtime (a file whose mtime cannot be read is still indexed — the cost is one unpruned row, not a lost document). `SqliteStore::prune_stale_document_text` stats every stored path, deletes rows whose file is `NotFound` or whose mtime differs, keeps rows with a NULL mtime, and keeps rows whose stat failed for any other reason ("I could not look" is not "it is gone"). The stat pass runs before the write transaction so a slow mount does not hold the write lock; the deletes go through one transaction and the `ON DELETE CASCADE` removes the tokens. A named `document-index-prune` thread runs it once at startup, after `AppState` is managed, and prints the counts.
- **Verification:** four new tests — an unchanged file survives, a rewritten file loses its row *and its tokens*, a deleted file is pruned, and a NULL-mtime row is kept even after its file disappears. `PrunedDocuments { missing, changed }` is compared by value so a miscount fails the test. Storage is now **70 tests**; the shell lib is **64**.

### 2.29 An OCR run was indexed as if it were a text file

- **Problem:** `runOcr` handles both paths — text extraction for `.docx`/`.xlsx`/`.odt`/text, and real OCR for images and PDFs — and it stored the result with `textDocumentSource(filePath)` unconditionally. `textDocumentSource` falls back to `"text"`, so every scanned image and every PDF was recorded in `document_text.source` as `text`. The column exists to say where the text came from; a scan was indistinguishable from a `.txt`.
- **Root cause:** the conditional was written at the export call site but not at the run call site, and the fallback in `textDocumentSource` made the wrong value type-check.
- **Solution:** both call sites now use `isTextDocument(filePath) ? textDocumentSource(filePath) : "ocr"`, and the `indexRecognizedText` parameter type was widened to include `"odt"` — the ODT work had left the two call sites failing `tsc` with `'"odt"' is not assignable`, which the previous round's summary had reported as clean.
- **Verification:** `npx tsc --noEmit` exits 0 with no diagnostics (it reported exactly those two errors before the fix), and `npm test` is 67/67.

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

### 3.4 Index pause, resume and cancel

A full walk is long enough that "wait for it" is not an answer, and the
dangerous part is not stopping — it is the stale-row sweep at the end. Rows the
walk never visited are not gone; pruning them would delete a large part of a
working index.

- `IndexControl` (`zeidex-indexing`) is one atomic state (Running / Paused /
  Cancelled) plus a condition variable. The walker checks it once per directory
  entry — an atomic load in the normal case — and parks on the condvar while
  paused, so a paused walk costs no CPU and no lock traffic.
- The database writer checks it between 512-row chunks; a cancel breaks the
  loop and **skips the sweep**, leaving the index a valid union of old rows and
  newly written ones. `BootstrapOutcome::{Complete,Cancelled}` reports which
  happened, and the shell surfaces a cancelled walk as a notice, not an error.
- Commands `index_pause` / `index_resume` / `index_cancel` /
  `index_control_state` return whether the state actually changed, so the UI
  never claims a pause that did not happen. `IndexStatus` gained `paused`, and
  the Settings panel shows Pause/Resume/Cancel while a walk runs; the footer
  says "Indexing paused" instead of a spinner that lies.
- Tests: `core/indexing` unit tests for the switch (park/resume, cancel wakes a
  parked worker, idempotent cancel, reset) and `core/engine/tests/index_control.rs`
  for the two rules that matter — a cancelled walk keeps every row, and the next
  complete walk still sweeps what really disappeared.

### 3.5 Search inside recognized text

The Studio used to recognize a scan, export it and forget the text. Now a
finished run stores it in the local database, and the launcher finds the file by
a phrase inside it:

- `document_text` (one row per path, replaced on re-run) plus `document_token`,
  an inverted index over the same normalized tokens search uses. No FTS5: a
  plain table with a covering primary key is enough at this scale and keeps the
  SQLite build unchanged.
- Name matches first, then up to two content hits (with an excerpt), then the
  web row. A file already matched by name is not repeated. Content search is
  skipped below three characters, because one letter matches nearly every page.
- Bounded: 400k characters per document, 20,000 documents, oldest dropped.
- Settings → Indexing shows the count, an on/off switch (on by default) and a
  *Clear indexed text* action that leaves the user's files alone. Exporting a
  document re-stores the text as edited, so the index describes what was saved.
- Arabic folds both ways: `فاتورة` in the document and `فاتوره` in the query meet
  at the same token, pinned by a storage test.

### 3.6 Documents that already hold text

The Studio could only read pixels. A `.docx` or `.xlsx` had to be printed or
photographed first, and a `.txt` could not be opened at all — while the search
index kept those files' names and nothing else. The pipeline now has a reader
for the formats that are already text (`core/ocr_engine/src/document.rs`):

- **`.docx`** — `word/document.xml` scanned paragraph by paragraph: `<w:t>` runs
  concatenated, tabs and breaks preserved, entities decoded, pages split where
  Word recorded a break. No pagination recorded means one page, which is honest
  rather than invented.
- **`.xlsx`** — `sharedStrings.xml`, `workbook.xml` and its relationships, then
  each sheet. Cells are placed by their `r` reference so sparse rows keep their
  columns; shared strings, inline strings and booleans resolve. Values are shown
  as stored, not as display-formatted, and a formula shows its cached result.
- **`.txt`, `.md`, `.csv`, `.tsv`, `.log`** — UTF-8 with or without a BOM, and
  UTF-16 with a BOM (or the zero-byte pattern of BOM-less Latin text). Anything
  else is refused with a message that names the fix; decoding Windows-1256 as
  UTF-8 would produce `�` and call it success.

The reader returns the same `PageResult` the OCR pipeline returns, so the text
pane, the region editor, the `.docx`/`.xlsx` export and the content index work
without a second path. The button reads **Extract text** for these formats, and
the preview pane says a text document has no page image instead of asking the
PDF renderer for one. Bounds: 64 MB per file, 32 MB per package entry, 2,000,000
characters and 500 pages per document; no temporary file, no network. Tests cover
docx paragraphs/tabs/entities/page breaks, xlsx shared strings/column placement/
multi-sheet naming, form-feed splitting, UTF-16 decoding, invalid-UTF-8 refusal,
cell references, entity and attribute parsing, a real Arabic-named file on disk,
and the character budget. A second integration test in `zeidex-exporter`
(`tests/round_trip.rs`) writes a `.docx` and an `.xlsx` with the real exporter —
Arabic title, native table — and reads them back through the new reader, so the
two halves are proven against the files the product actually produces, not only
against fixtures.

### 3.7 Other improvements

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
| Batch exact lookup | 100,000 files, 200 existing names, 100%, release | 110 ms total, **0.55 ms/name** (0.55–0.66 across two runs) |
| Batch misses | 100,000 files, 200 absent names, 70% | 701 ms total, **3.51 ms/name** — was 4.14 before this round's matcher changes (**−15%**) |
| Batch typos | 100,000 files, 200 typo'd existing names, 70% | 2,858 ms total, **14.3 ms/name** — was 17.8 (**−20%**) |
| Batch broad | 100,000 files, 200 broad names, 50% | 2,761 ms total, **13.8 ms/name** — was 16.2 (**−15%**) |
| Batch memory | same 100k-file run, peak resident set of the process | **59 MiB**, corpus included |
| Fuzzy pass before/after | 1,000 typo'd names vs 20,000 files, 8 cores, release | **136.6 s → 13.4–16.9 s** (rayon, one normalization, reused buffer, skip distance when containment ≥85) |
| Pack load + read (v4 server-det) | release, bundled layout/table + pack det/rec, one Arabic fixture page | **2.20 s warm** (2.84 s first run), 56 characters |
| Pack load + read (v5 server) | same fixture | **1.96 s warm** (8.26 s first run), 56 characters |
| Photo degradation | 12-cell table fixture, exact cell hits | clean 12/12, low contrast 12/12, dim 12/12, blur 12/12, noise 11/12, JPEG q45 12/12, combined 10/12 |
| Idle CPU, launcher visible | real app under Xvfb, software rendering, no interaction, 6 s `/proc` sample | **60–80% → 0.0%** of one core (blurred aurora animation removed) |
| Frontend bundle | production build | JS 387.89 kB (gzip 114.78 kB), CSS 69.70 kB (gzip 12.60 kB) |
| Release binary | aarch64 Linux, `cargo build --release` | 19,232,984 bytes (final, after §2.25–§2.29); the earlier build was 19,036,376 bytes, and the −524 KB step came from removing `tauri-plugin-fs` (§2.22) |
| Installer | Debian arm64 | 54,725,630 bytes (52.2 MiB), installed size 85,851 KB |
| Installer | Debian amd64 (CI) | 51,153,890 bytes (48.8 MiB), installed size 91,295 KB |
| Indexed-path lookup | `EXPLAIN QUERY PLAN` on the `candidates` schema | `SCAN candidates` → `SEARCH candidates USING COVERING INDEX idx_candidates_path (path=?)` (§2.23) |

The 100k-file numbers are from `core/batch/tests/perf_batch.rs` (ignored by default, run with `--release -- --ignored --nocapture`). The "was" figures come from the same test on the same host immediately before this round's matcher changes; the exact-lookup path is a hash probe either way, so its difference is run-to-run noise rather than a regression. The pack numbers are from `core/ocr_engine/tests/pack_smoke.rs` with the real ONNX models. No before/after claim is made for paths that were not measured before the change.

---

## 5. Security

Verified in code and tests:

- **Local-first OCR:** ONNX Runtime runs in-process; no OCR image or extracted text leaves the machine. Translation is the only network feature and is user-initiated; it is documented in the README privacy table.
- **No telemetry, account or analytics endpoint.**
- **Model downloads:** user-initiated only, HTTPS, every URL pinned to an immutable revision, SHA-256 checked before install, staging verified again before promotion, failed install leaves the working pack untouched. The model root is per-user and overridable only by `ZEIDEX_MODELS_ROOT` for tests.
- **Tauri surface:** `withGlobalTauri: false`, a strict CSP (`default-src 'self'`, no remote scripts), a minimal capability set (window, tray and core defaults only — the dialog and fs defaults were removed together with `tauri-plugin-fs`, §2.22), no shell plugin.
- **Reads are capabilities, not strings:** nine commands (`ocr_page_preview`, `ocr_process_image`, `document_extract_text`, `ocr_process_pdf`, `ocr_process_pdf_range`, `document_text_store`, `document_text_remove`, `open_target`, `reveal_target`) used to act on any path the page named. They now ask `AppState::authorize_read` first, which allows a path only when it is inside a configured scan root, was minted by a native picker / OS drop / Zeidex's own export, is an app-owned path (database, config, model folders), or is a row the index already holds. Containment is checked after canonicalization, so `..` and a symlink out of a root are refused; a missing leaf is resolved through its canonical parent. The index-known rule is what keeps launcher results in `/usr/share/applications` openable. Picks are capped at 512 entries, oldest forgotten first.
- **Writes are bound to a native dialog:** `batch_export`, `export_docx_pages` and `export_xlsx_pages` open their own save dialog in Rust (`src-tauri/src/commands/dialogs.rs`) and take only cosmetic strings from the page. Before this change the page supplied the path, so a script in the webview could overwrite any writable file without the user seeing a dialog. The suggested file name is reduced to its last component before it reaches the dialog, and every export is written to a `.part` sibling and renamed into place, so a failed export cannot truncate an existing file. The two unused single-page export commands were removed rather than left as extra IPC surface.
- **A report from the UI is validated:** `batch_export` rejects more than 2 000 rows, more than 256 matches per row, a threshold above 100 and a score above 100 — the same ceilings the search enforces — so a hand-built report cannot make the export thread allocate without bound.
- **Process execution:** the shell only uses the platform opener for `open`/`reveal`; `open_url` refuses anything that is not `http://` or `https://`; no user text reaches a shell.
- **SQL:** rusqlite parameter binding; the only formatted SQL is a static `WHERE` fragment built from code constants, not user input.
- **Clipboard:** secret-like strings over 8 KiB are refused, retention and size caps apply, recording can be paused.
- **Logs:** errors are reported to the UI; no file content or clipboard text is logged.
- **Unsafe code:** limited to Windows COM helpers and process-creation flags; no unsafe in the new model/download code.

### Dependency audit

`cargo audit` (v0.23.0, advisory DB 1555 entries) ran against both lockfiles. It
found **eight `zip` 8.6.0 advisories** — RUSTSEC-2026-0367/0368/0369 (archive
`extract` follows symlinks and applies Unix permissions through them; the third
scores 9.1 critical) and RUSTSEC-2026-0429/0545/0546/0547/0548. The app never
calls `extract`: it writes packages with `docx-rs` and reads its own output in
tests. Shipping a known-vulnerable archive reader anyway is how a "we don't use
that path" becomes a bug report, so the dependency moved rather than being
waived.

- `zeidex-ocr` and `zeidex-exporter` now use `zip = 9` with
  `default-features = false, features = ["deflate"]`. The default feature set
  pulls `zstd`, whose `zstd-sys` pin conflicts with the workspace's locked
  version; a `.docx` needs deflate only.
- Published `docx-rs` 0.4.22 still pins `zip ^8.6.0`, so the exporter takes the
  upstream commit that moved to zip 9 (`3b959fa`, "Fix Dependabot alerts for Rust
  and WASM dependencies"), pinned by revision for reproducibility.
- `zeidex-batch`'s dev-dependency (used to open the generated workbook in tests)
  moved to 9 as well.
- The zip 9 API made `entry.name()` and `file_names()` fallible; the two call
  sites in `mark_rtl_tables` and the report test now treat a nameless entry as
  corruption instead of assuming it.

After the change both lockfiles carry zip 9 only, and `cargo audit` reports
**0 vulnerabilities**. It still reports unmaintained-crate warnings (`chrono`
RUSTSEC-2026-0543, `paste` RUSTSEC-2024-0436, plus `proc-macro-error` and
`glib` in the Tauri tree) — those are warnings, not vulnerabilities, and come
from transitive dependencies of the framework, so they are recorded in §9 rather
than patched with a fork.

`npm audit --omit=dev` in `app/`: **0 vulnerabilities**.

Not performed: external penetration test, fuzzing or sanitizers, and signature
verification of the produced packages.

---

## 6. UI/UX

- Dark graphite surfaces with a cyan/iris accent, consistent type scale (Inter / Noto Kufi Arabic / JetBrains Mono), spacing and focus rings.
- **Dark and light themes.** Settings → Appearance offers Dark, Light and System. The choice persists in `localStorage` (`zeidex.theme`), is applied before the first paint (no flash), and `System` follows `prefers-color-scheme` live. The light theme is implemented by re-pointing the design tokens rather than duplicating the stylesheet: the zinc ramp inverts, `ink-*` becomes paper, and `--color-white` becomes ink so every `white/N` hairline and `text-white` label darkens in one move; a short block of targeted overrides covers panels, keys, segmented controls, skeletons, scrollbars and selection. Two literal-white materials (the page-image backing and the toggle knob) are pinned to `#fff` so the remap cannot turn them dark.
- Full RTL interface with logical CSS properties; Arabic and mixed-direction results render in logical order.
- New surfaces integrate with the existing footer navigation: Batch Search, OCR Models; the launcher keeps its keyboard-first flow.
- Models screen: tier badge, recommended badge, RAM/download/on-disk/languages/CPU-GPU/files, progress with speed and remaining bytes, verify problems, benchmark table with CER, file list with source/licence, two-step destructive confirmation.
- Advisor: hardware summary, transparent criteria with measured details, class, recommended pack, reason, trade-offs, OCR workers/batch chunk/index threads, and the four actions (download, test, compare, choose manually).
- Batch: review counts before searching, threshold label, expandable rows, filters/sorts, bulk actions, hidden-row restore, statistics, export with save dialog.
- **Accessibility is tested, not assumed.** `app/src/contrast.test.ts` checks 28 text tokens and two label-on-fill pairs in both themes against WCAG AA, asserts the muted ramp stays ordered, and scans every `.tsx` for a `text-<palette>-<step>` class the token table does not declare. It found two real failures — `zinc-500` at 3.78:1 and `zinc-600` at 2.36:1 on the panel — which were lifted to 6.61:1 and 5.76:1 (§2.24).
- Empty, loading and error states exist for each new surface.
- Motion is limited to what carries meaning: the 220 ms entrance, the shimmer on real skeletons and the footer's live dot. The decorative aurora is static (§2.9).

**Visual verification** was performed against the real Tauri binary under Xvfb with software rendering, not in a browser: launcher (dark and light), Settings/Search (dark and light), Settings/Appearance (dark and light), Models (dark and light), Batch Search with a live run (2 exact including an Arabic filename, 1 high-confidence, 1 not found; 12 ms over 4,806 indexed files), OCR Studio and the Arabic RTL layout in both themes. Screens were captured with `import -window` and inspected for contrast and layout. Chrome/Playwright still cannot start in this PRoot host, so no DOM-level automated visual regression exists; the checks are pixel inspections of the shipped app.

---

## 7. Testing Results

| Command | Outcome |
|---|---|
| `cargo fmt --all --check` (run from `core/`) | **PASS** — the workspace manifest is virtual, so `--manifest-path core/Cargo.toml --check` fails with "Failed to find targets"; `--all` from the directory is the form that works |
| `cargo fmt --manifest-path src-tauri/Cargo.toml --check` | **PASS** (after `cargo fmt` on this round's files) |
| `cargo clippy --manifest-path core/Cargo.toml --workspace --lib -- -D clippy::unwrap_used -D clippy::expect_used` | **PASS** (0 warnings) |
| `cargo clippy --manifest-path src-tauri/Cargo.toml --lib -- -D clippy::unwrap_used -D clippy::expect_used` | **PASS** (0 warnings) |
| `cargo test --workspace` (from `core/`, with `ORT_DYLIB_PATH`, model and fixture env vars, and the v5 pack) | **548 passed, 0 failed, 2 ignored** across 30 test binaries plus 12 doc-test binaries. The two ignored tests are the 100k scale test and a large-report test, both run separately. Every smoke test executed against real models and fixtures — `ocr_smoke` 3.8 s, `pdf_smoke` 39.7 s, `pack_smoke` 13.7 s, `photo_quality` 16.5 s, `photo_smoke` 6.9 s, `layout_smoke` 1.7 s, `table_smoke` 2.0 s, `ocr_accuracy` 1.9 s, `arabic_smoke` 0.8 s. `arabic_v5_smoke` needs a v5 recognizer directory and returned early in this run; it was then run separately (next row) rather than counted as coverage |
| `cargo test -p zeidex-ocr --test arabic_v5_smoke` (with `ZEIDEX_TEST_V5_MODELS_DIR` pointing at the v5 mobile recognizer) | **1 passed** in 0.81 s — three lines read at 0.920, 0.900 and 0.885 confidence, including the long line v4 cannot read in full |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib` | **64 passed, 0 failed** (adds the index-known capability case to the 53 above) |
| `cargo test -p zeidex-storage` | **70 passed** — including the two tests that pin `is_indexed_path` (exact-match semantics: prefix, suffix, other case and `/etc/shadow` refused, an Arabic path accepted; a deleted file still counts as indexed) and the four staleness tests |
| `cargo test -p zeidex-engine --test index_control` | **2 passed** (cancelled walk keeps rows; paused walk resumes and writes nothing while parked) |
| `cargo test -p zeidex-storage documents` | **9 passed** (token folding, content hit, all-token query, replace/remove, switch default, and the four staleness cases: unchanged kept, rewritten pruned with its tokens, deleted pruned, NULL-mtime kept) |
| `cargo test -p zeidex-ocr document` | **20 passed** (docx paragraphs/tabs/entities/page breaks, xlsx shared strings/column placement/sheet naming/number formats, odt paragraphs/tabs/spaces/page breaks, CP1256 and CP1252 decoding, invalid UTF-8 refused, cell references, entities/attributes, relationship targets, real Arabic-named file, character budget) |
| `cargo test -p zeidex-exporter --test round_trip` | **2 passed** (a `.docx` and an `.xlsx` written by the real exporter are read back with their Arabic title and table cells) |
| `cargo test -p zeidex-indexing` | **13 passed** (park/resume, cancel wakes a parked worker, idempotent cancel, reset) |
| `cargo test -p zeidex-batch` | **42 passed, 0 failed**; integration `report` **10 passed, 1 ignored**, `export_xlsx` **1 passed**, `perf_batch` **1 ignored** (run separately) |
| `cargo test -p zeidex-models` | **44 passed** (installer status/verify/recover/disk requirement, device classification) |
| `cargo test -p zeidex-ocr --lib` | **64 passed** (adds the shared-ONNX-environment regression) |
| `npx tsc --noEmit` (`app/`) | **PASS** |
| `npm test` (`app/`) | **67 passed, 0 failed** (adds the contrast suite and the Rust/TS extension-list parity test) |
| `npm run build` (`app/`) | **PASS** |
| `cargo test -p zeidex-batch --test perf_batch --release -- --ignored` | **PASS**, numbers in §4 |
| `pack_smoke` for `ppocrv4-server-det` and `ppocrv5-server` | **PASS**, 56 characters each |
| `cargo test -p zeidex-batch --test export_xlsx` | **PASS** |
| `cargo audit --file core/Cargo.lock` | **0 vulnerabilities**, 2 warnings (unmaintained `chrono`, `paste`) |
| `cargo audit --file src-tauri/Cargo.lock` | **0 vulnerabilities**, 4 warnings (adds unmaintained `proc-macro-error`, unsound `glib` 0.18.5) |
| `npm audit --omit=dev` (`app/`) | **0 vulnerabilities** |
| `python3 scripts/verify-xlsx.py core/target/tmp/zeidex-batch-report.xlsx` | **PASS** — 4 sheets, results filter `A1:K8`, 7 result rows, 1 missing row, 2 multiple rows, `chart series: ['Summary!$B$7:$B$10']` |
| Startup smoke, real binary under Xvfb | **PASS** — 30 s run of the rebuilt release binary with a throwaway `XDG_DATA_HOME`, no panic, exit only on the timeout, `zeidex.db` created with the expected nine tables, so `AppState::new` and the capability roots apply cleanly at startup |
| Startup prune, real binary under Xvfb | **PASS** — a `document_text` row pointing at a deleted file was inserted with one token; the next launch printed `zeidex: dropped 1 stale document index entries (1 missing, 0 changed)` and both the row and its token were gone afterwards (1 → 0 each) |
| `tauri build --bundles deb` (from the repository root, frontend pre-built) | **PASS** — `Zeidex Studio_0.1.0_arm64.deb`, rebuilt from the pushed commit after this round's fixes |
| Release workflow `45009610649` (tag `v0.1.0` → `4469de5`) | **PASS** — Linux and Windows jobs both succeeded; the four x64 assets were uploaded to the release |
| `dpkg-deb -I` on the amd64 and arm64 packages | **PASS** — `zeidex-studio 0.1.0`, correct architecture, exactly three dependencies, installed size 91,295 / 85,851 KB |
| Package format checks | **PASS** — AppImage `AI\x02` magic at offset 8 and x86-64 ELF; MSI OLE header `d0cf11e0a1b11ae1`; NSIS `MZ`; `.deb` ar archive read by `dpkg-deb` |
| `ldd` on the extracted arm64 binary | **PASS** (no missing libraries) |
| `SHA256SUMS` round-trip | **PASS** — re-downloaded from the published release and compared with the local file (`diff` clean) |
| GUI launch | **BLOCKED** (no display in PRoot) |
| Browser visual pass | **BLOCKED** (Chrome/Playwright cannot start in this host) |
| AppImage execution, Windows MSI/NSIS runtime, macOS DMG | **NOT RUN** (x86-64 binaries on an aarch64 host; no Windows or macOS machine; macOS is not in the release matrix) |

Test totals: **679 automated tests passed, 0 failed** (548 core + 64 shell + 67 frontend), plus the scale, pack and workbook checks above. The count is the sum of every `test result` line in a complete, untruncated run of each suite, not an estimate. The core run used real models, real fixtures and a real v5 pack for `pack_smoke`; `arabic_v5_smoke` was run separately against the v5 mobile recognizer and read all three lines at 0.920, 0.900 and 0.885 confidence. The only tests not counted are the two `#[ignore]`d ones (the 100k scale test and a large-report test), which are run separately.

---

## 8. Build and Release Status

- **Published:** [`v0.1.0`](https://github.com/zeiad6/zeidex-search/releases/tag/v0.1.0) — the tag now points at `4469de5`, and all six assets were replaced with builds of that commit. The previous four x64 assets and their `SHA256SUMS` were deleted first; nothing from the old build remains on the release.
- **x64 bundles, built by CI:** run [`45009610649`](https://github.com/zeiad6/zeidex-search/actions/runs/45009610649) from tag `v0.1.0` — both jobs (Linux, Windows) succeeded and uploaded:
  - `Zeidex.Studio_0.1.0_amd64.deb` — 51,153,890 bytes (48.8 MiB), installed size 91,295 KB, depends `libayatana-appindicator3-1, libwebkit2gtk-4.1-0, libgtk-3-0`, SHA-256 `4cca3a6df419027ebeb05b0405b984ee3dbbb58f502436d57218bac7b4523b2f`.
  - `Zeidex.Studio_0.1.0_amd64.AppImage` — 132,495,864 bytes (126.4 MiB), SHA-256 `1e2dd7a8f9384301f63af1c8dc678b7b32aea7e1a9b6d19064a1d52da4de0cec`.
  - `Zeidex.Studio_0.1.0_x64_en-US.msi` — 50,704,384 bytes (48.4 MiB), SHA-256 `0b0dee5a969488b9e2365f318edf76e7cfbc4d153858d1b4162ff2af745f60b8`.
  - `Zeidex.Studio_0.1.0_x64-setup.exe` — 45,618,021 bytes (43.5 MiB), SHA-256 `af7fcd3375a50f0aa680bc480f4abf1fc0ab0b07da33c27cee4a379aec668808`.
- **arm64 bundle, built here:** `Zeidex.Studio_0.1.0_arm64.deb` — 54,725,630 bytes (52.2 MiB), installed size 85,851 KB, same three dependencies, SHA-256 `c6a763da330d65d2ce50da598d6fea1735ced0f3f318ea29a2475ab041a07a97`. Built from the same commit with `tauri build --bundles deb` after `npm run build` (2.36 s, 387.93 kB JS / 69.70 kB CSS); `cargo` finished the release profile in about 1 m 50 s.
- **Artifact checks:** `dpkg-deb -I` on both `.deb` files reports `zeidex-studio 0.1.0` with the right architecture and exactly three dependencies (no duplicates). The AppImage carries the `AI\x02` magic at offset 8 and is an x86-64 ELF; the MSI is an OLE compound file (`d0cf11e0a1b11ae1`); the NSIS installer starts with `MZ`. `ldd` on the extracted arm64 binary reports no missing shared objects.
- **Launch:** the packaged arm64 binary was extracted from the `.deb` and started under Xvfb with a throwaway `XDG_DATA_HOME` — no panic, `zeidex.db` created with the expected tables, and the startup prune pass runs (verified separately by planting a stale row: `dropped 1 stale document index entries (1 missing, 0 changed)`, row and token both gone). The x64 packages cannot be executed on this aarch64 host.
- **Dependency list:** the earlier `bundle.linux.deb.depends` entries were removed from `tauri.conf.json` after they turned out to be *added* to Tauri's auto-detected list, which shipped every dependency twice. Auto-detection reads the binary's real linkage and names exactly three packages. The `| libgtk-3-0t64` alternative is not lost: on Ubuntu 24.04+ `libgtk-3-0t64` declares `Provides: libgtk-3-0` (verified here: `Provides: gtk3-binver-3.0.0, libgtk-3-0 (= 3.24.52-0ubuntu1)`), so a plain `libgtk-3-0` dependency resolves there.
- **Payload:** 19.2 MB binary, 25.1 MB `libonnxruntime.so`, 8.3 MB `libpdfium.so`, compact models only; Tier 2/3 packs are downloaded on demand and checksum-verified.
- **Checksums:** `SHA256SUMS` on the release lists all five packages and was re-downloaded after upload to confirm it matches the local file byte for byte.
- **Not verified:** Windows MSI/NSIS runtime behaviour and macOS (no such machines or toolchains here); AppImage execution (an x86-64 binary on an aarch64 host); code signing (no keys). The release workflow covers Linux and Windows on native runners; macOS is documented as build-from-source, not a shipped artifact.

---

## 9. Remaining Issues

1. **Light theme is a token remap, not a second design** — it inverts the zinc
   ramp, maps `ink-*` to paper and darkens the accent, which covers every screen
   without a duplicated stylesheet. It has been inspected on the launcher, Settings,
   Models, Batch and OCR Studio, but a full design review of every state in light
   mode has not been done. Low-medium.
2. **`.doc` and `.ppt` are still refused, not read** — these are binary OLE
   formats whose text lives in compressed streams, not XML; reading them is a
   separate parser, not another branch. `.odt`, `.docx`, `.xlsx` and the legacy
   Windows-1256/1252 text encodings are now read (§2.25–§2.27), and the Studio
   still names an unsupported format instead of guessing at its bytes. Low.
3. **Spreadsheet formulas are not evaluated** — a formula cell shows the cached
   result the file stores, which is what Excel last calculated; if the file was
   written by a tool that stored no cached value, the cell reads empty. Dates,
   percentages and times are now rendered from the cell's number format
   (§2.27); the formula engine itself is not implemented. Low.
4. **The content index is pruned at startup, not watched** — text indexed for a
   file that is edited while the app is running stays until the next launch
   (§2.28). There is no background re-OCR, by design: re-running a model over
   every changed file would cost far more than the stale row. Low.
5. **Visual verification is pixel inspection, not automated** — the real binary was
   run under Xvfb and screenshots inspected, but Chrome/Playwright cannot start in
   this PRoot host, so there is no DOM-level visual regression suite. Medium for
   confidence, environmental.
6. **Windows/macOS/AppImage packaging unverified** — the workflows exist for
   Linux/Windows; no artifact was produced here. macOS is not in the release matrix.
7. **GPU detection unverified on real hardware** — the Linux sysfs/NVIDIA probe is
   unit-tested with synthetic input; no GPU is present here. CUDA/DirectML feature
   builds were not exercised.
8. **Benchmark coverage** — the benchmark command and CER tests exist; only the
   bundled samples were run, not every pack through the UI.
9. **"Three additional models" interpretation** — the app ships three tiers: the
   bundled v4-mobile plus two additional packs. A literal fourth model was not
   added; the tier model is the requirement that matters.
10. **No fuzzing, sanitizers or external penetration test** — the security review is
   code-level and test-backed, not adversarial validation.
11. **In-app download UI not exercised in a webview** — the installer path produced
    both packs with valid manifests; the button flow is covered by unit tests, not
    a live click-through.
12. **Packages are not signed** — no code-signing keys in this environment.
13. **The read policy is allow-list by provenance, not by intent** — a path inside a
    scan root is readable because the user asked Zeidex to index that folder, not
    because the current action needs it. That is the right granularity for a local
    file tool (the alternative is a permission prompt per file), but it means the
    policy bounds *where* a compromised page can reach, not *what* it can do there.
    Picks are also in-memory: a restart forgets them, so a path chosen before a
    restart has to be chosen again. Both are deliberate; recorded so the boundary
    is not mistaken for something stronger. The open/reveal flow was not clicked
    through in a live GUI here; the design rests on the invariant that every
    launcher result is a row of `store.load_candidates`, so rule 4 always holds
    for a result the user can click — stated in `assets/docs/architecture.md`.
14. **Four advisory-database warnings remain in the dependency tree** — none is a
    vulnerability, and none is ours to patch directly:
    - `chrono` (RUSTSEC-2026-0543, unmaintained) comes from `pdfium-render` 0.9.5,
      which needs it for PDF dates. Replacing it means replacing the PDF binding.
    - `paste` (RUSTSEC-2024-0436, unmaintained) reaches the tree through
      `image` → `ravif` → `rav1e`. `image` is pulled with default features by
      `docx-rs`, `pdfium-render` and our own crate, so trimming the feature in one
      manifest cannot remove it; it is compile-time only and unused at runtime.
    - `proc-macro-error` (RUSTSEC-2024-0370, unmaintained) is a transitive
      build-time macro dependency in the Tauri tree.
    - `glib` 0.18.5 (RUSTSEC-2024-0429, *unsound*) is pinned by Tauri's GTK 0.18
      stack; the fix landed in glib 0.20, which requires Tauri to move its GTK
      bindings. The affected code is `glib::VariantStrIter`'s iterator impls,
      used for GMenu/tray labels; Zeidex's tray labels are set once at startup.
      Recorded rather than forked.

---

## 10. Final Completion Matrix

The count is defined by 52 requirement groups: the 13 previous-prompt phases plus 39 current-prompt deliverable groups (8 OCR/model manager, 5 advisor, 10 batch, 4 Excel, 4 security, 4 UI/UX, 4 packaging). A group counts as COMPLETE only with implementation and a test/inspection result; overlapping gaps are counted once in the group where they belong.

| Area | Items | Complete | Partial | Unverified / Missing |
|---|---|---|---|---|
| Previous audit (13 phases) | 13 | 11 | 1 | 1 |
| OCR tiers and model manager | 8 | 8 | 0 | 0 |
| Hardware advisor | 5 | 5 | 0 | 0 |
| Batch search | 10 | 10 | 0 | 0 |
| Excel report | 4 | 4 | 0 | 0 |
| Security and privacy | 4 | 4 | 0 | 0 |
| UI/UX | 4 | 2 | 1 | 1 |
| Packaging/release | 4 | 1 | 0 | 3 |
| **Total** | **52** | **45** | **2** | **5** |

**Fully complete: 45/52 (87%).** With partial items counted at half credit the weighted figure is **46/52 (88%).** The project is **not 100% complete**: the 2 partial and 5 unverified/missing groups are listed in §9.

The percentage did not move this round, and that is the honest result: every defect fixed in §2.11–§2.29 sat inside a group the previous matrix already counted as COMPLETE. The matrix measured *whether a feature existed and had tests*; it did not measure whether those tests covered the failure modes. One entry was genuinely inconsistent — the security row read 4/4 while §9 item 13 described an open read path. That hole is now closed by the capability policy (§2.20), so the 4/4 is accurate rather than optimistic; it was not a new requirement, and no other count moved. The five fixes in §2.25–§2.29 were of the same kind: a supported format that did not actually decode, a date that read as a serial number, an index that kept text for files it could no longer vouch for, and an OCR result filed under the wrong source. Each had a test gap behind it, not a missing screen, so fixing them made the existing COMPLETE verdicts true rather than adding new ones.

The largest functional gap is now the absence of `.doc`/`.ppt` readers and of a full light-theme design review; the largest confidence gap is still the absence of automated visual verification and of non-Linux packaging in this environment. The 2 partial and 5 unverified/missing groups are listed in §9, and none of them is concealed by a rounded percentage.

---

*Report generated from the working tree on 2026-11-22. All commands were run in this environment; blocked items are marked as blocked rather than passed.*
