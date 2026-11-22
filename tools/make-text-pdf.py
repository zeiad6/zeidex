#!/usr/bin/env python3
"""Write a small PDF that has a real text layer (no raster, no OCR needed).

`pdf_native_text.rs` uses it to prove that a born-digital PDF is read through
pdfium's text API instead of being rendered at 300 DPI and OCRed. The strings
are deliberately distinctive so an OCR pass could not plausibly produce them by
accident.

Regenerate with `python3 tools/make-text-pdf.py`; the output is committed so the
test does not need reportlab.
"""

from pathlib import Path

from reportlab.pdfgen import canvas

OUT = Path(__file__).resolve().parent.parent / "core/ocr_engine/tests/fixtures/text_layer.pdf"

LINES = [
    "ZEIDEX-NATIVE-TEXT-42",
    "This page carries a real text layer, so OCR must not run on it.",
]


def main() -> None:
    OUT.parent.mkdir(parents=True, exist_ok=True)
    # invariant=1 keeps the file byte-identical across runs: no timestamp or
    # random document id, so regenerating it does not dirty the tree.
    pdf = canvas.Canvas(str(OUT), invariant=1, pagesize=(595, 842))
    pdf.setTitle("Zeidex native text fixture")
    pdf.setAuthor("Zeidexsoft")
    pdf.setFont("Helvetica", 18)
    y = 780
    for line in LINES:
        pdf.drawString(60, y, line)
        y -= 40
    pdf.showPage()
    pdf.save()
    print(f"wrote {OUT}")


if __name__ == "__main__":
    main()
