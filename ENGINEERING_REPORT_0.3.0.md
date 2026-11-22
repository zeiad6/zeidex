# Zeidex 0.3.0 — engineering report

Evidence only. Every command in section I was executed in this tree on
2026-11-22; every number is a measurement from that run. Anything that could not
be exercised here is listed in section K instead of being described as done.

Environment: Linux arm64 (Termux/PRoot), no Windows machine, no display server.
Rust 1.99.1, Node 22, ONNX Runtime and pdfium from `src-tauri/resources/lib`.
Screenshots were taken from the real React tree in a headless browser against the
demo bridge, at 2× device pixel ratio.

---

## A. Summary

This release is measured in two places where the previous one could only claim
progress: Arabic recognition, where a word-level re-read took the character error
rate from 15.57 % to 0.60 %, and startup/index reliability, where a real race
between two SQLite connections could abort the app. It also adds the two things
asked for directly: official per-format file badges in every list, and launch at
login.

| Area | Change | Verified here |
| --- | --- | --- |
| Arabic OCR | Low-confidence Arabic lines are re-read word by word and merged | `arabic_cer`: CER 15.57 % → 0.60 %, WER 31.03 % → 3.45 % |
| Benchmark | Corpus font defect fixed; generator refuses undrawable characters | Corpus regenerated, mixed line now exact |
| Index | `ensure_column_exists` is idempotent under concurrent migration | New test, plus the failing `index_control` test now passes |
| File formats | Drawn badges for PDF/Word/Excel/PowerPoint/image/text/code/archive/audio/video | 7 new node tests, screenshots |
| Startup | Launch at login on Windows (registry), Linux (XDG), macOS (LaunchAgent) | 4 unit tests + a real write/read/remove cycle on Linux |
| Shutdown | Quit releases the global shortcut before exit; same on the exit path | Code path only; **runtime not verified** |
| UI | RTL base direction for Arabic file names in lists; format chips | Screenshots |
| Name matching | Exact / start / end / inside / partial / close tiers, typo tolerant | 26 new core tests, 5 node tests, release-mode latency bench |
| Naming | Product renamed **Zeidex Search**, repository `zeidex-search`, package names follow | Source, config, AppStream, workflow and assets updated; binary/identifier/data path unchanged |

105 files changed in one commit — 81 modified and 24 new (`FileGlyph.tsx`,
`WindowControls.tsx`, `file-format.ts`, `match-kind.ts` and their tests,
`autostart.rs`, `shortcut.rs`, the Arabic corpus and PDF fixtures, six
screenshots, the latency benchmark, the 0.2.0 and 0.3.0 reports and these notes)
— 5865 insertions and 267 deletions.

### A1. The rename, and what it does not touch

The product opened on a search field but was called *Zeidex Studio* — a name that
describes the document workspace, which is one mode inside the window. It is now
**Zeidex Search**, tagline *Offline search, OCR and document AI*, and the
repository is `zeidex-search`.

Three identifiers are load-bearing and were left alone on purpose:

| Kept | Why |
| --- | --- |
| binary `zeidex` | shortcuts, `PATH` entries and the autostart `Exec` line keep working |
| identifier `com.zeidexsoft.zeidex` | the webview/application identity does not change |
| `~/.zeidex` | an upgrade must not orphan the index, settings or OCR history |

Everything derived from `productName` did change: the Linux package
(`zeidex-studio` → `zeidex-search`), the desktop file
(`Zeidex Search.desktop`), the resource directory (`/usr/lib/Zeidex Search/`),
the AppStream `<name>`/`<summary>`/launchable, the tray tooltip, the window
title, the wordmark eyebrow and the README banner. The release workflow's
post-install check was updated in the same commit, because a `dpkg -s
zeidex-studio` there would fail the Linux job on the first tagged build.

Two consequences are recorded rather than hidden. A `.deb` whose name changed is
a *different* package to dpkg, and both versions own `/usr/bin/zeidex`, so the
old package must be removed before the new one installs. The MSI upgrade code is
derived from the product name, so 0.3.0 installs beside a previously installed
*Zeidex Studio* instead of replacing it. Both are one-line instructions in the
release notes, and neither touches user data.

---

## B. Arabic OCR: the word-level re-read

### B1. Why the line was the wrong unit

`recognize_line` fed the whole detected line to the recognizer as one image. On a
connected script a single ambiguous word biases the decoder for the rest of the
line, and the model has no way to recover — the CTC path that produced the bad
word has already consumed the columns. The corpus showed exactly that shape: the
line `اختبار PDF` came back as one token where the truth had two, and the greeting
lost the final `الله` to `اله`.

