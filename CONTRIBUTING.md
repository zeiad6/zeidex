# Contributing to Zeidex

Thanks for contributing! Bug reports, ideas and pull requests are all welcome.

## Issues

Search existing issues first. A good bug report has:

- what you expected and what happened
- numbered steps to reproduce, and how often it happens (`always`, `sometimes`, `once`)
- OS and Zeidex version (or commit)
- logs, screenshots or the crash text if you have them

For a feature request, describe the problem first, then the behavior you'd like.

## Development setup

**Requirements:** Rust (pinned by `rust-toolchain.toml`; 1.99.1), Node.js/npm,
GNU Make, and the [Tauri CLI prerequisites](https://tauri.app/start/prerequisites/)
for your platform (a webview, `patchelf`, …).

```text
core/                 Rust workspace: engine, indexing, matching, ranking, storage,
                      tools, answers, ocr_engine, exporter
src-tauri/            Tauri v2 shell (Rust): commands, tray, hotkey, state
app/                  React + TypeScript + TailwindCSS frontend (Vite)
assets/               icon source, screenshots, docs
third_party/models/   downloaded ONNX models (make models), git-ignored
legacy/               pre-Tauri macOS shell and C-ABI bridge, reference only
tools/perf/           benchmarks (separate crate, never bundled)
```

Build and run (from the repo root):

```bash
make frontend-install   # npm install in app/
make tauri-dev          # run the desktop app in development mode
make help               # every target
```

Run the checks before opening a PR:

```bash
make core-test          # cargo test the core workspace
make core-clippy        # deny unwrap/expect in production code
make frontend-build     # typecheck + build the React app
cd src-tauri && cargo check
```

The OCR smoke tests need real models and an ONNX Runtime shared library; they
skip unless `ZEIDEX_TEST_MODELS_DIR`, `ZEIDEX_TEST_IMAGE` and `ORT_DYLIB_PATH` are
set. `make models` downloads the models.

## App icon

The source of truth is `assets/brand/zeidex-mark.svg` — transparent, geometric,
and validated for repository-safe rendering. To change it:

```bash
rsvg-convert -w 1024 -h 1024 -o assets/icon-source.png assets/brand/zeidex-mark.svg
rsvg-convert -w 512  -h 512  -o assets/icon.png        assets/brand/zeidex-mark.svg
cd app && npx tauri icon ../assets/brand/zeidex-mark.png --output ../src-tauri/icons
```

Commit the regenerated files under `src-tauri/icons/` with the SVG change.
`assets/generate-icons.py` is kept for the legacy macOS `AppIcon.appiconset` only.

## Pull requests

- branch from `main` and target `main`; it is the only long-lived branch and must stay releasable
- keep the change focused; no unrelated formatting
- rebase onto the latest `main` (or merge `origin/main` whole) before asking for review
- update the README when user-visible behavior changes
- explain *why* in the PR description, and add before/after screenshots for UI changes

Commit prefixes: `feat:`, `fix:`, `refactor:`, `docs:`, `test:`, `chore:`.

## Building a bundle locally

```bash
make tauri-build        # per-platform bundle from src-tauri/tauri.conf.json
```

CI (`.github/workflows/ci.yml`) runs the core checks and builds the desktop shell
on Ubuntu and Windows. Release automation (tag → bundle → appcast → Homebrew)
has not been ported from the macOS shell yet; the old scripts live in `legacy/`.
