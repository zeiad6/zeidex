# Zeidex 0.5.0 — engineering report

Evidence only. Every number below came from a command run in this tree on
2026-11-24, or from a screenshot of the real React tree. Anything that could not
be exercised here is in section J instead of being described as done.

Environment: Linux arm64 (Termux/PRoot), no Windows machine, no Windows
toolchain. Rust 1.99.1, Node 22.22.1, six cores.

0.4.0 is unchanged: its tag, notes and packages stay as they were. This report
covers the new 0.5.0 release only.

---

## A. What this release is

The 0.4.0 release added data intelligence to the document studio. 0.5.0 makes
the shell match it: one theme system, real settings, exclusions enforced in the
walker, an optional Windows autostart, a first-run guide, About & Diagnostics,
and an index that reports its own health.

## B. What changed

| Area | Change | Where |
| --- | --- | --- |
| Theme | One provider for light, dark and system; sun/moon toggle on every screen; stored choice applied before first paint; 260 ms cross-fade skipped under reduced motion | `app/src/theme-context.tsx`, `ThemeToggle.tsx`, `styles.css` |
| Light theme | Warm off-white page (`#f3f4f6`), `#fafaf9` surface, white only for cards; light-theme accent label darkened to `#155e75` for 5.8:1 on its tint | `app/src/styles.css` |
| Dark theme | Four elevation steps (`sunken`, `surface`, `surface-2`, `float`) instead of one flat black | `app/src/styles.css` |
| Autostart | `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` entry, read back after every write; `--autostart` marks a login launch; optional Start Minimized; tray menu with Show Window, Open Search, OCR Studio, Settings, Exit | `src-tauri/src/autostart.rs`, `src-tauri/src/lib.rs` (tray), `commands/settings.rs` |
| First run | Welcome guide opens on the first launch after install, names the registered shortcut, traps focus, writes `onboarding_seen`, reopenable from Settings > General | `app/src/components/Welcome.tsx`, `src-tauri/src/commands/onboarding.rs` |
| Exclusions | Folder, file, extension and safe glob rules; persisted one per config line; compiled once per walk; excluded directories pruned before any stat, metadata, OCR or database row; adding purges affected rows; removing a folder/file rule re-walks that path, an extension/pattern rule schedules a full pass | `core/engine/src/exclusions.rs`, `index/files.rs`, `commands/exclusions.rs` |
| Presets | Development, Temporary and System Noise groups, opt-in only, previewed with an affected-path count; default skip list no longer hides project folders | `src-tauri/src/commands/exclusions.rs`, `app/src/components/settings/ExclusionsSection.tsx` |
| Results | Name first, then format and match badges, then one line with modified date, size and drive; hover, selection, focus and context menu share the same surface | `app/src/components/ResultList.tsx` |
| Icons | One 24 px stroke set, `currentColor`, per-kind file glyphs instead of coloured emoji | `app/src/components/Icons.tsx`, `FileGlyph.tsx` |
| Depth | Four shadow tokens per theme; cards, menus, dialogs and the search panel use them | `app/src/styles.css` |
| Settings | Eleven sections with an English/Arabic filter, grouped cards, inline validation and confirmations | `app/src/components/Settings.tsx` |
| Privacy | Settings > Privacy states what is indexed, which drives, which rules, where the database lives, and how to delete the index, stop indexing and clear search history | `app/src/components/Settings.tsx` |
| About & Diagnostics | Build, publisher, maintainer, paths, model libraries, index size, and a Check for updates button against the public GitHub releases API | `src-tauri/src/commands/update.rs`, `diagnostics.rs` |
| Update notice | A newer tag emits `zeidex://update` once; the launcher shows a dismissible toast; no file name, path or index content is sent | `src-tauri/src/lib.rs`, `app/src/App.tsx` |
| Index health | Health derived from the worker's own status (error, partial, updating, needs refresh, healthy) plus the exclusion-pruned count from the last walk | `app/src/index-health.ts`, `src-tauri/src/index_service.rs` |
| Build | `make deb` repairs only the current version's package instead of every `.deb` in the bundle directory | `Makefile` |

## C. Version bump