### B2. What the fix does

- A line whose recognizer confidence is below `WORD_MERGE_CONFIDENCE = 0.80` **and**
  whose text contains Arabic is split at word gaps by `segment_columns`, which
  distinguishes a word gap from a letter gap by width relative to the median
  glyph advance, and forces a single split on a tight mixed line so `اختبار PDF`
  is not glued together.
- Each segment is recognized on its own; `choose_segment` picks the
  script-matched model per segment, so a Latin segment is not pushed through the
  Arabic model.
- The merged candidate is kept only when it beats the whole-line result on
  confidence. A clean line cannot be made worse by this path.

### B3. Measured

`cargo test -p zeidex-ocr --test arabic_cer` on the committed ten-line corpus:

| Metric | 0.2.0 | 0.3.0 |
| --- | --- | --- |
| Character error rate | 15.57 % | **0.60 %** |
| Word error rate | 31.03 % | **3.45 %** |
| Lines exact | 5 / 10 | **9 / 10** |
| `MAX_CER` / `MAX_WER` enforced | 0.20 / 0.40 | **0.08 / 0.15** |

The remaining error is one line: `السلام عليكم ورحمة الله` reads as `…رحمة اله`
(4.35 % CER, 25 % WER). It is reported as the limit, not rounded away.

### B4. Cost

The merge is gated on Arabic, so Latin and table pages keep the old cost.
`pdf_smoke` (a Latin table page) measured **30.37 s**, against **32.33 s** in
0.2.0 — inside run-to-run noise. An intermediate build that re-read every line
took **103 s** on the same fixture. The gate is what makes the feature
shippable.

### B5. The corpus was lying

`اختبار PDF` had been rendered in Noto Naskh, which has no Latin glyphs: the
"recognition failure" was tofu baked into the fixture. The line now uses
`IBMPlexSansArabic-Bold.ttf`, and `tools/make-arabic-corpus.py` checks every
character against the chosen font with fontTools and refuses to write the corpus
if any character has no glyph. A benchmark that cannot draw its own ground truth
measures nothing.

---

## C. File-format badges

Every file row used one page icon, so a PDF, an Excel sheet and a scan looked
identical. The launcher, the Document Studio and Batch Search now share one badge
set.

- `app/src/file-format.ts` maps an extension to a format id, name (English and
  Arabic), short label and colour. It is pure, so the node suite covers it
  without a DOM.
- `app/src/components/FileGlyph.tsx` draws the badge as SVG: a coloured tile, a
  white page with a folded corner, and the format's own mark. Drawn, not an
  image: it inherits the theme, stays sharp at every DPI and adds no asset.
- These are format marks, not vendor logos. Shipping Microsoft or Adobe
  trademarks inside an app would be a licensing problem, and the requirement was
  a recognizable file type, not brand artwork.
- Rows keep their accessible name: the badge is `aria-hidden` because the file
  name beside it already says what the file is; where the badge stands alone the
  caller passes `decorative={false}` and it exposes a label.
- The kind chip on a file row now names the format ("PDF", "Excel") instead of
  the word "File".

Coverage: `app/src/file-format.test.ts` — every extension the file picker offers
has a real badge (not the fallback), case-insensitive lookup, dotted names,
dotfiles, unknown extensions, and Arabic file names.

---

## D. Launch at login

`src-tauri/src/commands/autostart.rs`, surfaced as **Settings → Startup**.

- Windows: one `REG_SZ` value under
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, written with `reg.exe`
  under `CREATE_NO_WINDOW` so a GUI build does not flash a console for every
  toggle. The command is the quoted executable path; a Windows path cannot
  contain a double quote, so quoting is sufficient.
- Linux: an XDG desktop entry at `$XDG_CONFIG_HOME/autostart/zeidex.desktop`,
  with `Exec` quoted and the four reserved characters escaped per the Desktop
  Entry spec.
- macOS: a per-user LaunchAgent plist with `RunAtLoad`, the path XML-escaped.
- The switch is **read back from the OS** after every write. A group policy that
  removes the entry shows as "Inactive" instead of a switch that lies. A write
  that succeeds but leaves no entry is reported as the state that exists.
- A debug build warns that the entry would point at `target/debug`.
- Disabling removes exactly the entry Zeidex created. No service, no second
  executable, no admin prompt.

