<p align="center">
  <img src="https://raw.githubusercontent.com/zeiad6/zeidex-search/main/assets/brand/zeidex-hero-static.png" alt="Zeidex Search" width="780">
</p>

<h1 align="center">Zeidex Search 0.4.0</h1>

<p align="center">
  <b>Your files. Your machine. Your data.</b><br>
  A keyboard-first launcher and an offline document AI for Windows and Linux —<br>
  with macOS built from source.
</p>

<p align="center">
  <img alt="No account" src="https://img.shields.io/badge/account-not%20required-2f855a?style=flat-square">
  <img alt="No telemetry" src="https://img.shields.io/badge/telemetry-none-2f855a?style=flat-square">
  <img alt="Offline OCR" src="https://img.shields.io/badge/OCR-100%25%20local-2f855a?style=flat-square">
  <img alt="OCR tiers" src="https://img.shields.io/badge/models-3%20tiers-2d3748?style=flat-square">
  <img alt="Licence" src="https://img.shields.io/badge/licence-GPL--3.0--or--later-2d3748?style=flat-square">
</p>

---

## What's new in 0.4.0

Recognition was last release's problem. This one is about what you get *after*
the scan: a document, not a wall of text.

### Data intelligence, one button from the Studio

**Data intelligence** reads the recognized pages as a document. It detects the
type — ID card, passport, invoice, receipt, bank statement, certificate, business
card, letter, form, table — and pulls the labeled fields into a table you can
correct. Eleven profiles ship, each with Arabic and English aliases, so
`الاسم`, `الاسم:` and `الأسم` all reach the same field.

The engine is deterministic and rule-based. It does not rewrite a value, does not
calculate a total that was not printed, and marks anything below 60 % confidence
for review instead of guessing. A profile that finds nothing says so.

### A stack of ID cards becomes one ordered table

- **One page, one row** — always. A page that produced no fields keeps an empty
  row, so page 3 never becomes page 2.
- **Page order is row order.** Nothing is renumbered.
- **Duplicate identifiers are reported, not dropped**, with both page numbers:
  two copies of the same card are a fact worth seeing.
- **A corrected profile applies to every row**, so the field view and the batch
  table cannot disagree.

### Edit, preview, export

Fields are editable in place, with undo. The JSON, CSV and Markdown tabs show the
exact bytes the exporter writes — both call the same Rust renderer, so the
preview cannot drift from the file. Exports: **TXT, CSV, JSON, Markdown, HTML**,
through the Rust-owned save dialog, atomically.

PDF export is deliberately absent: the build has no PDF writer that shapes
Arabic, and a PDF that breaks Arabic is worse than no button.

### Also fixed in this release

- Two organizer warnings (`national_id_missing`, `name_missing`) reached the UI as
  raw identifiers; they are now labelled in both languages, and a test scans the
  Rust crate so a future code cannot leak the same way.
- The batch table's last column was headed **Filled** while showing a confidence
  percentage; it is now **Confidence**.
- Twelve clippy warnings in the organizer crate: fixed.
- The demo bridge answered nothing for the organizer commands, so the panel
  could not be reviewed in a browser; it now serves honest fixtures in
  development only.

### Verified in this release

| Gate | Result |
| --- | --- |
| Core tests | 619 passed |
| Shell tests | 81 passed |
| Frontend tests | 100 passed |
| Clippy + rustfmt | clean, 0 warnings |
| Arabic OCR corpus | CER 0.60 %, WER 3.45 % |
| Accessibility (axe-core 4.10.2) | 0 violations, panel open, dark/RTL/light |
| Release binary | launches, indexes 5079 candidates, full schema |

The Windows runtime, the MSI/NSIS installers and GPU acceleration were **not**
exercised in the environment that produced this commit. They are listed in
`ENGINEERING_REPORT_0.4.0.md` § J with the reason, not described as done.

---

## What's new in 0.3.0

