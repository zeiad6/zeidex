# Batch Search

Batch Search answers a different question from the launcher. The launcher takes
one query and ranks the whole index against it; Batch Search takes a list of
names — a spreadsheet column, an email, a folder listing — and asks, for each
one, *is this file here, and if not, what is the closest thing that is?*

```text
paste / import .txt ─▶ review parsed list ─▶ threshold ─▶ search ─▶ results ─▶ .xlsx
```

It is a separate surface, opened from **Batch Search** in the launcher footer,
and it is built on the same index the launcher searches: the files and folders
the watcher has recorded, not a second crawl of the disk.

| Layer | Where |
| --- | --- |
| Parsing, matching, report | `core/batch` (pure Rust, no Tauri) |
| Commands | `src-tauri/src/commands/batch.rs` |
| Screen | `app/src/components/BatchSearch.tsx`, helpers in `app/src/batch-view.ts` |

## Getting names in

Three doors, one list:

- **Paste** into the box — one name per line, a column copied from Excel, a
  tab-separated row, names lifted from an email.
- **Import TXT** — a `.txt` file, capped at 8 MiB. A larger file is refused by
  name rather than read into memory.
- **Type** — the parsed list is editable: remove an entry, clear the list, paste
  again on top of what is there.

The box is a plain textarea, so the system clipboard works with the normal
shortcuts. A **Paste** button is there for mouse-only use: it reads the
clipboard through the webview and, where that permission is refused, focuses
the box and tells the user to press <kbd>Ctrl</kbd>+<kbd>V</kbd> instead — it
never fails silently.

### What the parser does, and does not do

A filename is data. The parser separates entries and trims the noise around
them; it never "corrects" a name.

| Input | Rule | Why |
| --- | --- | --- |
| Separators | newline and tab only | a single spreadsheet column arrives newline-separated, a row arrives tab-separated; `report, final.pdf` is one file, not two |
| Commas, semicolons | kept as characters | legal in filenames |
| UTF-8 BOM | dropped | Notepad adds one |
| CRLF | the `\r` is trimmed | Windows files |
| Wrapping quotes | one matching pair removed | what CSV writers and mail clients add |
| Leading/trailing space | trimmed | invisible, never part of a name on disk |
| Duplicates | folded, counted | compared after normalization, so `Invoice.PDF` and `invoice.pdf` are one request |
| Encoding | BOM decides UTF-16LE/BE, otherwise UTF-8 | Excel's "Unicode text" export is UTF-16LE; invalid UTF-8 is replaced, not fatal, so one damaged name does not cost the other 999 |
| Size | 2000 entries | the search is linear in entries × index; a pasted novel should fail loudly, not freeze the window |

The review step shows four numbers before anything runs: valid entries,
duplicates folded, empty lines dropped, and entries refused over the cap. The
overflow count matters — silently searching the first 2000 of 3000 names would
look like a complete answer.

## What a score means

Every comparison produces an integer percentage, and the slider's threshold is
applied to that same integer. The number shown next to a result is the number
that decided it; there is no second, hidden score.

Both sides are normalized first (see below) — case, diacritics, Arabic letter
variants, Arabic-Indic digits. The request is then compared with the candidate's
full name **and** its stem (the name without its last extension), and the best
measure wins:

| Measure | Score | Definition |
| --- | --- | --- |
| Exact | 100 | normalized name equals the request |
| Exact name | 100 | normalized stem equals the request (`invoice` → `invoice.pdf`) |
| Contains | 60–99 | one text is a contiguous part of the other, shorter side ≥ 3 characters: `60 + 40 × short/long`, plus 20 when the shorter side is a prefix, capped at 99 |
| All words | 85 | every request token appears among the candidate's tokens |
| Fuzzy | 0–99 | `1 − distance / max_len`, Levenshtein distance |

Three properties fall out of that table, and they are the ones users actually
ask about:

1. **An equality hit ends the search for that name.** If the file with exactly
   that name is in the index, fuzzy neighbours are not appended. The exact
   answer is the answer; burying it under near-misses is how a search loses
   trust.