The app already starts hidden (`visible: false` in `tauri.conf.json`), so a login
start puts the launcher in the tray one shortcut away instead of opening a window
in front of the user.

---

## E. Shutdown and restart

- The tray's **Quit** unregisters every global shortcut before calling
  `app.exit(0)`, and the same release runs on `RunEvent::Exit`. A restart cannot
  race a shortcut the previous process still owns.
- There is no unsaved state to flush: SQLite runs in WAL with crash-safe
  journals, and the index worker is the only writer. `exit` is the whole
  shutdown.
- Closing the window still hides to the tray, and only when a tray exists; with
  no tray, Close ends the process rather than leaving an invisible one.

---

## F. The migration race

`ensure_column_exists` checked `PRAGMA table_info` and then ran `ALTER TABLE`.
Two connections can open the same database — the shell's store and an index
worker, or two test threads — and both run the migrations. Between the check and
the write the other connection added the column, and the loser aborted startup
with:

```
Sql(SqliteFailure(Error { code: Unknown, extended_code: 1 },
    Some("duplicate column name: copy_count")))
```

`cargo test --workspace` surfaced it as a real failure in
`engine/tests/index_control.rs::a_paused_walk_resumes_where_it_stopped`, not as a
theory. The function now runs `ALTER TABLE` directly and treats exactly that
error as success, which is what makes the operation idempotent. `PRAGMA
table_info` is gone, so the check-then-act window is gone with it.

Regression test: `adding_a_column_twice_is_idempotent` adds the same column
twice and asserts `pragma_table_info` reports it once.

---

## G. UI details

- Batch Search requested names and match names carry `dir="auto"`, so an Arabic
  filename picks an RTL base inside an otherwise LTR table instead of being
  reversed by the bidi algorithm. The same applies to launcher result titles.
- The Studio shows the opened file as a chip: badge, file name (`dir="auto"`),
  full path (`dir="ltr"`, monospace) and the format name.
- The Studio's empty state lists the formats it reads, each with its badge.
- Export buttons carry the Word and Excel badges instead of generic icons.

---

## H. Name matching: start, end, close

The launcher could only answer whether a name *contained* the query. A file whose
name ended with the typed text ranked no better than one that contained it by
accident, a typo was a miss, and the list never said why a row was there. Two
things changed: the matcher learned an end tier and a typo tier, and every
displayed row now reports the tier that surfaced it.

### H1. The tiers

| Tier | Test | Score in `zeidex-matching` |
| --- | --- | --- |
| Exact | `title == query` | 2000 |
| Prefix | `title.starts_with(query)` | 1500 − length |
| Suffix | name (or its stem) ends with the query | 1300 − length |
| Contains | substring, or every query word present | engine's 1200 / 850 |
| Fuzzy | letters in order with gaps | bounded DP |
| Approximate | within the typo budget | 600 − 110 per edit |

`ends_with_query` compares the extension-less stem, which is what makes
`2026-014` end `invoice-2026-014.pdf` — the whole name ends with `.pdf`. A title
without a dot, or a dotfile such as `.gitignore`, is compared whole. A query that
is exactly the extension is excluded from the tier: `pdf` is a format filter, and
labelling every PDF "End" would be noise. A longer query that includes the
extension still qualifies (`2026-014.pdf`).

`zeidex-ranking` adds +500 exact, +200 prefix and +120 suffix on top of the base
score, so prefix > suffix > contains survives even when the length penalty pushes
the matcher's suffix score below a containment. A one-letter query earns no
suffix bonus, or every plural (`notes`) would be boosted.

### H2. Typo tolerance

`ApproximateMatcher` is built once per search pass and owns its query tokens and
DP scratch rows, so scoring a candidate allocates nothing. `new` returns `None` —
and the pass costs nothing per candidate — when the query cannot be matched
approximately at all: any token shorter than four characters, more than four
tokens, or more than 32 characters in total.

The distance is OSA along a band of `max` diagonals; it returns `None` as soon as
the distance is known to exceed the budget, so a hopeless word costs a few rows
instead of a full table. The budget is one edit per four characters, clamped to
1..3. Each query token is compared against each title word, and also against the
title word truncated to the token's length, so a typo in the prefix of a
concatenated name (`reprot` against `reportfinal`) still matches. A candidate
reaches the matcher only when every literal tier returned `None`, which is what
guarantees a typo can never displace a real match.

