<p align="center">
  <img src="https://raw.githubusercontent.com/zeiad6/zeidex/main/assets/brand/zeidex-hero-static.png" alt="Zeidex Studio" width="780">
</p>

<h1 align="center">Zeidex Studio 0.1.0</h1>

<p align="center">
  <b>Your files. Your machine. Your data.</b><br>
  A keyboard-first launcher and an offline document AI for Windows and Linux —<br>
  with macOS built from source.
</p>

<p align="center">
  <img alt="No account" src="https://img.shields.io/badge/account-not%20required-2f855a?style=flat-square">
  <img alt="No telemetry" src="https://img.shields.io/badge/telemetry-none-2f855a?style=flat-square">
  <img alt="Offline OCR" src="https://img.shields.io/badge/OCR-100%25%20local-2f855a?style=flat-square">
  <img alt="OCR tiers" src="https://img.shields.io/badge/models-3%20tiers-2d3748?style=flat-square">
  <img alt="Licence" src="https://img.shields.io/badge/licence-GPL--3.0--or--later-2d3748?style=flat-square">
</p>

---

## The first public release

Zeidex Studio started as a personal launcher and grew into a document studio. This
release packages both halves as a single, self-contained desktop app: **no account,
no telemetry, and no network calls at runtime** — the OCR models, dictionaries,
ONNX Runtime and the PDF rasteriser all ship inside the installer. Three OCR tiers,
a hardware advisor and batch file search are part of the same build.

### Highlights

| | |
| --- | --- |
| **Launch anything** | Fuzzy search across installed applications, files and folders, with keyboard-first navigation and a compact, always-ready window. The index updates itself while the app runs — new, renamed and deleted files appear without a manual rebuild, and indexing can be paused, resumed or cancelled from Settings. |
| **Clipboard history** | Searchable clipboard buffer with pinning, so the snippet you copied an hour ago is still one keystroke away. |
| **Inline answers** | Calculator, unit and currency-style expressions, and translation right in the search field — no browser tab required. |
| **Document Studio** | Drop a scan or a PDF, detect the layout, read the text, edit the result, then export to Word or Excel. A long PDF is read as a page window, and a run can be cancelled between pages. |
| **Three OCR tiers** | The bundled PP-OCRv4 Mobile model is ready on first launch; a balanced PP-OCRv4 Server pack and a high-accuracy PP-OCRv5 Server pack are one click away, downloaded on demand, checksum-verified and activated atomically. |
| **Hardware advisor** | Measures CPU, cores, RAM, GPU (where the platform exposes it without a driver query), free disk space and OS, classifies the machine as Basic, Standard, Advanced or High Performance against published criteria, and recommends a tier with the reason and the trade-offs stated. |
| **Batch file search** | Paste a list or import a `.txt`, review the parsed entries, pick a match threshold anywhere from 50% to 100%, then work through exact, high-confidence, possible, multiple and missing results — open, reveal, or copy paths. |
| **Excel report** | A four-sheet workbook: an executive summary with KPI block and chart, the full result table with filters and frozen panes, the missing files, and every multiple-match candidate with its score. |
| **Search inside documents** | Recognised text is indexed locally, so a launcher search finds a word inside a scanned PDF, not just its filename. `.docx`, `.xlsx`, `.odt` and plain text are read directly without an OCR pass. |
| **Reads Office and text files** | A legacy Windows-1256 or Windows-1252 text file is decoded instead of refused, and spreadsheet dates, percentages and times are rendered the way Excel shows them. |
| **Real Office files** | `.docx` with RTL `bidi` runs and native tables (including merged cells); `.xlsx` with a right-to-left sheet, numeric cells and merged ranges. |
| **Private by construction** | ONNX inference runs on your CPU, locally. Nothing is uploaded, and there is no analytics endpoint to disable. |

### OCR model tiers

| Tier | Model | Download | Best for |
| --- | --- | --- | --- |
| **Compact** | PP-OCRv4 Mobile | bundled | Low-end and CPU-only machines, quick extraction, first run with no network. |
| **Balanced** | PP-OCRv4 Server detection | ≈ 129 MB | Most users: sharper line detection on scans and screenshots at a moderate cost. |
| **Precision** | PP-OCRv5 Server | ≈ 181 MB | Difficult scans, dense Arabic pages and noisy images on a machine with headroom. |

Packs are written to the app data directory, never over the bundled model, and a
pack is only marked installed once every file matches its SHA-256. Activation is a
switch, so a failed download cannot leave OCR unusable — the previous tier keeps
working until the new one verifies.

### What is inside the installer

- The Tauri desktop shell and the React interface (RTL and Arabic UI included).
- PP-OCRv4 detection and English/Arabic recognition models, plus their dictionaries.
- ONNX Runtime and PDFium, staged next to the binary and loaded locally.
- Roughly **35 MB of models** and **32 MB of runtime libraries**, so the first run
  works on a machine that has never seen the internet. The two larger tiers are
  deliberately not bundled; they are downloaded when chosen.

<p align="center">
  <img src="https://raw.githubusercontent.com/zeiad6/zeidex/main/assets/screenshots/launcher-search.png" alt="Launcher search results" width="47%">
  <img src="https://raw.githubusercontent.com/zeiad6/zeidex/main/assets/screenshots/studio-document.png" alt="Document Studio" width="47%">
</p>

---

## Downloads

