---
name: zeidex-release
description: Cut a Zeidex release without overclaiming — the exact gates, the evidence rules, the Windows limits of this environment, and the files a release touches. Use before committing, tagging or writing release notes for zeidex-search.
---

# Zeidex release

## The rule that matters

A feature is done when a test, a build, a measured run or a screenshot shows it.
A name, a comment, a README paragraph or a previous report is context, never
proof. If something could not be exercised here, write it in the report's
"not verified" table with the reason.

## Environment limits (Linux arm64, Termux/PRoot)

- No Windows machine, no WebView2, no WiX/NSIS: the MSI/NSIS installers **cannot
  be built or run here**. Say so; never describe a Windows build as done.
- WebKitGTK 2.52.6 paints nothing under Xvfb (uniform black). UI pixels are
  verified in headless Chrome against the demo bridge, not in the real webview.
- PostgreSQL must not run here; this project does not need it.
- Disk is tight; keep builds targeted (`-p`, `--lib`).

## Gates (run all of them, in this order)

```bash
cargo test --workspace --manifest-path core/Cargo.toml --offline
cargo clippy --workspace --lib --manifest-path core/Cargo.toml --offline \
  -- -D clippy::unwrap_used -D clippy::expect_used
cargo fmt --all --manifest-path core/Cargo.toml -- --check

cargo test --lib --manifest-path src-tauri/Cargo.toml --offline
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --offline \
  -- -D warnings -D clippy::unwrap_used -D clippy::expect_used
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check

cd app && npm test && npx tsc --noEmit && npm run build
```

Real-model gates (need the staged models and libraries):

```bash
export ZEIDEX_TEST_MODELS_DIR="$PWD/third_party/models"
export ORT_DYLIB_PATH="$PWD/src-tauri/resources/lib/libonnxruntime.so"
export ZEIDEX_TEST_PDFIUM_LIB="$PWD/src-tauri/resources/lib/libpdfium.so"
cargo test --manifest-path core/Cargo.toml -p zeidex-ocr --test arabic_cer --offline
```

The Arabic gate is `MAX_CER 0.08`, `MAX_WER 0.15`. Any preprocessing or model
change must re-run it; quote the aggregate line, not a memory of it.

## Version bump (five places, all of them)

`core/Cargo.toml` (workspace), `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`,
`app/package.json`, and the two lockfiles. Then `grep -rn "<old version>"` over
`--include=Cargo.toml --include=package.json --include=tauri.conf.json`.

## Before committing

```bash
git status --porcelain          # nothing unexpected, no target/ or dist/
git diff --stat                 # read it
git diff --check                # whitespace
```

- No secrets: scan the changed files for keys, tokens and `.env` content.
- No build output, no models, no `.test-build` — `.gitignore` already covers
  them; check that nothing slipped in with `git status --ignored` if unsure.
- `git push` only to `origin` (this project's own remote). Never create repos,
  gists or releases, and never change visibility without an explicit instruction.

## Release documents

- `ENGINEERING_REPORT_<version>.md`: evidence, numbers, defect table, and a
  section that lists what was **not** verified and why.
- `RELEASE_NOTES.md`: user-facing. Do not print artifact sizes or hashes that
  were not measured; the workflow attaches them to the release it builds.
- Update the README feature section and add screenshots under
  `assets/screenshots/` when a user-visible feature ships.

## After pushing

Report the real result: the commit hashes, the `git push` output, and the
`origin/main` head. If the push failed, give the reason; do not describe it as
done.