The score enforces the same rule numerically: 600 at zero edits, minus 110 per
edit, and `None` when the result would be ≤ 0. The engine's all-terms score is
850 and its containment score 1200, so even with the usage bonus the approximate
tier stays below both.

### H3. Cost

`core/engine/tests/match_latency.rs` (ignored by default, release mode) builds
8,000 and 50,000 candidates and times the whole search path:

| Candidates | Prefix (`invoice`) | Suffix (`2026-01234`) | Approximate (`invoise`) | No match (`zzzzzzz`) |
| --- | --- | --- | --- | --- |
| 8,000 | 3.50 ms | 6.25 ms | 4.16 ms | 4.12 ms |
| 50,000 | 25.26 ms | 48.31 ms | 28.40 ms | 28.81 ms |

The typo pass is the only added work for candidates nothing else matched: about
3.6 ms over 50,000 candidates (~71 ns each), most of which the first-letter gate
rejects before any DP runs. The matcher alone, worst case (every title passes the
gates but matches no word), scores 50,000 titles in 14.6 ms. The suffix query's
48 ms is the pre-existing subsequence DP for the ~50,000 titles that do not end
with the query, not the new `ends_with` check. Timings vary with CPU frequency:
an earlier run of the same binary measured the no-match delta at 4.7 ms, so read
these as tens of nanoseconds per candidate, not as a fixed constant.

Classification for the UI runs after ranking, on at most `limit` rows (10 by
default, 24 in the demo), so it is outside the hot loop.

### H4. Tests and the UI

26 new core tests: 14 in `zeidex-matching` (tier order, stem rule, extension
exclusion, transposition and substitution, concatenated names, unrelated words,
short queries, multi-word queries, Arabic typos, classification, and a
banded-vs-full-table check against a reference OSA implementation), 5 in
`zeidex-ranking` (exact after folding, suffix against prefix, suffix ignoring
the extension, extension-only exclusion, one-letter guard) and 7 in
`zeidex-engine` (a query at the end of the name finds the file, tier ordering,
the tier is reported, a typo surfaces the file, an exact match outranks a typo
neighbour, short queries are never approximate, Arabic typos). Five node tests
cover the label table and the demo mirror.

`ResultList` renders the tier as a chip beside the format badge, with an Arabic
label and an explanatory tooltip; the field is absent for browse rows,
natural-language file recall, content hits and the web row, which are not name
matches. The capture `launcher-match-tiers.png` shows Start, End, Inside and
Close in one list.

---

## I. Tests and builds actually run

| Command | Result |
| --- | --- |
| `cargo test --workspace` (core) | **587 passed, 0 failed, 3 ignored** |
| `cargo clippy --workspace --lib -- -D clippy::unwrap_used -D clippy::expect_used` | clean |
| `cargo fmt --all -- --check` (core, src-tauri) | clean |
| `cargo test --lib` (src-tauri) | **69 passed**, includes 5 new autostart tests |
| `cargo clippy --all-targets -- -D warnings` (src-tauri) | clean |
| `npm test` (app) | **81 passed** |
| `npx tsc --noEmit` (app) | clean |
| `cargo test -p zeidex-ocr` (real models, whole crate) | **all pass**, 0 failed |
| ↳ `arabic_cer` | 1 passed, 2.44 s, **CER 0.60 % / WER 3.45 %** |
| ↳ `arabic_smoke`, `ocr_smoke`, `ocr_accuracy` | 1 + 1 + 1 passed |
| ↳ `pdf_native_text` | 1 passed |
| ↳ `pdf_smoke` (Latin table page) | 1 passed, **30.37 s** |
| ↳ `pdf_lifecycle` (two engines, one pdfium) | 1 passed, **198.33 s** |
| ↳ `photo_quality`, `photo_smoke`, `table_smoke`, `layout_smoke`, `pack_smoke`, `arabic_v5_smoke` | all passed |
| `cargo test -p zeidex-engine --release --test match_latency -- --ignored` | 1 passed; table in H3 |
| `npm run build` (app) | PASS |
| Release build (src-tauri) | PASS — binary **19,298,520 bytes**, sha256 `74cb45afc2274e9dec850dba163818ae08e57f7bbfefb487d49eaa7ede7f1a92` |
| `make deb` | PASS — `Zeidex Search_0.3.0_arm64.deb`, 49,696,884 bytes, `Package: zeidex-search`, `Installed-Size: 85915` |
| `dpkg-deb -I` / `-c` on the package | `zeidex-search 0.3.0`, arm64, three dependencies, `usr/bin/zeidex`, `Zeidex Search.desktop`, metainfo present |
| `dpkg -i` the package | first attempt over the installed 0.1.0 failed with `trying to overwrite '/usr/bin/zeidex', which is also in package zeidex-studio` — the documented upgrade path; after `dpkg -r zeidex-studio` it installed as `zeidex-search 0.3.0`, with `/usr/bin/zeidex`, `/usr/lib/Zeidex Search/{lib,models}`, the desktop entry and the metainfo in place |
| `timeout 12 xvfb-run -a /usr/bin/zeidex` (installed package) | exit 124 — the process was still running when the timeout fired; only `libEGL … DRI3` warnings from a GPU-less X server |
| `appstreamcli validate --no-net` | ✔ Validation was successful |