This release is about the two things a document launcher is judged on: reading
Arabic correctly, and behaving like a native desktop application. Every number
below is from a run in this tree; the commands are listed at the end of the
section.

### The product is now Zeidex Search

"Studio" described a document tool, but the window opens on a search box and the
document workspace is one mode inside it. The product name now says what the
program does, and the repository follows it: `zeidex` → **`zeidex-search`**
(`github.com/zeiad6/zeidex-search`). The tagline is *Offline search, OCR and
document AI*; the OCR workspace keeps its own name, **Document Studio**.

What deliberately did **not** change:

- the executable is still `zeidex`, and the bundle identifier is still
  `com.zeidexsoft.zeidex`;
- data still lives in `~/.zeidex`, so an upgrade keeps the index, settings,
  shortcut and OCR history;
- the Windows registry value that autostart writes is still `Zeidex`.

What did change, and matters when upgrading:

- the Linux package is now **`zeidex-search`**, its desktop entry
  `Zeidex Search.desktop`, and resources install to `/usr/lib/Zeidex Search/`;
- the Windows installer product name is **Zeidex Search**, which means it does
  not upgrade a previously installed *Zeidex Studio* in place. Remove the old
  entry in **Settings → Apps** first, then install this one.

The wordmark eyebrow reads `SEARCH`, and the README banner was re-rendered from
the same SVG so the animation and the still frame agree.

### Arabic OCR: CER 15.57 % → 0.60 %

The recognizer read an Arabic line as one image. On short, connected scripts that
is the wrong unit: a single misread word pulls the whole line with it, and the
model cannot recover from the mistake. `recognize_line` now re-reads a
low-confidence line word by word and merges the result:

- A line whose confidence is below **0.80** and whose text contains Arabic is
  split at word gaps (`segment_columns`), each segment is recognized on its own,
  and the best script-matched model is chosen per segment (`choose_segment`).
- The merge only runs for lines that actually contain Arabic. Latin text and
  table pages keep the previous cost: `pdf_smoke` (a Latin table page) takes
  **30.37 s**, against 32.33 s in 0.2.0 — the gate costs nothing measurable —
  while an intermediate build that re-read every line took 103 s on the same
  fixture.
- The whole-line result is kept when word re-reading does not beat it, so a
  clean line cannot be made worse.

Measured on the committed ten-line corpus (`tests/arabic_cer.rs`):

| Metric | 0.2.0 | 0.3.0 |
| --- | --- | --- |
| Character error rate | 15.57 % | **0.60 %** |
| Word error rate | 31.03 % | **3.45 %** |
| Lines read exactly | 5 of 10 | **9 of 10** |
| Ceiling enforced by the test | 0.20 / 0.40 | **0.08 / 0.15** |

The one line that is still not exact is the greeting
`السلام عليكم ورحمة الله`, read as `…رحمة اله` (4.35 % CER, 25 % WER). That is
the honest limit of this model on this corpus, not a number rounded up.

### The benchmark corpus was wrong before the model was blamed

`اختبار PDF` had been rendered in Noto Naskh, which has no Latin glyphs. The
"recognition failure" on that line was tofu in the fixture, not an OCR error.
The line is now rendered with `IBMPlexSansArabic-Bold.ttf`, and
`tools/make-arabic-corpus.py` uses fontTools to refuse to write a corpus when a
character has no glyph in the chosen font. A benchmark that cannot draw its own
ground truth is not a benchmark.

### File-format badges, not generic pages

Every file row used the same page icon, so a PDF, an Excel sheet and a scan were
visually identical. The launcher, the Document Studio and Batch Search now share
one badge set: PDF, Word, Excel, PowerPoint, image, text, code, archive, audio
and video. The badges are drawn in the app (`app/src/file-format.ts` and
`components/FileGlyph.tsx`), not vendor logos: a coloured tile, a white page and
a format mark that stays sharp at every DPI and follows the theme. The row's
kind chip also names the format instead of saying "File". The mapping is covered
by seven node tests, including every extension the file picker offers.

