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
| Packaging | `.deb` built, installed, and launched under Xvfb; AppStream validates | `zeidex-search 0.3.0` arm64, `/usr/bin/zeidex`, desktop entry and metainfo present |
| Repository | Failing dependency PRs closed, their branches removed, incompatible ranges ignored; duplicate draft release deleted | `gh pr list` empty, only `main` on the remote, one release |

105 files changed in one commit — 81 modified and 24 new (`FileGlyph.tsx`,
`WindowControls.tsx`, `file-format.ts`, `match-kind.ts` and their tests,
`autostart.rs`, `shortcut.rs`, the Arabic corpus and PDF fixtures, six
screenshots, the latency benchmark, the 0.2.0 and 0.3.0 reports and these notes)
— 5866 insertions and 267 deletions.

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
| `tauri build --bundles appimage` (aarch64, this host) | **not completed** — the linuxdeploy step blocked in `poll` with no CPU for 10+ minutes and was killed; the x86_64 AppImage is built by the workflow and was inspected (I1) |

The whole table was re-run after the rename, not carried over: the counts above
are the post-rename run (the ignored three are the two batch scale checks and
the latency benchmark, all opt-in).

### I1. The workflow's own artifacts

Run [45082848860](https://github.com/zeiad6/zeidex-search/actions/runs/45082848860)
on commit `6b453ab` finished **success**: both the Windows and the Linux bundle
jobs, every step green. Its artifacts were downloaded here with `gh run download`
and inspected, not assumed:

| Artifact | Size (bytes) | SHA-256 |
| --- | --- | --- |
| `Zeidex Search_0.3.0_amd64.AppImage` | 132,545,016 | `6cba11c6bb850133bb1eac891ea0dec23d5c884421c56a0bb9dde047f7fa98b8` |
| `Zeidex Search_0.3.0_amd64.deb` | 51,180,538 | `ef0eaae53683187820b5c00c73879e80a87babcec46a6da7737a6a4643490049` |
| `Zeidex Search_0.3.0_x64_en-US.msi` | 50,769,920 | `67713344233d4da6290e93c1a2ce9c93664e5f857b6b7bdf743573ea2a21cbda` |
| `Zeidex Search_0.3.0_x64-setup.exe` | 45,672,010 | `b0f811764d1236f70a5174db03796eed256c7e1739e9645cc108641b432b5b89` |

`dpkg-deb -I` on the CI deb reports `Package: zeidex-search`, `Version: 0.3.0`,
`Architecture: amd64`, `Installed-Size: 91398` and the same three dependencies as
the local arm64 build. `msiinfo export` on the MSI reports `ProductName Zeidex
Search`, `ProductVersion 0.3.0`, `Manufacturer Zeidexsoft` and `UpgradeCode
{B072E07D-FE23-58F7-AB41-51544D8EE5D1}`; its template is `x64`. `file` calls the
NSIS setup a `PE32 executable … (GUI)`, the subsystem that keeps a console from
opening, and the AppImage an `ELF 64-bit LSB pie executable, x86-64`. The Windows
binaries were built and packaged but not executed — this host cannot run them.


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
10. **The aarch64 AppImage was not produced on this host.** `tauri build
    --bundles appimage` finished the binary and reached the linuxdeploy step,
    which then blocked in `do_sys_poll` with one second of CPU over ten minutes
    and was killed. The `.deb` from the same build installs and runs; the x86_64
    AppImage built by the workflow was downloaded and inspected (I1).
11. **Three dependency groups are deliberately not upgraded.** Dependabot opened
    grouped PRs that move `sha2` 0.10 → 0.11, `rusqlite` 0.42 → 0.43, `base64`
    0.22 → 0.23, `dirs` 6 → 7, `notify` 6 → 8, `vite` 6 → 8,
    `@vitejs/plugin-react` 4 → 6 and `typescript` 5 → 7 in one step. The first
    group fails to compile (`sha2` 0.11's digest no longer implements `LowerHex`)
    and the others change public APIs or build configuration. Each is a migration
    with its own test run, not a version bump, so the three PRs were closed and
    the incompatible ranges are ignored in `.github/dependabot.yml` until the
    migration is done. Patch releases inside the verified lines still arrive.

---

## L. Post-0.3.0 hardening (unreleased)

The changes below are in the working tree after the 0.3.0 report was written.
They are recorded separately so the released 0.3.0 evidence above stays accurate.

### L1. The Windows window is opaque

**Cause.** The base config opened an undecorated window with
`transparent: true` and `shadow: true`. tao implements Windows transparency by
calling `DwmEnableBlurBehindWindow` with an empty region, so the window is
composited by DWM instead of painted opaquely. The panel surface itself is a
`rgba()` gradient at 0.96/0.97 alpha, so what showed through was the desktop or
the previous frame: the ghost window. The panel also relied on a transparent
margin around it to look like a floating card, which cannot be produced by an
opaque window.

**Change.**

- `src-tauri/tauri.windows.conf.json` overrides the `main` window for Windows:
  `transparent: false`, `shadow: false`, `backgroundColor: "#070910"`, same
  geometry, `decorations: false`, `visible: false`. The platform file repeats
  every base field because Tauri merges window arrays by replacement.
- `app/src/main.tsx` records the platform on `<html data-os="windows">` before
  the first React render.
- `app/src/styles.css` makes `.window-shell` padding-free and `.panel`
  full-bleed, square-cornered and fully opaque on that attribute.
- macOS and Linux keep the translucent floating panel unchanged.
- The tray icon now toggles the window on left click, matching Windows
  convention; the menu stays on right click.
- `setSize`/`center` are skipped while the window is maximized: tao clears the
  maximized flag when a client size is set on Windows, which desynchronised the
  caption button from the real window state.

**Guard.** `app/src/window-config.test.ts` (5 tests) fails if Windows regains
transparency or a shadow, if the platform file drops a base window field, if the
base config loses transparency for macOS/Linux, if the NSIS bitmaps are not
150×57 / 164×314 24-bit BMP, or if the licence, icons or installer icon are
missing. It runs under `npm test`.

### L2. PDFium is bound once and every FFI call is serialised

**Cause.** `pdfium-render` keeps its library bindings in a process-global
`OnceCell`; `Pdfium::bind_to_library` succeeds once and every later call returns
`PdfiumLibraryBindingsAlreadyInitialized`. `OcrEngine::new` bound the library per
engine, so activating a model pack, benchmarking, or resetting the engine after
a failed run failed with that error and took image OCR down with it. Binding
once is necessary but not sufficient: pdfium keeps process-wide state that is
not thread-safe, and a preview could render while an OCR run paged through the
same document.

**Change.** `core/ocr_engine/src/pdf.rs` owns a process-wide `SHARED_PDFIUM`
(`Mutex<Option<Arc<Pdfium>>>`), a `PDFIUM_FFI` mutex held for one FFI call, and a
`SharedDocument` wrapper whose `Drop` closes the document under the same lock.
`pipeline.rs` opens documents through `pdf::load_document`. The lock is never
held for a recognition pass, so rayon workers still run in parallel.

**Tests.** `pdf_lifecycle` (two tests, 180.8 s with the real library) covers
"a second engine reuses the bound pdfium" and "concurrent previews and OCR share
pdfium safely" (4 threads × 3 iterations). `pdf_native_text` covers the
born-digital fast path. Both pass.

### L3. Smaller defects fixed in the same pass

- **Second launches started a second application.** Nothing stopped a second
  process, so clicking the Start Menu entry while Zeidex sat in the tray added a
  second tray icon, a second clipboard monitor and a second index watcher over
  the same SQLite database. `tauri-plugin-single-instance` is now registered
  first in the builder: the second process hands its arguments to the running
  instance, which raises its window, and exits. On Windows the plugin also calls
  `AllowSetForegroundWindow`, so the running instance is allowed to take the
  foreground, which a background process normally cannot do. On Linux it uses a
  D-Bus name and degrades to "launch normally" when no session bus exists
  instead of failing to start. Verified with `cargo check`, `cargo clippy` and
  `cargo test --lib` (69 passed); the two-process behaviour itself needs a
  desktop session to exercise.
- **Dead configuration keys.** `ui_*` and `query_retention_seconds` were written
  by the config generator but never parsed. They are gone from
  `core/engine/src/config.rs`, and `every_generated_config_key_is_parsed` fails
  if a future key is added without a reader.
- **Dialog focus.** `dialogs.rs` re-activates the main window after every native
  dialog closes, success or cancel, so the next keystroke does not go to the
  desktop.
- **Capabilities.** `src-tauri/capabilities/default.json` now grants only the
  window calls the page actually makes (hide, drag, size, centre, monitor,
  minimise, toggle-maximise, is-maximised, close). Tray and shortcut permissions
  are gone because both run in Rust.
- **Broken header markup.** `OcrStudio` and `Settings` nested `WindowControls`
  inside a `<span>`, which broke the flex layout of the caption buttons. Both
  are `<div>` wrappers now; screenshots confirm the buttons sit at the trailing
  edge.
- **The embedded frontend was not a build input.** `generate_context!` bakes
  `app/dist` into the shell binary, but `tauri-build`'s `rerun-if-changed` list
  names only `tauri.conf.json`, `capabilities/` and the bundled resources (read
  from `target/release/build/zeidex-*/output`). A frontend-only change followed
  by `cargo build` could therefore reuse the previous shell crate and ship the
  old UI - invisible until someone opens the app. `src-tauri/build.rs` now adds
  `cargo:rerun-if-changed=../app/dist`, so the shell crate rebuilds whenever the
  page it embeds changes.
- **Installer identity.** The NSIS block now carries a branded header and
  sidebar generated from `assets/brand/zeidex-mark.svg` by
  `src-tauri/installer/generate.py`, the GPL text as `licenseFile`, English and
  Arabic with a language selector, `currentUser` install, a Start Menu folder
  and an uninstaller icon.
- **`AGENTS.md`.** The repository had no project instructions; the new file
  records the architecture, commands, Windows/PDFium/Arabic contracts and the
  verification standard.

### L4. A folder added in Settings could never be indexed

**Cause.** `file_scan_limit` is one budget shared by the whole walk, and
`discover_local_files_and_folders` sorted the roots before walking them. Sorting
is alphabetical, not intentional: `/root/Downloads` (or `C:\Users\me\Downloads`)
sorts ahead of most names a user picks, and a large Downloads folder could spend
the entire 8,000-entry budget before the folder the user had just added was
reached. The folder then existed in the config, was accepted by Settings, and
never appeared in search - a silent failure with no error anywhere.

**Change.** `RuntimeConfig::file_scan_root_list()` is now the single place that
orders the roots: folders the user added (`file_scan_extra_roots`, written by
Settings) first, then `file_scan_roots` in the order the config lists them, with
order-preserving de-duplication. The walk (`index/files.rs`) and the watcher
(`lib.rs`) both call it, so they cannot drift apart; the previous "same roots,
same order" comment was a promise between two copies of the sort.

**Test.** `a_folder_added_in_settings_is_walked_before_the_limit_is_spent`
builds a standard root that sorts first with five files and an added root that
sorts later with one, sets `file_scan_limit = 5`, and asserts the added file is
indexed. Reverting the fix makes it fail with exactly the reported symptom
(only the five standard files are walked), so the test guards the behaviour
rather than describing it.

### L5. Every screen now has a document outline

An axe-core 4.10.2 run against the real React tree (headless Chrome, the demo
bridge, 1280×880) reported the same three failures on all five screens:
`landmark-one-main` (no `<main>`), `page-has-heading-one` (no `<h1>`) and
`region` (content outside any landmark). A screen reader could not jump to the
screen or list its headings; every visible title was a styled `<span>`.

`app/src/App.tsx` now wraps the active screen in `<main>`, carries one
visually hidden `<h1>` named after the screen (English and Arabic), and moves
focus to that main element when the user opens a sub-screen. The launcher is
excluded from the focus move because its query box takes focus on mount, and
stealing it back would leave the launcher untypeable after a round trip.

Re-run of the same audit: **0 violations on launcher, Document Studio, Batch
Search, OCR Models and Settings** (1 "needs review" item per screen, the
gradient-background contrast check that axe cannot decide automatically).

### L6. What was verified on this host

| Check | Command | Result |
| --- | --- | --- |
| Core tests | `cargo test --workspace --manifest-path core/Cargo.toml --offline` | 590 passed, 0 failed, 2 ignored (scale checks) |
| Core clippy | `cargo clippy --workspace --lib … -- -D clippy::unwrap_used -D clippy::expect_used` | pass |
| Shell tests | `cargo test --lib --manifest-path src-tauri/Cargo.toml --offline` | 69 passed |
| Shell clippy | `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --offline -- -D warnings …` | pass |
| Formatting | `cargo fmt --check`, both manifests | pass |
| Frontend | `cd app && npm test && npm run build` | 86/86, build 408.10 kB JS / 73.44 kB CSS |
| Arabic OCR | `--test arabic_cer` with the real models | CER 0.60 %, WER 3.45 % |
| PDF lifecycle | `--test pdf_lifecycle --test pdf_native_text` | 3 passed |
| Search latency | `-p zeidex-engine --release --test match_latency -- --ignored` | 50k candidates: prefix 24.1 ms p50, suffix 45.8 ms, approximate 26.4 ms |
| UI | headless Chrome against the demo bridge at 760×520, 1280×880, 1140×820, 520×360, LTR/RTL, dark/light | reviewed |
| Accessibility | axe-core 4.10.2 against the real React tree, five screens | 0 violations per screen |

#### The real binary, under Xvfb

`target/release/zeidex` was launched on a virtual X server (`:99`,
1600×1000×24) with an isolated config, database and model root. What the run
established:

- The process starts, creates its window at **760×520** and maps it, with no
  panic and no error in the log beyond the expected
  `libayatana-appindicator is deprecated` warning from the tray backend.
- The database is created with the full schema: `settings`, `candidates`,
  `usage_events`, `index_state`, `clipboard_entries`, `document_text`,
  `document_token`, `url_history`.
- The index worker walks the configured roots and stores **243 applications,
  500 files and 191 folders**.
- The added root `/root/zeidex-fixture` produced all six expected candidates -
  the folder `Projects`, `Zeidex Notes.txt`, `budget-forecast.xlsx`,
  `invoice-2026-01234.pdf`, `تقرير المبيعات.docx` and `مشروع النور.xlsx` -
  including Arabic titles stored intact. On the same setup **before** the
  root-ordering fix the count was zero: `/root/Downloads` sorted ahead of
  `/root/zeidex-fixture` and spent the whole 500-entry budget. The fix is
  therefore proven end to end through the real binary, not only by the unit
  test.

**What this environment cannot show.** WebKitGTK 2.52.6 never paints inside
this Xvfb/PRoot server: the window is mapped and the correct size, but a
window-targeted capture is uniform black in all three paint modes tried
(default, `WEBKIT_DISABLE_COMPOSITING_MODE=1`,
`WEBKIT_DISABLE_DMABUF_RENDERER=1`). No compositor (picom/openbox/mutter) is
installed and the GPU is software-only. Pixel appearance is therefore verified
in headless Chrome against the same built assets, not in the GTK webview; that
distinction is carried into L7 rather than glossed over.

### L7. What remains unavailable for verification

- **Windows runtime.** No Windows machine, no WebView2 and no display server
  here. Transparency, ghosting, flicker, resize, DPI, multi-monitor, tray click,
  caption buttons, dialog z-order, console absence and the installer's
  install/upgrade/uninstall cycle are compiled, unit-tested and config-tested
  but not exercised on Windows. Every field of `tauri.windows.conf.json` and the
  NSIS block was checked against the Tauri config schema; the artwork is checked
  at the pixel level.
- **Installer execution.** NSIS and MSI cannot be built on this host (no Windows
  target, no WiX). The bundle is built by the release workflow on
  `windows-latest`; that run is the first real build, and install, upgrade and
  uninstall are still untested.
- **GPU paths.** CUDA and DirectML features compile only with their toolchains;
  the advisor reports them only when the build and a device both exist.
- **Linux webview rendering.** On this host WebKitGTK maps the window but paints
  nothing under Xvfb (uniform black in every compositing mode), so the GTK
  webview's pixels were not inspected. The same built assets were inspected in
  headless Chrome instead. A Linux desktop with a real compositor is untested.
