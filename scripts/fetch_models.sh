#!/usr/bin/env bash
# Downloads the offline ONNX models and dictionaries Zeidex's OCR engine needs.
#
# Everything is fetched once, at setup time, into a local folder. The desktop
# shell points ZEIDEX_MODELS_DIR at that folder; the app itself never touches
# the network.
#
# Usage:
#   ./scripts/fetch_models.sh                 # -> third_party/models
#   ZEIDEX_MODELS_DIR=/opt/zeidex/models ./scripts/fetch_models.sh
#
# Sources (Apache-2.0 PP-OCRv4 / PP-StructureV2 exports, mirrored on Hugging
# Face). The PaddleOCR model licence applies; see third_party/README.md.
set -euo pipefail

DEST="${ZEIDEX_MODELS_DIR:-third_party/models}"
HOST="https://huggingface.co"

mkdir -p "$DEST"

fetch() {
    local url="$1" dest="$2"
    if [[ -s "$dest" ]]; then
        echo "already present: $dest"
        return
    fi
    echo "downloading: $url"
    curl -fL --retry 3 --retry-delay 2 -o "$dest.part" "$url"
    mv "$dest.part" "$dest"
}

# Text detection: PP-OCRv4 (DBNet).
fetch "$HOST/cycloneboy/ch_PP-OCRv4_det_infer/resolve/main/model.onnx" \
    "$DEST/ppocrv4_det.onnx"

# Text recognition: English and Arabic PP-OCRv4.
fetch "$HOST/cycloneboy/en_PP-OCRv4_rec_infer/resolve/main/model.onnx" \
    "$DEST/ppocrv4_rec_en.onnx"
fetch "$HOST/cycloneboy/arabic_PP-OCRv4_rec_infer/resolve/main/model.onnx" \
    "$DEST/ppocrv4_rec_ar.onnx"

# Dictionaries (one character per line, line 0 is the CTC blank).
fetch "$HOST/cycloneboy/en_PP-OCRv4_rec_infer/resolve/main/en_dict.txt" \
    "$DEST/ppocr_keys_v1.txt"
fetch "$HOST/cycloneboy/arabic_PP-OCRv4_rec_infer/resolve/main/arabic_dict.txt" \
    "$DEST/arabic_dict.txt"

# Layout: PP-StructureV2 PicoDet layout model. `infer_cfg.yml` carries the
# `label_list` the engine maps class indices through, so the map is never guessed.
fetch "$HOST/cycloneboy/picodet_lcnet_x1_0_fgd_layout_infer/resolve/main/model.onnx" \
    "$DEST/ppstructurev2_layout.onnx"
fetch "$HOST/cycloneboy/picodet_lcnet_x1_0_fgd_layout_infer/resolve/main/infer_cfg.yml" \
    "$DEST/infer_cfg.yml"

# Table structure: PP-StructureV2 SLANet, plus the HTML token class list its
# structure head was trained with.
fetch "$HOST/cycloneboy/en_ppstructure_mobile_v2.0_SLANet_infer/resolve/main/model.onnx" \
    "$DEST/slanet_table.onnx"
fetch "$HOST/cycloneboy/en_ppstructure_mobile_v2.0_SLANet_infer/resolve/main/table_structure_dict.txt" \
    "$DEST/table_structure_dict.txt"

echo
echo "Models written to $DEST"
# The manifest is reproducible: sorted file list, no checksum of itself. Paths
# stay relative to the caller so `sha256sum -c third_party/models/SHA256SUMS`
# works from the repository root.
mapfile -t manifest < <(ls "$DEST"/*.onnx "$DEST"/*.txt "$DEST"/*.yml 2>/dev/null | LC_ALL=C sort)
if [[ ${#manifest[@]} -gt 0 ]]; then
    sha256sum "${manifest[@]}" > "$DEST/SHA256SUMS"
    cat "$DEST/SHA256SUMS"
fi
