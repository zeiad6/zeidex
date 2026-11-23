# Zeidex 0.4.0 — engineering report

Evidence only. Every number below came from a command run in this tree on
2026-11-23, or from a screenshot of the real React tree. Anything that could not
be exercised here is in section J instead of being described as done.

Environment: Linux arm64 (Termux/PRoot), no Windows machine, no Windows
toolchain. Rust 1.99.1, Node 22.22.1, six cores. ONNX Runtime and pdfium from
`src-tauri/resources/lib`.

---

## ملخص تنفيذي

هذا الإصدار يضيف طبقة **ذكاء البيانات** إلى استوديو المستندات: النص المستخرج من
OCR يُقرأ كمستند لا كنص خام. يكتشف النوع (بطاقة هوية، جواز، فاتورة، إيصال، كشف
حساب، شهادة، بطاقة عمل، خطاب، نموذج، جدول)، ويستخرج الحقول الموسومة إلى جدول،
ويعطي كل صفحة صفًا مستقلًا عند مسح مجموعة بطاقات، مع الحفاظ على الترتيب وعدم
إسقاط أي صفحة. المحرك قائم على قواعد حتمية: لا يعيد كتابة قيمة، ولا يحسب إجماليًا
غير مطبوع، ويعلّم ما ليس متأكدًا منه بدلًا من تخمينه. لوحة البيانات تعرض المصدر
والمنظّم والحقول والدفعة ومعاينات JSON/CSV/Markdown، وتصدّر TXT/CSV/JSON/MD/HTML.
تصدير PDF غير مضمّن لأن البناء لا يملك كاتب PDF يشكّل العربية، وزر لا يعمل أسوأ
من غيابه.

---

## A. Summary

The previous release made recognition accurate. This one makes the result
*usable*: a page of OCR text becomes a document with a type, a field table, a
per-page batch table and five export formats, and the whole thing is reachable
from the Studio without leaving the window.

| Area | Change | Verified here |
| --- | --- | --- |
| Document profiles | 11 document types; eight ordered field specs plus label-driven columns for the rest | 18 extraction tests, 12 organizer lib tests |
| ID cards | Name + national number columns, Egyptian and Saudi identifier shapes, unlabeled cards extracted but flagged | Tests `an_arabic_id_card_is_read_into_named_fields`, `an_unlabeled_card_is_extracted_but_flagged` |
| Batch | One page = one row, page order preserved, blank page keeps its slot, duplicate identifiers reported with both pages | `a_batch_of_id_cards_keeps_one_row_per_card_in_page_order`, `a_blank_card_keeps_its_row_instead_of_shifting_the_rest`, `a_repeated_identifier_is_reported_with_both_pages` |
| Profile override | A manual document type applies to the single document *and* every batch row | `a_manual_profile_applies_to_every_batch_row`, shell `a_manual_profile_reaches_the_batch_table` |
| Panel | Fields / Batch / Organized / JSON / CSV / Markdown, editable cells, undo, warning strip, review counter | 102 frontend tests, axe 0 violations, 3 screenshots, usable at 520×360 |
| Export | TXT, CSV, JSON, Markdown, HTML from one block list through the Rust save dialog | 12 shell organizer tests incl. `a_preview_matches_the_exported_body` |
| Defects | Two unlabeled warning codes, a mislabeled batch column, 12 clippy warnings, a missing batch override, a demo bridge gap | Fixed and tested in this pass (section G) |
| Version | 0.4.0 across workspace, shell, Tauri config, npm package and lockfile | `grep`, release build |

Gates, all re-run after the last edit:

| Gate | Result |
| --- | --- |
| `cargo test --workspace` (core) | **620 passed**, 0 failed, 2 ignored |
| `cargo test --lib` (shell) | **81 passed**, 0 failed |
| `npm test` (frontend) | **102 passed**, 0 failed |
| Core clippy (`-D unwrap_used -D expect_used`) | 0 errors, **0 warnings** |
| Shell clippy (`-D warnings -D unwrap_used -D expect_used`) | 0 errors |
| `cargo fmt --check` (core and shell) | clean |
| Frontend build | 429.98 kB JS / 75.63 kB CSS (gzip 126.04 / 13.45) |
| Release binary | built in 7m08s, launches under Xvfb |
| Arabic OCR gate | CER **0.60 %**, WER **3.45 %** on the 0.4.0 build (167 chars, 29 words) |