### A name matches at the start, at the end, or approximately

Search used to answer one question — does the name contain what I typed — so a
file was findable but the list did not say *why* it was there, and a typo was a
miss. Every result now carries the tier that surfaced it, shown as a small chip
next to the format badge:

| Tier | What it means | Example for `report` | Chip |
| --- | --- | --- | --- |
| Exact | The whole name is the query | `report` | Exact / مطابق |
| Start | The name begins with it | `report-2026.docx` | Start / بداية |
| End | The name ends with it | `annual-report.pdf` | End / نهاية |
| Inside | It appears anywhere else | `budget-report-2025.xlsx` | Inside / داخل |
| Partial | Letters in order, with gaps | `vsc` → Visual Studio Code | Partial / جزئي |
| Close | Within a small typo budget | `reprot-final.docx` | Close / مقارب |

- The end tier compares the extension-less stem, so `2026-014` finds
  `invoice-2026-014.pdf`. A query that is only the extension (`pdf`) is treated
  as a format filter rather than an ending, so every PDF is not labelled "End".
- A name that starts with the query still ranks above one that ends with it: the
  matcher scores prefix 1500 and suffix 1300, and the ranking layer adds +200 and
  +120 respectively. Both stay above a bare containment (1200).
- "Close" is bounded OSA (an adjacent transposition is one edit, so `reprot` is
  one edit from `report`), with one edit per four characters, at most three
  edits, at most four words, and never for a word shorter than four letters. It
  runs only when no literal tier matched the candidate, so it cannot displace a
  real match; its ceiling (600) stays below the engine's all-terms score (850).
- The tier is classified only for the rows about to be shown (at most `limit`),
  not for every candidate, so the retrieval loop keeps its previous cost.
- In a release-mode benchmark over 50,000 candidates, a query that matches
  nothing costs 28.8 ms against 25.3 ms for a prefix query: the typo pass adds
  about 3.6 ms (tens of nanoseconds per candidate), and it never runs for a row a
  literal tier already matched.
- Arabic file names work in all tiers, including the approximate one:
  `تقري` finds `تقرير-الاجتماع.pdf` after normalization folds the letter variants.

### Launch at login, and a quit that releases the shortcut

**Settings → Startup** adds one entry to the operating system's own login list:
the `HKCU\…\Run` value on Windows (written with `reg.exe` under
`CREATE_NO_WINDOW`, so no console flashes), an XDG autostart file on Linux, or a
per-user LaunchAgent on macOS. The switch is not a local flag: after every write
the shell reads the entry back, so a group policy that removes it shows as
"Inactive" instead of a switch that lies. A debug build warns that the entry
would point at `target/debug`. Disabling removes exactly the entry Zeidex
created; no service is installed.

The tray's **Quit** now unregisters the global shortcut before asking the process
to exit, and the same release runs on the normal exit path. A restart therefore
never races a shortcut the previous instance still owns. Closing the window
still hides to the tray; Quit is the action that ends the process.

### A startup race that could abort the app

Opening the same SQLite database from two connections at once (the shell's store
and an index worker, or two test threads) could run the schema migration twice.
The old `ensure_column_exists` checked `PRAGMA table_info` and then ran
`ALTER TABLE`; between the two the other connection added the column, and the
loser aborted with `duplicate column name: copy_count`. The migration now treats
that specific error as success, which is what makes the operation idempotent.
The regression test adds the same column twice and asserts it exists once.

### Batch Search reads Arabic names in the right order

Requested names and match names now carry `dir="auto"`, so an Arabic filename
picks an RTL base inside an otherwise LTR table instead of being reversed by the
bidi algorithm.

### Screenshots from the running UI

Nine captures in `assets/screenshots/`, taken from the real React tree against
the browser demo, at 2× device pixel ratio:

