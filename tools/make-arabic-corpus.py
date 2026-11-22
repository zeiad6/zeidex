#!/usr/bin/env python3
"""Render the Arabic OCR benchmark corpus, with its ground truth.

The corpus is deliberately synthetic and deterministic: the same text, font,
size and pitch every run, so a CER measured today can be compared with a CER
measured after a model or preprocessing change. It is not a claim about real
scanned documents; it is a fixed ruler for "did this change help or hurt".

Each line is rendered on its own image because the recognizer works line by
line, and because a failure can then be attributed to one string instead of a
whole page. Ground truth is written as JSON next to the images and committed
with them, so the benchmark runs without this script.

Every character of a line is checked against the chosen font's character map
before it is drawn. An earlier revision of this script rendered the Latin half
of `اختبار PDF` with Noto Naskh Arabic, which has no Latin letters: the image
contained three empty tofu boxes, and the benchmark reported a 40 % error rate
for a limitation the recognizer never had. The check below turns that class of
mistake into a hard failure instead of a number.

Requires Pillow built with libraqm (for `direction="rtl"` and Arabic shaping),
fontTools for the coverage check, and the fonts listed in `LINES`.

Regenerate with `python3 tools/make-arabic-corpus.py`.
"""

from __future__ import annotations

import json
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

NASKH_BOLD = "/usr/share/fonts/truetype/noto/NotoNaskhArabic-Bold.ttf"
NASKH_REGULAR = "/usr/share/fonts/truetype/noto/NotoNaskhArabic-Regular.ttf"
# The only installed Arabic face that also carries Latin capitals: the mixed
# line needs both scripts in one font, or raqm draws .notdef boxes.
PLEX_BOLD = "/usr/share/fonts/truetype/ibm-plex/IBMPlexSansArabic-Bold.ttf"

FONT_SIZE = 44
MARGIN = 24
LINE_HEIGHT = 60
OUT_DIR = Path(__file__).resolve().parent.parent / "core/ocr_engine/tests/fixtures/arabic_corpus"

# (file stem, text, font candidates). Keep each entry short enough for the
# recognizer's line limit and cover the letter substitutions Arabic writers
# actually type. The last entry mixes scripts on purpose and therefore needs a
# dual-script face.
LINES = [
    ("short_greeting", "مرحبا بكم في لوميو", (NASKH_BOLD, NASKH_REGULAR)),
    ("short_book", "كتاب الطالب", (NASKH_BOLD, NASKH_REGULAR)),
    ("long_greeting", "السلام عليكم ورحمة الله", (NASKH_BOLD, NASKH_REGULAR)),
    ("invoice_digits", "الفاتورة رقم ١٢٣٤٥", (NASKH_BOLD, NASKH_REGULAR)),
    ("amount", "المجموع 1250 ريال", (NASKH_BOLD, NASKH_REGULAR)),
    ("report_title", "تقرير الاجتماع الدوري", (NASKH_BOLD, NASKH_REGULAR)),
    ("school", "مدرسة الأطفال", (NASKH_BOLD, NASKH_REGULAR)),
    ("name_maqsura", "مصطفى علي", (NASKH_BOLD, NASKH_REGULAR)),
    ("mixed_latin", "اختبار PDF", (PLEX_BOLD,)),
    ("ai_education", "الذكاء الاصطناعي في التعليم", (NASKH_BOLD, NASKH_REGULAR)),
]


def pick_font(candidates: tuple[str, ...]) -> str:
    for candidate in candidates:
        if Path(candidate).exists():
            return candidate
    raise SystemExit(f"none of these fonts exist: {candidates}")


def assert_coverage(font_path: str, text: str) -> None:
    """Fail loudly when the font cannot draw one of the line's characters.

    This is the check that would have caught the tofu boxes. It runs before
    rendering, so a corpus image can never again contain a placeholder glyph
    that the ground truth still claims is text.
    """
    try:
        from fontTools.ttLib import TTFont
    except ImportError:  # pragma: no cover - dev tool, documented requirement
        raise SystemExit("fontTools is required for the coverage check: pip install fonttools")
    cmap = TTFont(font_path, lazy=True).getBestCmap() or {}
    missing = sorted({ch for ch in text if not ch.isspace() and ord(ch) not in cmap})
    if missing:
        raise SystemExit(
            f"{Path(font_path).name} cannot draw {''.join(missing)!r} from {text!r}"
        )


def render(text: str, font: ImageFont.FreeTypeFont, path: Path) -> None:
    # Render wide first, then crop to the ink so every image has the same
    # padding the detector expects: no huge white field, no clipped edges.
    probe = Image.new("L", (10, 10), 255)
    width = ImageDraw.Draw(probe).textlength(text, font=font, direction="rtl", language="ar")
    canvas = Image.new("L", (int(width) + MARGIN * 2, LINE_HEIGHT + MARGIN * 2), 255)
    ImageDraw.Draw(canvas).text(
        (MARGIN, MARGIN), text, font=font, fill=0, direction="rtl", language="ar"
    )
    bbox = canvas.point(lambda value: 255 - value).getbbox()
    if bbox is None:
        raise SystemExit(f"rendering {text!r} produced no ink")
    left, top, right, bottom = bbox
    canvas.crop(
        (max(0, left - MARGIN), max(0, top - MARGIN), right + MARGIN, bottom + MARGIN)
    ).convert("RGB").save(path)


def main() -> None:
    OUT_DIR.mkdir(parents=True, exist_ok=True)

    truth = []
    for name, text, candidates in LINES:
        font_path = pick_font(candidates)
        assert_coverage(font_path, text)
        font = ImageFont.truetype(font_path, FONT_SIZE)
        file_name = f"{name}.png"
        render(text, font, OUT_DIR / file_name)
        truth.append({"file": file_name, "text": text, "font": Path(font_path).name})
        print(f"rendered {file_name}: {text} ({Path(font_path).name})")

    (OUT_DIR / "ground_truth.json").write_text(
        json.dumps(
            {"size": FONT_SIZE, "lines": truth},
            ensure_ascii=False,
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )
    print(f"wrote {OUT_DIR / 'ground_truth.json'}")


if __name__ == "__main__":
    main()