---

## B. What a "training" is here, and what it is not

The request was to *train* the formatter per document type. This release
implements that as **deterministic extraction profiles**, not as a model, and the
distinction is deliberate:

- A model trained on a handful of samples would be unverifiable here — no
  Windows corpus, no labelled Arabic ID set, no way to measure a regression on
  the user's own documents.
- Every profile is a list of field keys with aliases in Arabic and English. A
  label is matched after folding, so `الاسم` / `الاسم:` / `الأسم` all reach the
  same field. What the profile cannot match, it does not invent.
- The profile is data, not code: adding a document type means adding a spec
  table, and the tests that scan the profile set keep it honest.

### B1. The profiles

Eleven kinds ship: `id_card`, `passport`, `invoice`, `receipt`, `bank_statement`,
`certificate`, `business_card`, `letter`, `form`, `table`, `unknown`.

Each has an ordered field list. For example:

| Kind | Fields (order is the column order) |
| --- | --- |
| ID card | full name, national ID, date of birth, gender, nationality, address, issue date, expiry date |
| Passport | full name, passport number, nationality, date of birth, place of birth, gender, issue date, expiry date |
| Invoice | invoice number, invoice date, vendor, customer, subtotal, tax, total, currency, payment method |
| Receipt | merchant, receipt number, date, total, currency, payment method |
| Bank statement | account holder, account number, IBAN, statement date, opening balance, closing balance |
| Business card | full name, job title, company, phone, email, website, address |

`form`, `table` and `unknown` intentionally have **no** fixed profile: they
carry the text, not a claim about fields. They are not left empty either — every
labeled pair the page contains is kept as a `custom:<label>` field, appended
after the profile fields in first-seen order and flagged for review. A stack of
forms therefore gets one column per label it actually uses, and the batch table
builds those columns from the pages themselves. An extraction with no fields at
all returns the `no_fields` warning and the organized view still has everything.

### B2. Detection

`detect_kind` scores each kind from keyword hits in the block text, then breaks
ties by the presence of identifier shapes:

- Egyptian national ID: 14 digits, first digit 2 or 3, month 1–12, day 1–31.
- Saudi national ID: 10 digits starting with 1 or 2.
- A card with an identifier but no readable title is still detected as an ID
  card and every field is flagged for review — the test
  `an_unlabeled_card_is_extracted_but_flagged` asserts exactly that.
- A page with three or more labeled pairs and no stronger keyword evidence is a
  form. Three pairs is the floor: the score is 0.5 per pair, so 1.5 clears the
  detector's evidence threshold exactly (it used to score 1.35 and fall through
  to Unknown).
- A table with no keyword evidence is a table (`has_table`), and everything else
  is Unknown rather than a guess.

Detection is folded (`fold`) before matching, so `بطاقة تحقيق الشخصية` matches
regardless of alef form, and `ID CARD` matches `id card`.

### B3. Folding rules

`fold` unifies أ/إ/آ → ا, ى → ي, ة → ه, strips tatweel and harakat, lowercases
Latin, and maps Arabic-Indic digits to ASCII. It is used for **matching only**;
stored and exported text is never folded. Every rule has a test in
`core/organizer/tests/extract.rs`.

### B4. Review, not guessing

`REVIEW_THRESHOLD` is 0.6. A field below it is marked `needs_review` and the
extraction carries a `field_needs_review` warning naming the field and the value.
A manual edit clears the flag (a human has now looked at it) but keeps the OCR
confidence, because the confidence is a fact about the read, not about the edit.
That behaviour is pinned by
`a_manual_correction_clears_its_review_flag_and_touches_nothing_else`.

---

## C. Batch: one page is one document

The batch path exists because the common real task is a stack of cards, not one.
Its contract:

