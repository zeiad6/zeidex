Native OCR runtime libraries
============================

The Tauri bundler copies every file in this folder to `<resources>/lib/` inside
the installed app. Zeidex's shell looks for the ONNX Runtime and PDFium shared
libraries there at startup (see `src-tauri/src/state.rs`), so a release build
works without the user exporting anything.

Drop the matching library for the target platform here before packaging:

  Linux    libonnxruntime.so      libpdfium.so
  Windows  onnxruntime.dll        pdfium.dll
  macOS    libonnxruntime.dylib   libpdfium.dylib

Sources:
  - ONNX Runtime releases: https://github.com/microsoft/onnxruntime/releases
  - PDFium binaries:       https://github.com/bblanchon/pdfium-binaries/releases

Environment overrides win over these files, which is useful in development:
`ORT_DYLIB_PATH` and `ZEIDEX_PDFIUM_LIBRARY`.

This file must stay: the bundler treats `resources/lib/*` as a glob and fails
when it matches nothing, and the OCR models are required for any build that
actually bundles (`./scripts/fetch_models.sh` first).
