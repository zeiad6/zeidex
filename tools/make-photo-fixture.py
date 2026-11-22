#!/usr/bin/env python3
"""Renders the phone-photo fixtures used by the orientation tests.

A phone stores the sensor frame and records how to turn it upright in the EXIF
``Orientation`` tag (274). The bytes on disk can therefore be sideways while
every viewer shows the page upright — and ``image::open`` ignores the tag, so
the OCR pipeline used to read such a photo sideways.

Two fixtures come out of this script, both rotated 90° CCW and saved with
``Orientation=6`` ("rotate 90° CW to display"), which is the sensor frame of a
portrait shot of a landscape page:

* ``photo-rotated.jpg`` — a 320×200 marker drawing, for the unit test that
  needs no models and can assert exact pixels.
* ``photo-table.jpg`` — ``table.png``, for the model-backed test that reads a
  real table out of a sideways photo.

    python3 tools/make-photo-fixture.py
"""

from __future__ import annotations

from pathlib import Path

from PIL import Image, ImageDraw

WIDTH, HEIGHT = 320, 200
BLACK = (0, 0, 0)
WHITE = (255, 255, 255)
RED = (220, 30, 30)

ROOT = Path(__file__).resolve().parent.parent
FIXTURES = ROOT / "core" / "ocr_engine" / "tests" / "fixtures"


def save_sideways(display: Image.Image, out: Path, quality: int) -> None:
    """Stores ``display`` as the sensor frame a phone writes for orientation 6."""
    stored = display.transpose(Image.Transpose.ROTATE_90)
    exif = Image.Exif()
    exif[274] = 6
    stored.save(out, quality=quality, exif=exif)
    print(f"wrote {out} ({stored.width}x{stored.height}, orientation 6)")


def marker_page() -> Image.Image:
    # A black block in the top-left quadrant and a red block in the bottom-right
    # corner, so a wrong orientation is unmistakable at a glance and in a test.
    display = Image.new("RGB", (WIDTH, HEIGHT), WHITE)
    draw = ImageDraw.Draw(display)
    draw.rectangle([0, 0, WIDTH // 2 - 1, HEIGHT // 2 - 1], fill=BLACK)
    draw.rectangle([WIDTH - 40, HEIGHT - 30, WIDTH - 1, HEIGHT - 1], fill=RED)
    return display


def main() -> None:
    save_sideways(marker_page(), FIXTURES / "photo-rotated.jpg", quality=95)
    table = Image.open(FIXTURES / "table.png").convert("RGB")
    save_sideways(table, FIXTURES / "photo-table.jpg", quality=92)


if __name__ == "__main__":
    main()
