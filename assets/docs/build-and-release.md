# Build and release

Run everything from the repo root. `make help` lists every target.

## Requirements

- Rust 1.99.1 (pinned by `rust-toolchain.toml`; the core declares 1.88 as the
  minimum but edition 2024 is built with the pinned toolchain in practice).
- Node.js and npm for the frontend.
- Platform webview dependencies: `libwebkit2gtk-4.1-dev`, `librsvg2-dev`,
  `patchelf` on Debian/Ubuntu; the Tauri CLI's prerequisites on macOS and
  Windows.
- ONNX Runtime and pdfium shared libraries at runtime. They are not vendored:
  put `libonnxruntime.so` / `onnxruntime.dll` / `libonnxruntime.dylib` (and the
  pdfium equivalent) into `src-tauri/resources/lib/`, or install them
  system-wide. `ORT_DYLIB_PATH` and `ZEIDEX_PDFIUM_LIBRARY` override the lookup.

## Common targets

| Command | Does |
| --- | --- |
| `make core-test` | `cargo test` the core workspace |
| `make core-clippy` | clippy the core, denying `unwrap`/`expect` in production code |
| `make core-fmt` | `cargo fmt --check` the core |
| `make ocr-test` | run the real-model OCR tests against `third_party/models` |
| `make frontend-install` | `npm install` in `app/` |
| `make frontend-build` | typecheck + build the React app |
| `make tauri-dev` | run the desktop app in development mode |
| `make tauri-build` | build the release bundle |
| `make models` | download the ONNX models into `third_party/models` |

`make ocr-test` needs an ONNX Runtime shared library; it exports
`ORT_DYLIB_PATH` from `ZEIDEX_ORT_DYLIB` if you set it, otherwise it uses the
`ORT_DYLIB_PATH` already in the environment. The fixture page is committed
(`core/ocr_engine/tests/fixtures/table.png`), so no extra assets are needed.

## Checks

```bash
make core-test
make core-clippy
make ocr-test        # after `make models`
make frontend-build
cd src-tauri && cargo check
```

CI (`.github/workflows/ci.yml`) runs on pull requests and manual dispatch:

- **core** — fmt, clippy (`unwrap`/`expect` denied) and tests on Ubuntu and
  Windows.
- **ocr-models** — downloads the models (cached on `SHA256SUMS`), fetches the
  ONNX Runtime release, and runs the real-model detection, recognition, layout
  and table tests. This is the job that would catch a decoder regression the
  unit tests cannot see.
- **desktop (Ubuntu, Windows)** — installs the frontend, builds it, runs clippy
  with `-D warnings`, and `cargo check`s the shell.

## Bundling

`make tauri-build` (from the repository root, where `src-tauri/` is visible)
produces the bundles listed in `src-tauri/tauri.conf.json`: `.deb`, `.AppImage`,
`.msi`, `.nsis` and `.dmg`. The Tauri CLI must see `src-tauri/`; running it from
`app/` fails because the shell lives one level up. The bundle icons live in
`src-tauri/icons/`.

`bundle.resources` in `src-tauri/tauri.conf.json` copies:

| From | To | Why |
| --- | --- | --- |
| `../third_party/models/*` | `<resources>/models/` | the app is offline; the models must ship |
| `resources/lib/*` | `<resources>/lib/` | ONNX Runtime and pdfium shared libraries |

The models glob is required, so **run `make models` before `tauri build`**; a
missing model folder fails the bundle instead of shipping an app that cannot
OCR. `resources/lib/` always contains a `README.txt`, which keeps the glob
valid while the folder is empty in a source checkout.

At startup the shell points `ORT_DYLIB_PATH` at a bundled runtime (if any) and
resolves the model directory as: `ZEIDEX_MODELS_DIR` → bundled
`<resources>/models` → a development checkout (`third_party/models`) → the
per-user data dir.

Release automation lives in `.github/workflows/release.yml`: on a `v*` tag (or a
manual dispatch) it builds `.deb` + `.AppImage` on Ubuntu and `.msi` + `.nsis`
on Windows, stages ONNX Runtime and pdfium next to the executable, and uploads
the bundles to the GitHub release. MSI is built on a Windows runner because WiX
cannot run on Linux. The old macOS DMG/appcast/Homebrew pipeline is still in
`legacy/` and not wired up for the Tauri shell.