`0.5.0` in: `core/Cargo.toml` (workspace), `core/Cargo.lock`,
`src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, `src-tauri/tauri.conf.json`,
`app/package.json`, `app/package-lock.json`, and the demo fixtures. The 0.4.0
tag and its published files were not modified.

## D. Rust gates

| Gate | Result |
| --- | --- |
| `cargo test --workspace` (core) | **632 passed**, 0 failed |
| `cargo test --lib` (shell) | **92 passed**, 0 failed |
| Core clippy (`-D unwrap_used -D expect_used`) | 0 errors, **0 warnings** |
| Shell clippy (`--all-targets -D warnings -D unwrap_used -D expect_used`) | 0 errors |
| `cargo fmt --check` (core and shell) | clean |

New tests in this release:

- `a_folder_rule_prunes_before_descending_and_is_counted` — the walker never
  enters the excluded folder, indexes its sibling, and counts the pruned folder
  once, not once per descendant.
- Six `index-health` cases: missing status, error over flags, paused, updating,
  empty index, healthy.
- The status parser keeps a valid exclusion counter and zeroes a malformed one.
- Five `menu-position` cases keep the result context menu inside the window,
  including a point outside it and a window smaller than the menu.

## E. Frontend gates

| Gate | Result |
| --- | --- |
| `npx tsc --noEmit` | clean |
| `npm test` | **114 passed**, 0 failed |
| `npm run build` | clean |

## F. Accessibility

axe-core 4.10.2 (WCAG 2.0/2.1/2.2 A+AA) against the whole document:

```text
launcher light/dark, welcome light/dark,
General light/dark, Indexing light/dark,
Exclusions light/dark, Startup, Privacy,
About light/dark, About RTL, Exclusions RTL
→ 0 violations on all 16 screens
```

## G. Visual review

Screenshots of the real React tree through CDP (Chromium 1255, arm64), not
mock-ups: launcher and results in both themes, welcome, all Settings sections,
and RTL Settings. The Indexing screen was captured in light, dark and RTL with
zero page or driver errors. The result context menu was exercised over CDP:
right-click and Shift+F10 open it, Escape closes it, the three items are present,
Copy path writes the exact path to the clipboard, and axe reports 0 violations
with the menu open. Theme persistence, the pre-paint guard, the system
follow, the section filter and the first-run modal were exercised by scripted
checks (`theme-check.js`, `settings-search-check.js`, `welcome-check.js`), all
passing.

## H. What was actually run

```text
core:    cargo test --workspace --manifest-path core/Cargo.toml --offline
         → 632 passed, 0 failed
         cargo clippy --workspace --lib ... -D clippy::unwrap_used -D clippy::expect_used
         → 0 errors, 0 warnings
         cargo fmt --all -- --check → clean

shell:   cargo test --lib --manifest-path src-tauri/Cargo.toml --offline
         → 92 passed, 0 failed
         cargo clippy --all-targets ... -D warnings -D clippy::unwrap_used -D clippy::expect_used
         → 0 errors
         cargo fmt --all -- --check → clean

front:   npm test → 114 passed, 0 failed
         npx tsc --noEmit → clean
         npm run build → clean

package: make deb
         → src-tauri/target/release/bundle/deb/Zeidex Search_0.5.0_arm64.deb
```

## I. Package evidence

`I1.` The arm64 Debian package built by `make deb` (Tauri bundle, then
`scripts/fix-deb-depends.sh` repacks the control file):

| Field | Value |
| --- | --- |
| Package | `zeidex-search` |
| Version | `0.5.0` |
| Architecture | `arm64` |
| File | `Zeidex Search_0.5.0_arm64.deb` |
| Size | 50,176,778 bytes (47.85 MiB) |
| SHA-256 | `a5710fe024f60993253d60781275d889a93287b5a6905fe10571609cff096df6` |
| Extracted binary | differs from `target/release/zeidex` by exactly the 3 bytes Tauri patches per bundle type |

`I2.` The package was extracted and inspected with `dpkg-deb -x`; `dpkg-deb -f`
reported the fields above, `cmp -l` against `target/release/zeidex` reported
exactly 3 differing bytes (the bundle-type patch), and the frontend bundle inside
`app/dist` contains the result-menu strings. It was **not** installed on this
host. The same file was copied to `/storage/emulated/0/Download/` and `cmp`
reported no difference.

`I3.` `make deb` used to repair every `.deb` in the bundle directory, which
rewrote the archived 0.4.0 package and changed its hash. The target now reads the
version from `src-tauri/tauri.conf.json` and repairs only that version's package;
the 0.4.0 file was restored from the verified copy
(`dbc11087d9187c6ceecaa1e95047ac0eee58e1b2d4fb106eec0116ac20e6049a`).

## J. What was not exercised

- **Windows runtime.** No Windows machine or toolchain here: the window,
  autostart registry entry, tray, Start Minimized, MSI and NSIS installers were
  not run. The code paths exist and are compiled for Linux only in this tree.
- **GPU acceleration.** Not exercised; the OCR tests that need models and
  ONNX Runtime were not run in this pass because the recognizer did not change.
- **Arabic OCR corpus.** Not re-run in this pass. The 0.4.0 result (CER 0.60 %,
  WER 3.45 %) stands; the recognizer, models and preprocessing are untouched.
- **Windows/amd64 packages.** Not built here. The `Release` workflow builds them
  on GitHub runners from the `v0.5.0` tag; this host can only produce arm64.
- **Installation of the deb.** Inspected, not installed.

## K. GitHub release

Release commit `d591796`; the source is pushed to `origin/main` and tagged
`v0.5.0`. The `Release` workflow builds the Windows and Linux x64 bundles from
that tag and attaches them to a draft release; the arm64 deb built here is
attached to the same release. Nothing in the 0.4.0 release is replaced.