2. **"Contains" is deliberately generous at the prefix.** A pasted name is
   usually the start of the real one — `invoice` for `invoice-2024-final.pdf` —
   so a prefix scores `60 + 40 × 7/22 + 20 ≈ 93`, while the same text found in
   the middle scores 73.
3. **Fuzzy is bounded twice.** By length: an edit distance of `d` changes the
   length by at most `d`, so at 70% a 10-character request cannot match a
   20-character name. And by an absolute budget of **4 edits**: two or three
   typos in a name are plausible, nine are a different name. Without that cap a
   percentage alone would let a long name drift arbitrarily far, because
   `distance / max_len` shrinks as the name grows.

The fuzzy distance is computed on a diagonal band and abandoned as soon as a
whole row leaves the band, so a hopeless candidate costs a few cells per
character instead of a full matrix. The banded routine is tested against a
full-matrix reference on 600 random pairs.

### What 100% means

At 100% the only match that can pass is equality after normalization — exact or
stem. That is a definition, not a rounding accident: containment tops out at 99
and fuzzy at 99 by construction. The slider never claims that a fuzzy match is
the same thing as an exact one.

At the other end, 50% is the floor because below it almost every file in the
index is a "possible match", which is not a result. A threshold outside
50–100 is clamped, not silently accepted.

| Slider | Label in the UI | What it admits |
| --- | --- | --- |
| 50 | Broad | containment of a short name inside a much longer one, loose fuzzy |
| 55–64 | Flexible | containment, up to 3–4 edits on long names |
| 65–79 | Balanced (default 70) | containment, tokens, typos that keep the name recognisable |
| 80–89 | High confidence | strong containment, all-words, one or two edits |
| 90–99 | Very strict | near-equal names, prefix containment of a long request |
| 100 | Exact only | equality after normalization |

### Arabic, and everything else normalization touches

`normalize_for_search` (in `core/matching`) folds both sides of every
comparison. It never rewrites what is displayed or stored: the original name and
path are carried through untouched, and normalization only produces comparison
keys.

- NFKD, combining marks stripped, lowercased — `Café` = `cafe`, `RÉSUMÉ` = `resume`.
- `đ` → `d` (Vietnamese treats it as its own letter, so Unicode does not fold it).
- Arabic: tatweel removed; `ة` → `ه`; `ى` → `ي`; `ک` → `ك`; `ی` → `ي`; hamza
  forms fold to the bare alef via NFKD. So `مدرسه` finds `مدرسة` and `مصطفي`
  finds `مصطفى`, which is exactly the substitution Arabic writers make.
- Arabic-Indic (`١٢٣`) and Persian (`۱۲۳`) digits fold to ASCII, so a number
  typed on an Arabic keyboard matches one typed on a Latin keyboard.

Han, kana and other caseless scripts pass through unchanged.

## Results

Each requested name gets a status from its best match:

| Status | Rule |
| --- | --- |
| Exact | best score is 100 |
| High confidence | best score ≥ 85 |
| Possible | best score is below 85 but at or above the threshold |
| Not found | nothing reached the threshold |
| Search error | the request had no searchable characters after normalization (only tatweel or diacritics), so no honest score exists; counted separately and left out of the missing-files sheet |

A row expands to show every candidate, best first. Up to **8** candidates are
returned per name, but `total_matches` reports how many actually passed — a row
that says "12 matches" is not hiding the other four silently, it is telling you
the list was truncated for display.

The summary strip counts requested, found, not found, exact, approximate,
multiple-match and the success rate, plus the wall-clock duration. Filters cover
found / not found / exact / fuzzy / multiple / errors and minimum percentage;
sorting covers percentage, name, type, size and path. Results page in chunks of
150 rows so a 2000-name paste does not build 2000 DOM trees at once.

Every discovered file can be opened, revealed in its folder, copied by path or
by name, and a row can be hidden from the table without re-running the search
(a **Restore hidden** chip brings it back). Rows and their candidates are
multi-selected for bulk copy of paths or names, bulk open, or bulk hide. Files
are never executed by the batch screen: opening is a shell open of the file's
own type, and everything else is a copy.