| File | What it shows |
| --- | --- |
| `launcher-idle.png` | The launcher at rest, caption cluster and filter tabs |
| `launcher-search.png` | Search results with the new format badges |
| `launcher-match-tiers.png` | Start, End, Inside and Close chips in one list |
| `launcher-arabic-search.png` | The same query in the RTL interface |
| `studio-formats.png` | The Studio empty state and the formats it reads |
| `studio-document.png` | A recognized invoice with regions and extracted text |
| `settings-shortcut.png` | The global-shortcut card |
| `settings-startup.png` | The new launch-at-login card |
| `batch-search.png` | Batch results, scores, format badges and export |

---

## What's new in 0.2.0

This release fixes the three defects that made the Windows build feel broken, and
adds the window controls the frameless shell never had. Everything below was
verified by building and testing the code in this tree; the Windows-only runtime
checks are listed at the end, with what could not be exercised here.

### PDF processing no longer dies after a model switch

`pdfium-render` keeps its library bindings in a process-global cell: binding a
second time returns `PdfiumLibraryBindingsAlreadyInitialized`. Zeidex built a new
OCR engine whenever a model pack was activated or the benchmark ran, so the second
engine failed to load and took image OCR down with it until the app restarted.

- New `pdf::shared_pdfium` binds once per process behind a mutex and hands the
  same handle to every engine; a failed bind is not cached, so installing the
  library later still works.
- The preview path and the OCR path now share that handle instead of each binding
  their own.
- Regression test `a_second_engine_reuses_the_bound_pdfium` builds two engines,
  OCRs and previews the same PDF with each, and checks the handle is the same
  allocation. **It fails on the previous code with the exact reported error and
  passes on this one** (see Verification).

### A release build is a GUI process

`src-tauri/src/main.rs` now carries
`#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]`. That one
attribute is the difference between a console-subsystem executable - which gets a
terminal window and dies when that terminal is closed - and a GUI executable.
Debug builds keep the console, so `cargo run` still shows panics and logs.
Startup failures in a release build are appended to `zeidex-startup.log` next to
the database (size-capped), because there is no console left to print to.

### Native dialogs open in front of Zeidex

Two causes, both fixed:

- The window was configured `alwaysOnTop: true`. A native picker is not a topmost
  window, so an always-on-top launcher sat above its own dialog. The flag is gone.
- Dialogs were created without an owner. Every picker, save dialog and folder
  chooser now calls `set_parent(main window)`, which hands the OS the window
  handle and makes the dialog modal to Zeidex.

No delays, no focus hacks, no forced topmost on the dialog itself.

### Window controls and a defined close policy

The window is frameless, and it had no minimize, maximize or close control at all.
All five screens now share a caption cluster (`WindowControls`) with tooltips,
accessible labels, hover/active states and keyboard focus:

- **Minimize** and **maximize/restore** act on the real window, and the icon
  follows the window's state through `onResized`.
- **Close** hides Zeidex to the tray, but only when the tray icon was actually
  created. On a desktop without a tray, Close keeps its ordinary meaning and ends
  the process instead of leaving an invisible one behind. Quit is an explicit tray
  action.
- The window now appears in the taskbar (`skipTaskbar: false`), so minimize is
  recoverable the way Windows users expect.
- `show_main` un-minimizes before focusing: a minimized window that is only
  shown stays minimized, which made the hotkey look dead.
- The toggle now hides only when the window is focused; pressing the hotkey while
  another application is active brings Zeidex forward instead of throwing it away.

### Global shortcut: one registration, visible status, configurable

- Registration is owned by `commands/shortcut.rs`, unregisters before registering
  (so a press can never toggle twice), and falls back to `Alt+Space` when a saved
  custom combination is refused by the system.
- The result - registered or not, and why - is shown in **Settings → Search →
  Global shortcut**, with Save and Reset to default. It used to go to `eprintln!`,
  which a release build has no console for.
- The choice is stored in the database and re-applied on the next launch.
- `Alt+Space` stays the default because PowerToys Run and Flow Launcher ship it;
  it is configurable precisely because that claim can fail.
