# Third-party models

Lumio's OCR engine runs entirely offline. The ONNX models and dictionaries it
loads live here (or wherever `LUMIO_MODELS_DIR` points).

```bash
./scripts/fetch_models.sh
# or: LUMIO_MODELS_DIR=/path/to/models ./scripts/fetch_models.sh
```

| File | Model | Licence |
| --- | --- | --- |
| `ppocrv4_det.onnx` | PP-OCRv4 text detection (DBNet) | Apache-2.0 |
| `ppocrv4_rec_en.onnx` | PP-OCRv4 English recognition | Apache-2.0 |
| `ppocrv4_rec_ar.onnx` | PP-OCRv4 Arabic recognition | Apache-2.0 |
| `ppocr_keys_v1.txt` | English character dictionary | Apache-2.0 |
| `arabic_dict.txt` | Arabic character dictionary | Apache-2.0 |
| `ppstructurev2_layout.onnx` | PP-StructureV2 layout (PicoDet) | Apache-2.0 |
| `infer_cfg.yml` | PicoDet `label_list` and NMS settings | Apache-2.0 |
| `slanet_table.onnx` | PP-StructureV2 table structure (SLANet) | Apache-2.0 |
| `table_structure_dict.txt` | SLANet structure-token classes | Apache-2.0 |

The downloads are Hugging Face mirrors of the PaddleOCR model zoo exports
(`cycloneboy/*`). `SHA256SUMS` is written by the fetch script (sorted, one line
per file, relative to the directory you ran it from); verify it with
`sha256sum -c third_party/models/SHA256SUMS` from the repository root.

The ONNX Runtime shared library is not vendored here. `ort` loads it
dynamically at runtime (`load-dynamic`); drop `libonnxruntime.so` /
`onnxruntime.dll` / `libonnxruntime.dylib` into `src-tauri/resources/lib/` so
the bundler ships it, or install it system-wide. pdfium is the same:
`pdfium-render` binds to `libpdfium.so` / `pdfium.dll` / `libpdfium.dylib` at
runtime.
