## Summary

-

## Why

<!-- The problem this solves, not the diff in prose -->

## Changes

-

## Testing

- `core` (Rust workspace)
  - [ ] `make core-test`
  - [ ] `make core-fmt`
  - [ ] `make core-clippy`
- `src-tauri` (desktop shell), if touched
  - [ ] `cargo clippy --all-targets -- -D warnings`
  - [ ] `cargo check`
- `app` (React frontend), if touched
  - [ ] `make frontend-build`
  - [ ] Checked in both LTR and RTL
- OCR, if touched
  - [ ] `make ocr-test` with real models

## Screenshots / Recordings (if UI changed)

### Before

### After

## Risks / Notes

-

## Checklist

- [ ] PR title is clear and scoped
- [ ] Docs updated for user-visible changes
- [ ] No secrets, private paths or confidential documents included
- [ ] Licence headers/notices preserved