- Regaining window focus puts the cursor in the search box and selects the old
  query, so typing works immediately after the hotkey or the tray.

### Under the hood

- `SqliteStore::load_app_setting` / `save_app_setting` for shell preferences that
  have no engine-side config key, with a round-trip/overwrite test.
- Tauri capabilities extended for the new window calls
  (`toggle-maximize`, `is-maximized`, `unmaximize`, `unminimize`, `close`), so the
  buttons work in a packaged build instead of failing silently.

### Born-digital PDFs are read, not re-recognized

Every PDF used to be rendered at 300 DPI and OCRed, including PDFs that already
contain text. On the one-page fixture that is **32.3 s per page**; reading the
same page's own text layer takes **1.4 s including engine and model load** and
returns the exact characters instead of a guess.

- `pdf::native_page_text` reads a page's text layer through pdfium and reports
  the page's pixel size at the OCR DPI, so a native page and a rendered page
  have the same geometry.
- `process_pdf_window` decides per page, before rendering: a page with at least
  24 letters or digits is taken from its text layer; anything less (a scan with
  a "Page 3" stamp, an image-only page) is rendered and OCRed exactly as before.
- Page order, page windows, progress events and cancellation are unchanged.
- Trade-off, stated plainly: a native page carries one page-sized text region,
  not detector line boxes, so table structure is not reconstructed for
  born-digital PDFs. The text itself is complete and more accurate than OCR.
- Tests: `pdf_native_text` proves the exact text and the page-sized region on a
  committed fixture (`tools/make-text-pdf.py`); two unit tests pin the threshold.

### An Arabic OCR benchmark with measured error rates

"Arabic works" is not a measurement. `tools/make-arabic-corpus.py` renders ten
deterministic lines (letters, ta marbuta, alef maqsura, Arabic-Indic digits,
mixed Latin) with `ground_truth.json`, and `tests/arabic_cer.rs` reports CER and
WER per line and in aggregate against that truth:

| Metric | Measured on PP-OCRv4 Arabic rec |
| --- | --- |
| Character error rate | **15.57 %** over 167 characters |
| Word error rate | **31.03 %** over 29 words |
| Lines read exactly | 5 of 10 |
| Aggregate search-folded CER | 15.57 % |

The per-line output also records the honest limits: the long greeting is
garbled in the middle (39 % CER), `١٢٣٤٥` loses two digits (11 %), a line mixing
Arabic and `PDF` returns only the Arabic half (40 %). The test fails if CER or
WER exceed the pinned ceilings, so a preprocessing or model change has to show
its effect instead of being assumed.

### Verification in this release

| Command | Result |
| --- | --- |
| `cargo test --workspace --manifest-path core/Cargo.toml` | 555 passed, 0 failed, 0 ignored |
| `cargo test -p zeidex-ocr --test pdf_native_text` (models + pdfium) | 1 passed, 1.36 s |
| `cargo test -p zeidex-ocr --test pdf_smoke` (models + pdfium) | 1 passed, 32.33 s — the image-only page still OCRs |
| `cargo test -p zeidex-ocr --test pdf_lifecycle` (models + pdfium) | 1 passed — four OCR passes across two engines |
| `cargo test -p zeidex-ocr --test arabic_cer` (models) | 1 passed, CER 15.57 % / WER 31.03 % |
| same lifecycle test on the pre-fix code | failed with `PdfiumLibraryBindingsAlreadyInitialized` |
| `cargo clippy --workspace --lib --manifest-path core/Cargo.toml -- -D clippy::unwrap_used -D clippy::expect_used` | clean |
| `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets` | clean |
| `cargo build --release --manifest-path src-tauri/Cargo.toml` | finished in 8 m 34 s, 18 MB binary |
| `cargo fmt --all -- --check` (core and shell) | clean |
| `npm test` in `app/` | 67 passed, 0 failed |
| `npx tsc --noEmit` in `app/` | clean |

