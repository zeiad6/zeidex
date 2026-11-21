#!/usr/bin/env python3
"""Renders the sample invoice used by the browser demo and the README.

The demo mounts the real React tree against a mocked Tauri bridge; this page is
what the mock returns as the OCR page preview. Everything is generated from the
constants below, so the image and the region boxes in ``app/src/demo/fixtures.ts``
can never drift apart: run this script, then paste the printed JSON.

    python3 tools/make-demo-page.py
"""

from __future__ import annotations

import json
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

WIDTH, HEIGHT = 1240, 1600
INK = (23, 27, 34)
MUTED = (110, 118, 132)
RULE = (32, 38, 48)
PAPER = (252, 252, 250)

FONT_DIR = Path("/usr/share/fonts/truetype/dejavu")
BOLD = FONT_DIR / "DejaVuSans-Bold.ttf"
REGULAR = FONT_DIR / "DejaVuSans.ttf"
MONO = FONT_DIR / "DejaVuSansMono.ttf"

OUT = Path(__file__).resolve().parent.parent / "app" / "src" / "demo" / "sample-page.png"


def font(path: Path, size: int) -> ImageFont.FreeTypeFont:
    return ImageFont.truetype(str(path), size)


def main() -> None:
    image = Image.new("RGB", (WIDTH, HEIGHT), PAPER)
    draw = ImageDraw.Draw(image)

    title_box = (90, 118, 820, 196)
    draw.text((90, 120), "AL NOOR TRADING CO.", font=font(BOLD, 46), fill=INK)
    draw.text((90, 178), "Document services · Riyadh", font=font(REGULAR, 22), fill=MUTED)

    draw.text((880, 128), "INVOICE", font=font(BOLD, 20), fill=MUTED)
    draw.text((880, 156), "2026-014", font=font(MONO, 30), fill=INK)
    draw.text((880, 196), "Issued 12 Feb 2026", font=font(REGULAR, 18), fill=MUTED)
    draw.line((90, 236, 1150, 236), fill=RULE, width=2)

    bill_box = (90, 268, 620, 430)
    draw.text((90, 268), "BILL TO", font=font(BOLD, 15), fill=MUTED)
    bill_lines = [
        "Najd Logistics",
        "Olaya District, Building 12",
        "Riyadh 12211, Saudi Arabia",
        "VAT 3100 4421 9900 003",
    ]
    for index, line in enumerate(bill_lines):
        draw.text((90, 296 + index * 30), line, font=font(REGULAR, 21), fill=INK)

    draw.text((700, 268), "PAYMENT", font=font(BOLD, 15), fill=MUTED)
    for index, line in enumerate(["Due 26 Feb 2026", "Bank transfer · SAR", "Ref INV-2026-014"]):
        draw.text((700, 296 + index * 30), line, font=font(REGULAR, 21), fill=INK)

    draw.text((90, 476), "DESCRIPTION OF SERVICES", font=font(BOLD, 15), fill=MUTED)

    cols = [90, 700, 850, 1150]
    rows = [548, 606, 664, 722, 780, 838]
    for y in rows:
        draw.line((cols[0], y, cols[-1], y), fill=RULE, width=2)
    for x in cols:
        draw.line((x, rows[0], x, rows[-1]), fill=RULE, width=2)

    header = ["Description", "Qty", "Amount"]
    body = [
        ["Document scanning setup", "1", "1,450.00"],
        ["Arabic OCR licence", "2", "780.00"],
        ["On-site training (hours)", "6", "540.00"],
        ["Priority support", "1", "260.00"],
    ]
    for column, text in enumerate(header):
        draw.text((cols[column] + 18, rows[0] + 18), text, font=font(BOLD, 21), fill=MUTED)
    for row_index, row in enumerate(body):
        y = rows[row_index + 1] + 18
        for column, text in enumerate(row):
            align = "left" if column == 0 else "right"
            x = cols[column] + 18 if align == "left" else cols[column + 1] - 18
            draw.text(
                (x, y),
                text,
                font=font(REGULAR, 22),
                fill=INK,
                anchor="la" if align == "left" else "ra",
            )

    draw.text((850, 856), "TOTAL DUE", font=font(BOLD, 15), fill=MUTED)
    draw.text((1150, 878), "3,030.00 SAR", font=font(BOLD, 32), fill=INK, anchor="ra")

    draw.line((90, 1450, 1150, 1450), fill=(224, 226, 230), width=1)
    draw.text(
        (90, 1466),
        "Thank you for your business · billing@alnoor.example · +966 11 000 0000",
        font=font(REGULAR, 17),
        fill=MUTED,
    )

    OUT.parent.mkdir(parents=True, exist_ok=True)
    image.save(OUT, optimize=True)

    table_bbox = [float(cols[0]), float(rows[0]), float(cols[-1]), float(rows[-1])]
    cells = []
    for row_index in range(len(rows) - 1):
        for column in range(len(cols) - 1):
            cells.append(
                {
                    "row": row_index,
                    "col": column,
                    "row_span": 1,
                    "col_span": 1,
                    "bbox": {
                        "x0": float(cols[column]),
                        "y0": float(rows[row_index]),
                        "x1": float(cols[column + 1]),
                        "y1": float(rows[row_index + 1]),
                    },
                    "text": (
                        header[column]
                        if row_index == 0
                        else body[row_index - 1][column]
                    ),
                }
            )

    page = {
        "page_index": 0,
        "width": WIDTH,
        "height": HEIGHT,
        "text": "AL NOOR TRADING CO.\nINVOICE 2026-014\nTotal due 3,030.00 SAR",
        "layout": {
            "width": WIDTH,
            "height": HEIGHT,
            "regions": [
                {
                    "kind": "title",
                    "bbox": {"x0": 90.0, "y0": 112.0, "x1": 820.0, "y1": 206.0},
                    "confidence": 0.98,
                    "text": "AL NOOR TRADING CO.",
                    "table": None,
                },
                {
                    "kind": "text",
                    "bbox": {"x0": 90.0, "y0": 262.0, "x1": 620.0, "y1": 422.0},
                    "confidence": 0.96,
                    "text": "\n".join(bill_lines),
                    "table": None,
                },
                {
                    "kind": "table",
                    "bbox": {
                        "x0": table_bbox[0],
                        "y0": table_bbox[1],
                        "x1": table_bbox[2],
                        "y1": table_bbox[3],
                    },
                    "confidence": 0.94,
                    "text": None,
                    "table": {
                        "bbox": {
                            "x0": table_bbox[0],
                            "y0": table_bbox[1],
                            "x1": table_bbox[2],
                            "y1": table_bbox[3],
                        },
                        "rows": len(rows) - 1,
                        "cols": len(cols) - 1,
                        "cells": cells,
                    },
                },
                {
                    "kind": "text",
                    "bbox": {"x0": 820.0, "y0": 848.0, "x1": 1150.0, "y1": 924.0},
                    "confidence": 0.97,
                    "text": "Total due 3,030.00 SAR",
                    "table": None,
                },
                {
                    "kind": "footer",
                    "bbox": {"x0": 90.0, "y0": 1442.0, "x1": 1150.0, "y1": 1496.0},
                    "confidence": 0.9,
                    "text": "Thank you for your business · billing@alnoor.example",
                    "table": None,
                },
            ],
        },
    }
    print(json.dumps(page, ensure_ascii=False, indent=2))
    print(f"\nwrote {OUT}", flush=True)


if __name__ == "__main__":
    main()