1. **One row per page, always.** The row is emitted even when the page produced
   nothing, so page 3 cannot silently become page 2.
2. **Page order is the row order.** Rows carry the page index; the UI prints
   `page + 1`. Nothing renumbers.
3. **A blank page keeps its slot.** `a_blank_card_keeps_its_row_instead_of_shifting_the_rest`
   asserts the empty row exists between its neighbours.
4. **Duplicates are reported, not removed.** `duplicate_value` names the column
   and both pages: *"The same National ID "298…" appears on pages 1 and 3."*
   Two identical cards are a real thing; dropping one would hide it.
5. **The manual profile applies to every row.** `extract_batch_as` forces the
   profile, and the shell command takes the same `profile` argument as the
   single-document command, so the table and the field view cannot disagree.

The batch table is also a real table in every serializer: CSV writes a header and
one line per page, Markdown writes a pipe table, JSON keeps the batch shape
(columns + rows + warnings), and TXT writes the same rows tab-separated.

---

## D. The Data Intelligence panel

### D1. Where it lives

The Studio footer gains one button, **Data intelligence**, enabled when pages
exist. It opens as a full-panel overlay (`absolute inset-0 z-30`) over a **frozen
snapshot** of the pages, not as a bottom drawer:

- A drawer that reflows the studio would break at the 520×360 minimum window.
- The snapshot means the analysis cannot change under the user while the panel
  is open, and it runs once.
- Closing restores focus to the button that opened it.

### D2. Tabs are the subject and the format

| Tab | Shows | Exports |
| --- | --- | --- |
| Fields | The extraction's field table, editable | the edited `Extraction` |
| Batch | One row per page with the page column | the `BatchExtraction` |
| Organized | The organized document, with a "Show source" toggle | the `OrganizedDocument` |
| JSON / CSV / Markdown | The exact body the exporter would write | the edited `Extraction` |
| — | — | HTML (export only; there is no HTML tab) |

The preview is not a second renderer. `preview_export` calls the same
`render_*_for_export` functions the export command calls, so the pane cannot
drift from the file. A shell test (`a_preview_matches_the_exported_body`) compares
the two byte for byte, and `an_unsupported_preview_format_is_an_error` pins the
PDF refusal.

### D3. Editing model

- A field is an `<input>`; each keystroke replaces the value in place.
- Edits push the previous extraction onto a bounded history (50), so **Undo edit**
  is a real snapshot, not a recomputation.
- The edit clears `needs_review` on that field only.
- The review counter in the footer counts flagged fields (Fields tab) or flagged
  rows (Batch tab), so the number always describes what is on screen.

### D4. Warnings

The organizer emits nine codes. The panel shows them once each, in first-seen
order, with a count when repeated, and the full messages in the tooltip. A code
the UI has not learned is printed verbatim rather than hidden — and a test scans
`core/organizer/src/*.rs` for every `code:` and `*_missing` literal and fails if
one has no English and Arabic label. That test is why `national_id_missing` and
`name_missing` are labelled in this release instead of leaking as identifiers.

### D5. Accessibility

- `role="region"` with an accessible name, `tabIndex={-1}`, and focus moved into
  the panel on mount; Escape closes it.
- The tab strip is a real `tablist` with roving `tabIndex`, Arrow Left/Right
  (direction-aware in RTL), Home and End.
- Tables use `<th scope>`, the page column is a row header, and every field input
  has an accessible name.
- axe-core 4.10.2, WCAG 2.0/2.1/2.2 A+AA, against the whole document with the
  panel open: **0 violations** in dark English, dark Arabic and light English.

### D6. RTL, themes, and the 520×360 minimum

The panel uses logical utilities (`ms-`, `me-`, `text-start`, `text-end`), and
the screenshots in `assets/screenshots/data-intelligence-*.png` are the real
component at 1440×960 in dark LTR, dark RTL and light LTR. In RTL the page
column moves to the right, the confidence column to the left, and `dir="auto"`
on every value cell keeps `2026-014` readable inside Arabic text.

