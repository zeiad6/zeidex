# Rust core

A Cargo workspace in `core/` (edition 2024, Rust 1.88+). Each crate has one job.
The workspace builds and tests on its own — no Tauri, Node or webview needed.

| Crate | Role |
| --- | --- |
| `zeidex-indexing` | The `Candidate` type (app, file, folder, setting, …) and its id scheme. Shared by every other crate. |
| `zeidex-matching` | Query normalization (case and accent folding) and fuzzy scoring. |
| `zeidex-ranking` | `rank_score`: adjusts a base match score using exact/prefix matches and usage history. |
| `zeidex-storage` | SQLite persistence: candidates, usage events, index state, URL history, clipboard entries, in-memory settings. |
| `zeidex-engine` | The query engine: config, discovery, search, scoring, modes, platform detection. Depends on the crates above. |
| `zeidex-tools` | Preferred tools (editor, terminal): turns an action plus an object into something the platform can launch. |
| `zeidex-answers` | URL helpers (`classify_url`) and, behind the non-default `translate` feature, HTTP translation. |
| `zeidex-ocr` | Offline document AI: layout (PP-StructureV2), table structure (SLANet), Arabic/English recognition (PP-OCRv4), Arabic BiDi, PDF rendering. |
| `zeidex-exporter` | Writes a recognized page to editable `.docx` and `.xlsx`. |

## Engine (`core/engine`)

- `config.rs`, `config_path.rs`: parse `~/.zeidex/config` into `RuntimeConfig`.
- `index/`: discovery of candidates. `apps.rs`, `files.rs`, `settings.rs` each run on their own thread and stream `Candidate`s over a bounded channel.
- `platform/`: per-OS discovery and settings catalogs (`macos`, `linux`, `windows`).
- `query.rs`, `search.rs`, `scoring.rs`: parse a query, score candidates, keep the top-K in a heap, then re-rank a bounded pool.
- `action.rs`, `result.rs`: what a result is and what can be done with it.
- `bootstrap`: the SQLite walk that rebuilds the candidate table, whole or scoped.

## Search pipeline

```text
query ─▶ normalize ─▶ fuzzy score every candidate ─▶ top-K heap
      ─▶ re-rank the pool with ranking::rank_score (usage, exact/prefix bonus)
      ─▶ kind/path penalties and bonuses ─▶ limit ─▶ results
```

Search aliases from the config (`alias_code=Visual Studio Code|Cursor|Zed`) are applied to app queries before scoring.

## OCR engine (`core/ocr_engine`)

`ort` (ONNX Runtime, loaded dynamically) drives five sessions; `pdfium-render`
renders PDF pages. The pipeline is:

```text
image/PDF page ─▶ layout (PicoDet) ─▶ per-region: text lines (DBNet)
      ─▶ recognition (PP-OCRv4, EN or AR) ─▶ Arabic BiDi ─▶ table structure (SLANet)
```

Modules: `onnx.rs` (sessions, model paths, detection/recognition), `layout.rs`,
`table.rs`, `pico.rs` (PicoDet decode), `bidi.rs`, `dict.rs` (character
dictionaries), `geometry.rs`, `pdf.rs`, `pipeline.rs` (the `OcrEngine`).

Two details are load-bearing:

- **Detection boxes.** The long side of the page is capped at
  `DETECT_MAX_SIDE` (default 1600, configurable through
  `OcrEngineConfig::det_max_side`), so 300 dpi pages keep their stroke detail.
  Boxes are scaled back by the factor actually applied and then expanded with
  PaddleOCR's `unclip` ratio, because DBNet's probability mask covers only the
  middle band of a line; cropping it raw cuts off Arabic letter forms.
- **Table structure.** SLANet is a *sequence* decoder: structural tokens
  (`<thead>`, `<tr>`, `</tr>`, `<tbody>`) drive the grid and only cell tokens
  become cells, with spans read from `colspan`/`rowspan`. The class list comes
  from `table_structure_dict.txt`, never from a hardcoded index. Regions the
  layout model did not label `table` fall back to `table::from_ruling_lines`,
  which recovers a grid from 3+ dark rows and columns and requires at least one
  recognized cell, so a framed figure cannot become a table.

Models are downloaded once into `third_party/models` (`make models`) and loaded
from `ZEIDEX_MODELS_DIR` at runtime, then from the bundled resources, then a
development checkout. Hardware acceleration is opt-in (`cuda` / `directml`
features); the default build is CPU-only.

## Exporter (`core/exporter`)

- `docx.rs`: a native Word document with `w:bidi` on Arabic paragraphs *and*
  table cells, real tables with merges, bold titles, and a page break between
  pages (`export_docx_pages`).
- `xlsx.rs`: a real spreadsheet with one worksheet per page, RTL sheet view,
  merged cells, numeric cells kept as numbers, and a row cursor per table so
  two tables on one page cannot overwrite each other (`export_xlsx_pages`).

## Tests and benchmarks

```bash
cargo test --workspace --manifest-path core/Cargo.toml
```

Real-model smoke tests (`ocr_engine/tests/`) skip unless `ZEIDEX_TEST_MODELS_DIR`,
`ZEIDEX_TEST_IMAGE` and `ORT_DYLIB_PATH` are set, so a plain `cargo test` stays
offline and fast. `make ocr-test` wires all three for the committed fixture
(`ocr_engine/tests/fixtures/table.png`), and the `ocr-models` CI job runs the
same tests against the real models.

Benchmarks and stress tests live in `tools/perf`, a separate crate that is never bundled.
