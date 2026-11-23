---
name: zeidex-ocr-organizer
description: Work on Zeidex's OCR Data Intelligence layer — document-type profiles, field extraction, the one-row-per-page batch table, and the TXT/CSV/JSON/Markdown/HTML exporters. Use when adding or changing a document type, a field alias, a warning code, an export format, or the Data intelligence panel.
---

# Zeidex OCR organizer

The organizer turns recognized `PageResult`s into a document. It is rule-based
on purpose; the value of the feature is that it never invents anything.

## Where things are

| Concern | File |
| --- | --- |
| Neutral input model (`SourceDocument`, `SourcePage`, `SourceBlock`) | `core/organizer/src/model.rs` |
| Layout/block detection, language, stats, warnings | `core/organizer/src/detect.rs` |
| Profiles, aliases, detection scoring, extraction, batch | `core/organizer/src/extract.rs` |
| Serializers (TXT, CSV, JSON, Markdown, HTML) | `core/organizer/src/render.rs` |
| OCR `PageResult` → `SourceDocument`, Tauri commands, export preview | `src-tauri/src/commands/organize.rs` |
| IPC types | `app/src/api.ts` |
| Presentation rules (labels, tones, TSV, tab→subject) | `app/src/organizer-view.ts` |
| Panel | `app/src/components/ocr/DataIntelligence.tsx` |
| Studio integration (frozen snapshot, focus return) | `app/src/components/ocr/OcrStudio.tsx` |
| Dev-only browser fixtures | `app/src/demo/organizer.ts`, `app/src/demo/mock.ts` |

## The contract — do not break these

1. **One page is one document, one row.** `extract_batch` emits a row for every
   page in page order, including pages with no fields. Never filter or renumber.
2. **No invention.** No value is rewritten, no total is computed, no field is
   filled from a guess. If it is not on the page, it is absent or flagged.
3. **`fold` is for matching only.** Stored and exported text keeps its original
   characters (hamza forms, ta-marbuta, Arabic-Indic digits).
4. **A manual profile applies to the batch too.** `extract_as` and
   `extract_batch_as` must stay in step; the shell passes the same `profile`
   argument to both commands.
5. **Preview and export share one renderer.** `preview_export` must call the same
   `render_*_for_export` function the export command calls. A test compares them
   byte for byte.
6. **PDF is not offered.** There is no Arabic-shaping PDF writer; an unsupported
   format is an error, not a fake file.
7. **Every warning code needs a label in both languages.** A test scans
   `core/organizer/src/*.rs` for `code:` and `*_missing` literals and fails
   otherwise.

## Add a document type

1. `DocumentKind` in `extract.rs`: add the variant, `as_str()` (snake_case id),
   `label()`, `label_ar()`, and `from_id`.
2. `profile(kind)`: the ordered field keys. Order is the column order.
3. `spec_for(key)`: label, Arabic label, and aliases (Arabic and English). Add a
   short alias only if it is unambiguous; `match_spec` prefix-matches only when
   the alias has ≥2 words or ≥9 characters.
4. `keyword_score(kind, text)`: the words that identify the type, folded.
5. `app/src/organizer-view.ts`: add the id to `DOCUMENT_KINDS` with both labels.
6. Tests: a positive extraction, a negative (wrong profile → no fields), and a
   batch case if the type is commonly stacked.

## Add a warning code

Emit it from `extract.rs` or `detect.rs`, then add `{ label, labelAr }` to
`WARNINGS` in `app/src/organizer-view.ts`. The scanning test in
`app/src/organizer-view.test.ts` fails until you do.

## Add an export format

1. `render.rs`: the body from the block list (never from a second source).
2. `organize.rs`: add the arm to `render_for_export`, `render_extraction_for_export`
   and `render_batch_for_export`, and an extension.
3. `api.ts`: widen the format union.
4. `DataIntelligence.tsx`: a tab (if it has a preview) and a footer button.
5. `organizer-view.ts`: `subjectForTab` and `PREVIEW_FORMAT` if the tab is new.
6. Tests: `every_text_format_has_an_extension`, the extraction and batch
   equivalents, plus `a_preview_matches_the_exported_body`.

## Gates

```bash
cargo test --manifest-path core/Cargo.toml -p zeidex-organizer --offline
cargo test --manifest-path src-tauri/Cargo.toml --lib --offline
cd app && npm test && npx tsc --noEmit && npm run build
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --offline \
  -- -D warnings -D clippy::unwrap_used -D clippy::expect_used
cargo fmt --all --manifest-path core/Cargo.toml -- --check
```

A new frontend test module must be listed in `app/tsconfig.test.json`, or
`npm test` will not compile it.

## Visual check

`?demo=ocr` installs the mock bridge in development. Open the Studio, run OCR,
then open **Data intelligence**. The organizer commands are answered by
`app/src/demo/organizer.ts`; exports and previews are intentionally not faked
there. `contrast.test.ts` fails on any text token that is painted but not
declared, so a new colour needs a row in `TEXT_TOKENS`.