At the 520×360 minimum window the panel is fully usable: the tab strip fits, the
field table scrolls vertically, the footer keeps every export button, and the
document is exactly 520×360. That measurement is what exposed the scroll defect
in G — the overlay had opened 25 px off the left edge with its first column cut.

---

## E. Export

One block list feeds every serializer (`core/organizer/src/render.rs`), so a
number cannot appear in one format and vanish from another:

| Format | Shape |
| --- | --- |
| TXT | Blocks joined by a blank line; tables as tab-separated rows |
| CSV | Table(s) only when a table exists, else `kind,text`; RFC-style quoting |
| JSON | Pretty-printed document, schema-tagged |
| Markdown | `#` titles, `-` lists, pipe tables, escaped markup |
| HTML | Escaped, self-contained, RTL when the document is Arabic |

**PDF is not implemented.** The build has no PDF writer with Arabic shaping;
pdfium is a reader. The preview returns an error for `pdf`, and the UI does not
offer the button. Section J records this as unavailable rather than planned.

All five text formats go through the Rust-owned save dialog
(`commands::dialogs::save_file`), so the page never chooses the path it writes
to, and the write is atomic.

---

## F. Wiring

The organizer crate is independent of ONNX, PDFium and the recognition stack:

```text
PageResult (OCR)  ──source_document()──▶  SourceDocument (organizer)
                                              │
                                    organize()│extract() / extract_batch()
                                              ▼
                        OrganizedDocument · Extraction · BatchExtraction
                                              │
                          render_*_for_export()│  (one body per format)
                                              ▼
                                   save dialog → file
```

Seven commands are registered: `organize_pages`, `extract_document`,
`extract_document_batch`, `preview_export`, `export_organized`,
`export_extraction`, `export_batch`. The TypeScript side mirrors the Rust types in
`app/src/api.ts`, and `app/src/organizer-view.ts` holds only presentation rules
(labels, tones, TSV, tab→subject) so the component stays a view.

---

## G. Defects found and fixed in this pass

| Defect | Why it mattered | Fix |
| --- | --- | --- |
| `national_id_missing` and `name_missing` had no label | The panel would print a raw identifier at an Arabic user | Added both labels, plus a test that scans the crate for every warning code |
| Batch column headed **Filled** showed a confidence percentage | A number under the wrong name is misinformation | Renamed to **Confidence** / **الثقة** |
| 12 clippy warnings in the organizer (collapsible `if`, manual char comparison, manual range) | A shipped crate should not carry noise | `cargo clippy --fix`, formatted, 0 warnings |
| `extract_document_batch` had no profile argument | Correcting the document type fixed the fields view and left the table wrong | Added `profile`, `extract_batch_as`, and two tests |
| Demo bridge answered `null` to every organizer command | The panel could not be reviewed or audited in a browser | Dev-only fixtures in `app/src/demo/organizer.ts`, imported lazily and never in a release build |
| `.panel` used `overflow: hidden`, so opening the panel scrolled the studio 38 px sideways and the overlay started 25 px off-screen at 520 px width | The first table column and every tab label were cut at the minimum window size | `overflow: clip` (not a scroll container) + `focus({ preventScroll: true })`; `layout-invariants.test.ts` pins both |
| The launcher panel had no height bound: at 360 px tall the document grew to 605 px | The whole window scrolled, title bar included | `max-h-full` on the panel; the results list scrolls in 113 px instead |
| A form with three labeled pairs was never detected: 3 × 0.45 = 1.35 fell under the 1.5 evidence floor, so the Form branch was dead code | "Form" existed in the enum and the profile picker but auto-detection could not reach it | 0.5 per pair (3 × 0.5 clears the floor exactly); `a_batch_without_a_fixed_profile_builds_columns_from_its_own_labels` covers detection, column order and per-page alignment |
| Version still 0.3.0 in five files | A release that lies about its version | 0.4.0 in workspace, shell, `tauri.conf.json`, `package.json`, lockfile |

---

## H. What was actually run