**Not verified here:** this session runs on Linux arm64 with no Windows machine
and no display server, so the console window, dialog z-order, caption buttons,
tray, DPI and multi-monitor behaviour were not exercised on Windows. They are
implemented against the documented Tauri/Windows APIs and the code compiles, but
they need a Windows smoke test before the claim is complete. The Arabic corpus
is synthetic print, not scanned paper; it is a fixed ruler, not a claim about
every real document.

---

## 0.1.0 — the first public release

Zeidex Search started as a personal launcher and grew into a document studio. This
release packages both halves as a single, self-contained desktop app: **no account,
no telemetry, and no network calls at runtime** — the OCR models, dictionaries,
ONNX Runtime and the PDF rasteriser all ship inside the installer. Three OCR tiers,
a hardware advisor and batch file search are part of the same build.

### Highlights

| | |
| --- | --- |
| **Launch anything** | Fuzzy search across installed applications, files and folders, with keyboard-first navigation and a compact, always-ready window. The index updates itself while the app runs — new, renamed and deleted files appear without a manual rebuild, and indexing can be paused, resumed or cancelled from Settings. |
| **Clipboard history** | Searchable clipboard buffer with pinning, so the snippet you copied an hour ago is still one keystroke away. |
| **Inline answers** | Calculator, unit and currency-style expressions, and translation right in the search field — no browser tab required. |
| **Document Studio** | Drop a scan or a PDF, detect the layout, read the text, edit the result, then export to Word or Excel. A long PDF is read as a page window, and a run can be cancelled between pages. |
| **Three OCR tiers** | The bundled PP-OCRv4 Mobile model is ready on first launch; a balanced PP-OCRv4 Server pack and a high-accuracy PP-OCRv5 Server pack are one click away, downloaded on demand, checksum-verified and activated atomically. |
| **Hardware advisor** | Measures CPU, cores, RAM, GPU (where the platform exposes it without a driver query), free disk space and OS, classifies the machine as Basic, Standard, Advanced or High Performance against published criteria, and recommends a tier with the reason and the trade-offs stated. |
| **Batch file search** | Paste a list or import a `.txt`, review the parsed entries, pick a match threshold anywhere from 50% to 100%, then work through exact, high-confidence, possible, multiple and missing results — open, reveal, or copy paths. |
| **Excel report** | A four-sheet workbook: an executive summary with KPI block and chart, the full result table with filters and frozen panes, the missing files, and every multiple-match candidate with its score. |
| **Search inside documents** | Recognised text is indexed locally, so a launcher search finds a word inside a scanned PDF, not just its filename. `.docx`, `.xlsx`, `.odt` and plain text are read directly without an OCR pass. |
| **Reads Office and text files** | A legacy Windows-1256 or Windows-1252 text file is decoded instead of refused, and spreadsheet dates, percentages and times are rendered the way Excel shows them. |
| **Real Office files** | `.docx` with RTL `bidi` runs and native tables (including merged cells); `.xlsx` with a right-to-left sheet, numeric cells and merged ranges. |
| **Private by construction** | ONNX inference runs on your CPU, locally. Nothing is uploaded, and there is no analytics endpoint to disable. |

### OCR model tiers

| Tier | Model | Download | Best for |
| --- | --- | --- | --- |
| **Compact** | PP-OCRv4 Mobile | bundled | Low-end and CPU-only machines, quick extraction, first run with no network. |
| **Balanced** | PP-OCRv4 Server detection | ≈ 129 MB | Most users: sharper line detection on scans and screenshots at a moderate cost. |
| **Precision** | PP-OCRv5 Server | ≈ 181 MB | Difficult scans, dense Arabic pages and noisy images on a machine with headroom. |

Packs are written to the app data directory, never over the bundled model, and a
pack is only marked installed once every file matches its SHA-256. Activation is a
switch, so a failed download cannot leave OCR unusable — the previous tier keeps
working until the new one verifies.

### What is inside the installer

