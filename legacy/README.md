# Legacy macOS shell

These directories are the pre-Tauri macOS implementation, kept for reference
only. They are **not** part of any Cargo workspace and are not built or tested.

- `macos/` — the SwiftUI app (`apps/macos`), including the Apple Vision /
  PDFKit OCR worker (`ContentTextExtractor.swift`) and the Xcode project.
- `bridge-ffi/` — the C-ABI crate (`bridge/ffi`) that exposed the Rust core to
  Swift. Superseded by Tauri commands in `src-tauri/`.

The cross-platform shell lives in `src-tauri/` (Rust) and `app/` (React/TS).
The Rust core in `core/` no longer links Objective-C: bundle metadata is read
with the `plist` crate, and translation is behind the non-default `translate`
feature of `zeidex-answers`.