| Platform | Package | Size | Best for |
| --- | --- | --- | --- |
| **Windows 10/11 (x64)** | `Zeidex.Studio_0.1.0_x64_en-US.msi` | 48 MB | Standard installer, per-machine or per-user, WiX-based. |
| **Windows 10/11 (x64)** | `Zeidex.Studio_0.1.0_x64-setup.exe` | 44 MB | Lightweight NSIS installer; pick this if MSI is blocked by policy. |
| **Linux (x86_64)** | `Zeidex.Studio_0.1.0_amd64.deb` | 49 MB | Debian, Ubuntu, Mint, Pop!_OS and derivatives. |
| **Linux (x86_64)** | `Zeidex.Studio_0.1.0_amd64.AppImage` | 126 MB | Any distribution; no installation, no root. |
| **Linux (arm64)** | `Zeidex.Studio_0.1.0_arm64.deb` | 52 MB | Raspberry Pi 4/5 and ARM laptops on Debian 12+ or Ubuntu 22.04+. |

> The x64 packages are produced on GitHub's native runners from the tagged source,
> so every artifact matches the commit behind this release. The arm64 package is
> built and startup-tested on aarch64 Linux. macOS bundles are not part of this
> workflow yet — see [Build from source](#build-from-source).

### Install

**Windows — MSI**

```powershell
msiexec /i "Zeidex.Studio_0.1.0_x64_en-US.msi"
```

**Windows — NSIS**

```powershell
"Zeidex.Studio_0.1.0_x64-setup.exe"
```

**Debian / Ubuntu (x86_64)**

```bash
sudo apt install ./Zeidex.Studio_0.1.0_amd64.deb
```

**Debian / Ubuntu (arm64)**

```bash
sudo apt install ./Zeidex.Studio_0.1.0_arm64.deb
```

**AppImage**

```bash
chmod +x "Zeidex.Studio_0.1.0_amd64.AppImage"
./"Zeidex.Studio_0.1.0_amd64.AppImage"
```

The Linux packages depend on `libwebkit2gtk-4.1-0`, `libgtk-3-0` and
`libayatana-appindicator3-1`; `apt` installs them automatically. On distributions
that do not ship WebKitGTK 4.1, use the AppImage.

---

## First run

1. Start Zeidex Studio — the launcher is ready immediately, with no indexing
   wizard and no sign-in.
2. Type to search applications, files and folders; press `Enter` to open, or use
   the arrow keys to move through results.
3. Open **Studio** to drop a scan or PDF, run detection and recognition, edit the
   recognised text, and export to `.docx` or `.xlsx`.
4. Open **Models** to see the three tiers and install a larger one; open
   **Advisor** to have the machine measured and a tier recommended. Neither is
   required — the bundled model works offline out of the box.
5. Open **Batch** to search for a list of filenames at once and export the result
   as an Excel report.
6. Set `ZEIDEX_MODELS_DIR` only if you want the models stored somewhere other than
   the bundled location.

Configuration lives in `~/.zeidex` (Linux/macOS) or `%APPDATA%\zeidex` (Windows).

---

## Verification

Every x64 asset is built from the tag `v0.1.0`. SHA-256 checksums are attached to
this release as `SHA256SUMS`; verify a download before installing:

```bash
sha256sum -c SHA256SUMS --ignore-missing
```

```powershell
Get-FileHash "Zeidex.Studio_0.1.0_x64_en-US.msi" -Algorithm SHA256
```

---

## Known limitations

- **Arabic recognition** is usable but not perfect: dense lines can lose
  characters and Arabic-Indic digits are not read reliably yet. The Precision
  tier uses
  [PP-OCRv5 Arabic](https://huggingface.co/PaddlePaddle/arabic_PP-OCRv5_mobile_rec_onnx),
  which reads clean letterforms noticeably better once its visual-order output is
  converted, but digit runs and mixed Latin text stay weak. It is an optional
  download rather than the default because it has not been measured against a
  labelled corpus of real scans.
- **`.doc` and `.ppt` are refused, not read** — both are binary OLE formats whose
  text lives in compressed streams. `.docx`, `.xlsx`, `.odt` and legacy encoded
  text are supported.
- **Spreadsheet formulas are not evaluated** — a formula cell shows the cached
  result the file stores, which is what Excel last calculated.
- **No auto-update** in this release; download a newer installer when one is
  published.
- **macOS** builds are not produced by this workflow. The shell compiles on macOS
  from source, but no signed `.dmg` ships here.

---

## Build from source

```bash
git clone https://github.com/zeiad6/zeidex.git
cd zeidex
make models            # one-time download of the ONNX models
make core-test         # Rust core test suite
make frontend-install  # npm install
make tauri-build       # platform bundle
```

Requirements: Rust 1.99.1 (pinned in `rust-toolchain.toml`), Node.js 22, and the
Tauri system dependencies for your platform. See
[`assets/docs/build-and-release.md`](https://github.com/zeiad6/zeidex/blob/main/assets/docs/build-and-release.md).

---

## Links

- [README](https://github.com/zeiad6/zeidex#readme) — screenshots, architecture and configuration
- [Contributing](https://github.com/zeiad6/zeidex/blob/main/CONTRIBUTING.md)
- [Issues](https://github.com/zeiad6/zeidex/issues)
- [Developer card](https://zeidexlink.zeidexsoft.com/zeidex) · [zeidexsoft.com](https://zeidexsoft.com)

<p align="center"><sub>Copyright © 2026 Zeidexsoft · GPL-3.0-or-later · Commercial use permitted</sub></p>