- The Tauri desktop shell and the React interface (RTL and Arabic UI included).
- PP-OCRv4 detection and English/Arabic recognition models, plus their dictionaries.
- ONNX Runtime and PDFium, staged next to the binary and loaded locally.
- Roughly **35 MB of models** and **32 MB of runtime libraries**, so the first run
  works on a machine that has never seen the internet. The two larger tiers are
  deliberately not bundled; they are downloaded when chosen.

<p align="center">
  <img src="https://raw.githubusercontent.com/zeiad6/zeidex-search/main/assets/screenshots/launcher-search.png" alt="Launcher search results" width="47%">
  <img src="https://raw.githubusercontent.com/zeiad6/zeidex-search/main/assets/screenshots/studio-document.png" alt="Document Studio" width="47%">
</p>

---

## Downloads

| Platform | Package | Best for |
| --- | --- | --- |
| **Windows 10/11 (x64)** | `Zeidex Search_0.4.0_x64_en-US.msi` | Standard installer, per-machine or per-user, WiX-based. |
| **Windows 10/11 (x64)** | `Zeidex Search_0.4.0_x64-setup.exe` | Lightweight NSIS installer; pick this if MSI is blocked by policy. |
| **Linux (x86_64)** | `Zeidex Search_0.4.0_amd64.deb` | Debian, Ubuntu, Mint, Pop!_OS and derivatives. |
| **Linux (x86_64)** | `Zeidex Search_0.4.0_amd64.AppImage` | Any distribution; no installation, no root. |
| **Linux (arm64)** | `Zeidex Search_0.4.0_arm64.deb` | Raspberry Pi 4/5 and ARM laptops on Debian 12+ or Ubuntu 22.04+. |

