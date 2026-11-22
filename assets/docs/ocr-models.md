# OCR models, tiers and the hardware advisor

Zeidex runs OCR entirely on the machine. The bundled models are small and fast;
the **OCR Models** screen adds two larger sets for users who need them, and a
hardware advisor that says which one this machine can actually run.

```text
bundled (Tier 1) ──▶ Models screen ──▶ download ──▶ SHA-256 ──▶ activate
                            │
                            └──▶ Advisor: probe ──▶ class ──▶ recommendation
```

| Layer | Where |
| --- | --- |
| Catalog, paths, install rules, classification | `core/models` (`zeidex-models`), pure Rust |
| Download, hardware probe, Tauri commands | `src-tauri/src/commands/models.rs`, `src-tauri/src/hardware.rs` |
| Screen | `app/src/components/Models.tsx`, `ModelCard.tsx`, `AdvisorCard.tsx` |

## The three tiers

| Tier | Pack id | Detection | Recognition | Download | RAM | Arabic |
| --- | --- | --- | --- | --- | --- | --- |
| Lightweight · Fast | `ppocrv4-mobile` (bundled) | PP-OCRv4 mobile | v4 mobile, EN + AR | 0 (in the installer) | ≈ 320 MB | v4 Arabic |
| Balanced · Recommended | `ppocrv4-server-det` | PP-OCRv4 server | v4 mobile, EN + AR | ≈ 129 MB | ≈ 900 MB | v4 Arabic |
| Advanced · High accuracy | `ppocrv5-server` | PP-OCRv5 server | v5 server (EN) + v5 mobile (AR) | ≈ 255 MB | ≈ 1.6 GB | v5 Arabic, visual-order output converted |

A pack replaces **detection and recognition only**. The layout model
(`ppstructurev2_layout.onnx`) and the table model (`slanet_table.onnx`) stay in
the bundled folder and are shared by every tier, so a pack download is the part
that actually differs.

Every file is pinned to an immutable upstream revision (a Hugging Face commit
SHA or the PaddleOCR commit `a38c087b…`), with its SHA-256 and byte count in the
catalog. The Arabic v5 recognizer publishes no standalone dictionary, so the
installer downloads its `inference.yml` and extracts `PostProcess.character_dict`
into `arabic_dict.txt` (`FileFormat::CharacterDictYaml`).

Sources and licences are part of each catalog entry: PaddleOCR PP-OCRv4/v5
(Apache-2.0) via the `cycloneboy` ONNX exports, and PaddlePaddle's
`arabic_PP-OCRv5_mobile_rec_onnx` (Apache-2.0).

## Where the files live

Two roots, because the bundled folder may be read-only:

| Root | Default | Purpose |
| --- | --- | --- |
| `models_dir` | bundled resources / `third_party/models` | the files that ship with the app |
| `models_root` | platform data dir, e.g. `~/.local/share/zeidex/models` | downloaded packs and the active marker |

`ZEIDEX_MODELS_ROOT` overrides the writable root (the tests use it).

```text
<models_root>/
  packs/
    <pack-id>/            installed pack: files + manifest.json
    .staging-<pack-id>/   an install in progress
    <file>.part           a resumable partial download
  active-pack.json        {"pack_id": "ppocrv5-server"}
```

`manifest.json` is written only after every file has been hashed, and a pack is
reported as **Installed** only when its manifest is present. Activation of a
downloaded pack hashes every file again, so a model damaged on disk cannot be
selected because its manifest still exists. A marker that names a pack which is
no longer installed is ignored and the shell falls back to the bundled tier.

## Installing

1. **Disk check** — the catalog size plus 128 MB must be free on the models
   volume; otherwise the download is refused before a byte is written.
2. **Download** — one file at a time, streamed to `<name>.part`. A retry sends
   `Range: bytes=<offset>-` and appends; a server that ignores `Range` answers
   `200`, and the partial file is restarted rather than doubled. A 120 s read
   timeout turns a stalled connection into a retryable error. Cancel is checked
   between chunks.
3. **Verify** — SHA-256 of the complete file. A mismatch deletes the `.part`
   and fails with both digests in the message; the installed pack is untouched.