The whole table was re-run after the rename, not carried over: the counts above
are the post-rename run (the ignored three are the two batch scale checks and
the latency benchmark, all opt-in).

Environment for the real-model runs:

```
ZEIDEX_TEST_MODELS_DIR=$PWD/third_party/models
ZEIDEX_TEST_PDF=$PWD/core/ocr_engine/tests/fixtures/table.pdf
ZEIDEX_TEST_PDFIUM_LIB=$PWD/src-tauri/resources/lib/libpdfium.so
ORT_DYLIB_PATH=$PWD/src-tauri/resources/lib/libonnxruntime.so
ZEIDEX_TEST_ARABIC_IMAGE=$PWD/core/ocr_engine/tests/fixtures/arabic_lines.png
```

---

## J. Screenshots

Nine captures in `assets/screenshots/`, produced by
`/tmp/opencode/shots/shoot-030.mjs` against Vite plus a headless Chromium shell
over CDP, screenshotting the `.panel` element at 2× DPR:

`launcher-idle.png`, `launcher-search.png`, `launcher-match-tiers.png`,
`launcher-arabic-search.png`, `studio-formats.png`, `studio-document.png`,
`settings-shortcut.png`, `settings-startup.png`, `batch-search.png`.

They are the real components against the demo bridge (`?demo=…`), not mockups:
the demo answers the same Tauri commands the shell does, including the new
`autostart_status` / `autostart_set`.

---

## K. Remaining issues (honest list)

1. **Windows runtime is still unverified.** No Windows machine and no display
   server here. Console absence, dialog z-order, caption buttons, tray behaviour,
   DPI, multi-monitor, the installer and the `Run` registry entry were compiled
   and unit-tested but not exercised on Windows. The registry code path is
   reviewed and the string builders are unit-tested on every host; the `reg.exe`
   calls are not.
2. **One Arabic line is not exact.** `السلام عليكم ورحمة الله` → `…رحمة اله`,
   4.35 % CER. The ceiling is set at 0.08 so the test fails if this regresses
   further, but it does not claim perfection.
3. **Word segmentation is heuristic.** `segment_columns` splits on relative gap
   width and forces one split on a tight mixed line. A line with an unusually
   wide letter gap inside a word can be split in the wrong place; the merge only
   keeps the result if confidence improves, so the failure mode is "no change".
4. **Native PDF pages do not reconstruct tables.** A born-digital page with
   ≥ 24 letters/digits in its text layer is read as text, not re-OCRed, so its
   table structure is not recovered. Accepted in 0.2.0; unchanged.
5. **The badges are drawn format marks, not the platforms' official logos.**
   Recognizable and consistent, but not the Microsoft or Adobe artwork, for
   licensing reasons.
6. **Autostart is per-user only.** A machine-wide (all users) entry would need an
   installer-time write and elevation; it is not offered.
7. **Approximate matching is edit-distance, not phonetic.** `fone` will not find
   `phone`, and a word shorter than four letters is never typo-tolerant by
   design. Transliterations and sound-alike spellings are out of scope.
8. **The tier chip is a launcher feature.** Batch Search keeps its own score
   table and does not carry a match tier, and content hits are excluded on
   purpose: they matched text inside the file, not the name.
9. **The rename changes package identity.** Debian sees `zeidex-search` as a new
   package, and both versions ship `/usr/bin/zeidex`, so `zeidex-studio` has to
   be removed before the new `.deb` installs (verified here: `dpkg -i` refused the
   overwrite until the old package was removed). The MSI upgrade code is derived
   from the product name, so 0.3.0 installs beside an existing *Zeidex Studio*
   rather than replacing it. User data in `~/.zeidex` is untouched by either
   path, and both instructions are in the release notes.