> Sizes and SHA-256 hashes are attached to the GitHub release by the workflow
> that builds each artifact from this commit; this file does not guess them.
> The Windows packages are built on GitHub's Windows runners — they were **not**
> built or run in the Linux environment that produced this commit (see
> `ENGINEERING_REPORT_0.4.0.md` § J). Remove a previously installed
> *Zeidex Studio* before installing this release — see
> [Known limitations](#known-limitations).

> The x64 packages are produced on GitHub's native runners from the release
> commit, so every artifact matches the source behind this release. The arm64
> package is built and startup-tested on aarch64 Linux. macOS bundles are not
> part of this workflow yet — see [Build from source](#build-from-source).

### Install

**Windows — MSI**

```powershell
msiexec /i "Zeidex Search_0.4.0_x64_en-US.msi"
```

**Windows — NSIS**

```powershell
"Zeidex Search_0.4.0_x64-setup.exe"
```

**Debian / Ubuntu (x86_64)**

```bash
sudo apt install "./Zeidex Search_0.4.0_amd64.deb"
```

**Debian / Ubuntu (arm64)**

```bash
sudo apt install "./Zeidex Search_0.4.0_arm64.deb"
```

**AppImage**

```bash
chmod +x "Zeidex Search_0.4.0_amd64.AppImage"
./"Zeidex Search_0.4.0_amd64.AppImage"
```

The Linux packages depend on `libwebkit2gtk-4.1-0`, `libgtk-3-0` and
`libayatana-appindicator3-1`; `apt` installs them automatically. On distributions
that do not ship WebKitGTK 4.1, use the AppImage.

---

## First run

1. Start Zeidex Search — the launcher is ready immediately, with no indexing
   wizard and no sign-in.
2. Type to search applications, files and folders; press `Enter` to open, or use
   the arrow keys to move through results.
3. Open **Studio** to drop a scan or PDF, run detection and recognition, edit the
   recognised text, and export to `.docx` or `.xlsx`.
4. Open **Models** to see the three tiers and install a larger one; open
   **Advisor** to have the machine measured and a tier recommended. Neither is
   required — the bundled model works offline out of the box.
5. Open **Batch** to search for a list of filenames at once and export the result
   as an Excel report.
6. Set `ZEIDEX_MODELS_DIR` only if you want the models stored somewhere other than
   the bundled location.

Configuration lives in `~/.zeidex` (Linux/macOS) or `%APPDATA%\zeidex` (Windows).

---

## Verification

Every x64 asset is built from commit `6b453ab` by the release workflow; the
arm64 package was built from the same commit on aarch64 Linux and inspected with
`dpkg-deb`. Verify a download before installing:

```bash
sha256sum "Zeidex Search_0.4.0_arm64.deb"
# 33674452e76d642b7c0af1c5ee51a40d77bfeefecfe4d9c0e10ed9191b0ae4e1
```

```powershell
Get-FileHash "Zeidex Search_0.4.0_x64_en-US.msi" -Algorithm SHA256
# 67713344233d4da6290e93c1a2ce9c93664e5f857b6b7bdf743573ea2a21cbda
Get-FileHash "Zeidex Search_0.4.0_x64-setup.exe" -Algorithm SHA256
# b0f811764d1236f70a5174db03796eed256c7e1739e9645cc108641b432b5b89
```

```bash
sha256sum "Zeidex Search_0.4.0_amd64.deb"
# ef0eaae53683187820b5c00c73879e80a87babcec46a6da7737a6a4643490049
sha256sum "Zeidex Search_0.4.0_amd64.AppImage"
# 6cba11c6bb850133bb1eac891ea0dec23d5c884421c56a0bb9dde047f7fa98b8
```

---

## Known limitations

- **Arabic recognition** is usable but not perfect: dense lines can lose
  characters and Arabic-Indic digits are not read reliably yet. The Precision
  tier uses
  [PP-OCRv5 Arabic](https://huggingface.co/PaddlePaddle/arabic_PP-OCRv5_mobile_rec_onnx),
  which reads clean letterforms noticeably better once its visual-order output is
  converted, but digit runs and mixed Latin text stay weak. It is an optional
  download rather than the default because it has not been measured against a
  labelled corpus of real scans.
- **`.doc` and `.ppt` are refused, not read** — both are binary OLE formats whose
  text lives in compressed streams. `.docx`, `.xlsx`, `.odt` and legacy encoded
  text are supported.
- **Spreadsheet formulas are not evaluated** — a formula cell shows the cached
  result the file stores, which is what Excel last calculated.
- **No auto-update** in this release; download a newer installer when one is
  published.
- **The 0.3.0 packages do not upgrade 0.1.0 in place** because the product name
  changed: remove the old *Zeidex Studio* entry (Apps & features on Windows,
  `sudo apt remove zeidex-studio` on Debian/Ubuntu) before installing *Zeidex
  Search*. The index, settings and OCR history in `~/.zeidex` are untouched.
- **macOS** builds are not produced by this workflow. The shell compiles on macOS
  from source, but no signed `.dmg` ships here.

---

## Build from source

```bash
git clone https://github.com/zeiad6/zeidex-search.git
cd zeidex-search
make models            # one-time download of the ONNX models
make core-test         # Rust core test suite
make frontend-install  # npm install
make tauri-build       # platform bundle
```

Requirements: Rust 1.99.1 (pinned in `rust-toolchain.toml`), Node.js 22, and the
Tauri system dependencies for your platform. See
[`assets/docs/build-and-release.md`](https://github.com/zeiad6/zeidex-search/blob/main/assets/docs/build-and-release.md).

---

## Links

- [README](https://github.com/zeiad6/zeidex-search#readme) — screenshots, architecture and configuration
- [Contributing](https://github.com/zeiad6/zeidex-search/blob/main/CONTRIBUTING.md)
- [Issues](https://github.com/zeiad6/zeidex-search/issues)
- [Developer card](https://zeidexlink.zeidexsoft.com/zeidex) · [zeidexsoft.com](https://zeidexsoft.com)

<p align="center"><sub>Copyright © 2026 Zeidexsoft · GPL-3.0-or-later · Commercial use permitted</sub></p>