4. **Prepare** — the YAML dictionary is extracted; everything else is stored as
   published.
5. **Stage → promote** — files land in `.staging-<id>`, the manifest is written,
   the staging folder is verified again, and only then is it renamed into place.
   A failed download leaves the previous pack working.
6. **Activate** — `active-pack.json` is written beside the target and renamed,
   so a crash leaves either the old marker or the new one, never half a file.
   The OCR engine is dropped so the next run loads the new pack.

Progress is emitted as `models-progress` events: phase (`downloading`,
`verifying`, `installing`), file and file index, downloaded and total bytes,
bytes per second, and a message for failures. The UI also shows installed size,
required RAM, Arabic/English support and the active pack.

## The hardware advisor

The probe uses `sysinfo` for CPU model, physical and logical cores, total and
available RAM, OS and architecture. On Linux it reads the PCI vendor from
`/sys/class/drm/card*/device/vendor`, takes AMD VRAM from
`mem_info_vram_total`, and takes the NVIDIA board name and VRAM from
`/proc/driver/nvidia/gpus/*/information`; it does not shell out and does not
need root. Free space on the models volume and the process RSS are measured
too.

### Classification

Seven criteria, one point each, summed into a score:

| Criterion | Point |
| --- | --- |
| ≥ 4 logical cores | 1 |
| ≥ 8 logical cores | 1 |
| ≥ 16 logical cores | 1 |
| ≥ 8 GB RAM | 1 |
| ≥ 16 GB RAM | 1 |
| ≥ 32 GB RAM | 1 |
| GPU acceleration available in this build and detected | 1 |

| Score | Class |
| --- | --- |
| 0–1 | Basic |
| 2–3 | Standard |
| 4–5 | Advanced |
| 6–7 | High performance |

The criteria and their points are returned with the recommendation, so the
screen can show *why* a machine was classified the way it was.

### Recommendation

The RAM budget is a quarter of total RAM, capped by what is currently available
(an unmeasured profile is not guessed at). A pack is eligible when its
requirement plus 25% headroom fits that budget, and the advisor steps down a
tier while the pack does not fit on disk, never below the bundled one. It then
reports:

- the recommended pack, with a reason and the trade-offs in both languages;
- whether it had to downgrade because of memory pressure;
- suggested OCR worker count, batch chunk size and indexing threads for the
  class;
- whether GPU acceleration is *built in* and *actually detected* — those are
  different questions and are shown separately.

The advisor never claims a speed or accuracy figure before a benchmark has run
on this machine.

## Benchmark (opt-in)

**Test** on a card runs that pack over the bundled sample images — one English,
one Arabic — and reports per sample: milliseconds, characters read, expected
characters, character error rate, and the text itself so the number can be
checked. **Compare models** runs every installed pack in sequence (not in
parallel, which would make each look slower and overlap the memory figures) and
summarises total milliseconds.

CER is plain Levenshtein distance over characters divided by the length of the
expected text, so a transposition counts as two edits. Confidences are asserted
to be within `0..1` for every pack in `core/ocr_engine/tests/pack_smoke.rs`: a
recognizer that emits logits instead of probabilities would otherwise produce
nonsense scores silently.

Measured on this development host (release build, one page, model load
included): `ppocrv4-server-det` 3.23 s and `ppocrv5-server` 2.92 s for the
Arabic sample, 56 characters read, every confidence inside `0..1`.

## Failure modes

| What happens | What the user sees |
| --- | --- |
| Network drops mid-file | Retry resumes from the byte count already on disk |
| Server ignores `Range` | The partial file is discarded and the file restarts |
| SHA-256 mismatch | The file is deleted, the digests are shown, the old pack stays |
| Disk full before start | Refused with the required and available megabytes |
| Cancel | The `.part` is kept for a later resume; the pack stays not installed |
| Marker points at a removed pack | Falls back to the bundled tier |
| `manifest.json` missing | Shown as partial, not installed, and cannot be activated |

Nothing in this flow contacts a service other than the pinned download URL, and
no document, image or extracted text is ever sent anywhere.