```text
core:    cargo test --workspace --manifest-path core/Cargo.toml --offline
         → 620 passed, 0 failed, 2 ignored
         cargo clippy --workspace --lib ... -D clippy::unwrap_used -D clippy::expect_used
         → 0 errors, 0 warnings
         cargo fmt --all -- --check → clean

shell:   cargo test --lib --manifest-path src-tauri/Cargo.toml --offline
         → 81 passed, 0 failed
         cargo clippy --all-targets ... -D warnings -D clippy::unwrap_used -D clippy::expect_used
         → 0 errors
         cargo fmt --all -- --check → clean

front:   npm test → 102 passed, 0 failed
         npx tsc --noEmit → clean
         npm run build → 429.95 kB JS, 75.32 kB CSS

binary:  cargo build --release (7m08s) → src-tauri/target/release/zeidex
         Xvfb: window maps 760×520, full schema created,
               5079 candidates indexed (243 apps, 3488 files, 1348 folders)

ocr:     ZEIDEX_TEST_MODELS_DIR=third_party/models \
         ORT_DYLIB_PATH=src-tauri/resources/lib/libonnxruntime.so \
         cargo test -p zeidex-ocr --test arabic_cer --offline
         → 1 passed; aggregate cer=0.60% wer=3.45% search-cer=0.60%
           over 167 characters, 29 words
```

The frontend gate is not a formality: `contrast.test.ts` scans every component
for text tokens and fails on an undeclared colour (it caught `zinc-700` in this
pass), and `organizer-view.test.ts` now scans the Rust crate for warning codes.

---

## I. Screenshots

| File | What it shows |
| --- | --- |
| `assets/screenshots/data-intelligence-fields.png` | Invoice profile, five fields, 58 % total flagged, warning strip, export footer |
| `assets/screenshots/data-intelligence-batch.png` | Three pages, one row each, duplicate identifier warning ×2, 3 to review |
| `assets/screenshots/data-intelligence-rtl.png` | The same panel in Arabic RTL |

They are element screenshots of the real React tree at 1440×960, 2× DPR, driven
through the demo bridge in headless Chrome — the same harness the earlier
screenshots used.

---

## J. Not verified here, stated plainly

| Item | Status | Why |
| --- | --- | --- |
| Windows runtime (opaque window, DPI, multi-monitor, tray, dialogs) | **Not verified** | No Windows machine or WebView2 in this environment |
| MSI / NSIS build, install, upgrade, uninstall | **Not built** | No Windows target, no WiX/NSIS toolchain |
| PDF export | **Not implemented** | No Arabic-shaping PDF writer; pdfium is read-only |
| GPU acceleration | **Not implemented** | ONNX Runtime CPU build ships; a GPU toggle without a GPU EP would be a lie |
| Arabic CER on this exact build | **Re-measured** | `arabic_cer`: aggregate CER 0.60 %, WER 3.45 %, search-CER 0.60 % over 167 characters and 29 words; the gate (`MAX_CER 0.08`, `MAX_WER 0.15`) passes |
| WebKitGTK pixels under Xvfb | **Black** | WebKitGTK 2.52.6 paints nothing under Xvfb; UI pixels were verified in Chrome instead |

Everything in this table is a limit of the environment or of the shipped
binaries, not a task that was silently skipped.

---

## K. Files

New:

- `core/organizer/` — `model.rs`, `detect.rs`, `extract.rs`, `render.rs`,
  `lib.rs`, `tests/extract.rs`
- `src-tauri/src/commands/organize.rs`
- `app/src/organizer-view.ts`, `app/src/organizer-view.test.ts`
- `app/src/components/ocr/DataIntelligence.tsx`
- `app/src/demo/organizer.ts`
- `assets/screenshots/data-intelligence-*.png`
- this report

Modified: `app/src/api.ts`, `app/src/components/OcrStudio.tsx`,
`app/src/demo/mock.ts`, `app/tsconfig.test.json`, `src-tauri/src/lib.rs`,
`src-tauri/src/commands/mod.rs`, `core/Cargo.toml`, `core/Cargo.lock`,
`src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, `src-tauri/tauri.conf.json`,
`app/package.json`, `app/package-lock.json`, `README.md`, `NOTICE`, and the
0.3.0 hardening files already in the tree.