File sizes are filled with a `stat` only for the matches that are returned (at
most 8 per name), not for the whole index — the size column costs what it shows,
not what it scans. Folders have no size and are not `stat`ed.

## The Excel report

**Export Excel** writes four sheets through `rust_xlsxwriter`, chosen because it
is a pure-Rust writer with no Office dependency and a permissive licence.

| Sheet | Contents |
| --- | --- |
| Summary | Zeidex mark, title, generation time, ten KPIs (requested, found, not found, exact, approximate, multiple, errors, success rate, threshold, duration), search scope in files, a column chart of the four outcomes, and five notes stating how matching works |
| Results | One flat table — requested name, status, matched name, match %, match type, extension, size, path, modified, search time, notes — with autofilter, frozen header and first column, repeated header rows when printed, landscape at 55% |
| Missing | Every name that reached nothing, with its extension and a sentence saying it was not in the index at that threshold |
| Multiple matches | One row per candidate for names with more than one match, with each candidate's score, type, path and date |

Numbers are stored as numbers with a format: a percentage is a real percentage
cell, a date is a real datetime, a duration is milliseconds with a `ms` suffix
format, sizes are human-readable strings because a file size is not a quantity
anyone sums. The chart is a single series over four contiguous rows — two
one-point series render as nothing in some viewers.

The report is generated off the UI thread (`#[tauri::command(async)]`), so a
large export does not freeze the window, and the save path comes from the native
save dialog rather than a guessed location.

## Performance

Measured on this machine (8 cores, release build) with the ignored scale test:

```bash
cargo test --release -p zeidex-batch --test report -- --ignored --nocapture
```

The workload is 1000 requested names against 20 000 indexed files at a 70%
threshold. Every request carries a typo (`report-fnal.pdf`), so none of them is
an exact hit and all 1000 scan the whole index — the worst case, and the one a
real "find these invoices" batch actually hits. Successive changes to the
matcher brought that run down from **136.6 s** to **13.4–16.9 s** (the spread is
run-to-run variance on a machine that is doing other work; 58.5 s and 44.9 s
were measured in between). The changes, in the order they matter:

1. Entries are independent, so the batch runs one entry per core with rayon.
   A batch is dominated by the names that miss — each scans the whole index —
   so this is the difference between a progress bar and a frozen window.
2. The request is normalized and split into characters and tokens once, not per
   candidate; a `Vec<char>` per comparison was the single largest cost.
3. One scratch buffer is reused for the whole pass instead of allocating per
   candidate.
4. Containment and token checks run first and short-circuit the fuzzy pass when
   they already reach 85 — the edit distance could only confirm a score that is
   already "high confidence".

The exact lookup itself is a hash map built once per batch, so a name that is
present costs a lookup rather than a scan.

A second, smaller scale check separates the three shapes of a batch against a
100 000-file index (200 names each, release build, this machine):

```bash
cargo test --release -p zeidex-batch --test perf_batch -- --ignored --nocapture
```

| Batch | Per name | 200 names |
| --- | --- | --- |
| All present, 100% | 0.63 ms | 0.13 s |
| All missing, 70% | 5.3 ms | 1.07 s |
| One typo each, 70% | 22.5 ms | 4.5 s |
| All missing, 50% (broad) | 21.6 ms | 4.3 s |

A typo costs more than a miss because the length filter admits every
`report_*.pdf` candidate and the banded edit distance then runs on each; the
search still runs off the UI thread, one name per core.

## Limits, stated plainly

- **It searches names, not contents.** Batch Search answers "is this file
  here"; it does not open documents. Content search is the launcher's job.
- **The index is the scope.** Files under a root the watcher never saw, or on a
  drive that is not mounted, are not found — and the report says how many files
  were searched, so "not found" can be read in context.
- **Fuzzy stops at 4 edits.** A badly mangled long name is a different name.
  Raise nothing but the threshold and it still will not match; that is
  intentional.
- **Arabic recognizer quality does not enter here.** Matching is on filenames.
  Whether the *contents* of an Arabic scan were read correctly is the OCR
  pipeline's business, not this screen's.
- **2000 names per batch.** Above that the list is truncated with a reported
  overflow rather than accepted silently.
