#!/usr/bin/env python3
"""Build the NSIS installer artwork from the Zeidex brand mark.

    python3 assets/installer/generate.py

Outputs (24-bit BMP, the format NSIS requires):

    header.bmp   150x57   header of every installer page
    sidebar.bmp  164x314  Welcome and Finish pages

The mark is rendered from `assets/brand/zeidex-mark.svg` with rsvg-convert, so
the installer carries the same logo as the window, the icon and the README
instead of a second hand-drawn copy that drifts.
"""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[2]
OUT = Path(__file__).resolve().parent
MARK_SVG = ROOT / "assets" / "brand" / "zeidex-mark.svg"

INK = (7, 9, 16)
PANEL = (21, 26, 38)
ACCENT = (34, 211, 238)
IRIS = (167, 139, 250)
TEXT = (230, 235, 245)
MUTED = (150, 158, 175)

FONT_BOLD = "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"
FONT_REGULAR = "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"


def font(path: str, size: int) -> ImageFont.FreeTypeFont:
    try:
        return ImageFont.truetype(path, size)
    except OSError as err:  # pragma: no cover - host without DejaVu
        sys.exit(f"font not found: {path} ({err})")


def render_mark(size: int) -> Image.Image:
    """The brand mark at `size` px, straight from the shipped SVG."""
    png = OUT / "_mark.png"
    subprocess.run(
        ["rsvg-convert", "-w", str(size), "-h", str(size), str(MARK_SVG), "-o", str(png)],
        check=True,
    )
    try:
        return Image.open(png).convert("RGBA")
    finally:
        png.unlink(missing_ok=True)


def vertical_gradient(size: tuple[int, int], top: tuple[int, int, int], bottom: tuple[int, int, int]) -> Image.Image:
    width, height = size
    image = Image.new("RGB", size)
    draw = ImageDraw.Draw(image)
    for y in range(height):
        t = y / max(1, height - 1)
        color = tuple(round(a + (b - a) * t) for a, b in zip(top, bottom))
        draw.line([(0, y), (width, y)], fill=color)
    return image


def glow(image: Image.Image, center: tuple[int, int], radius: int, color: tuple[int, int, int]) -> None:
    """A soft radial pool, drawn additively so it never turns into a flat disc."""
    overlay = Image.new("RGB", image.size, (0, 0, 0))
    draw = ImageDraw.Draw(overlay)
    steps = 48
    for step in range(steps, 0, -1):
        t = step / steps
        r = int(radius * t)
        strength = (1.0 - t) ** 2 * 0.5
        fill = tuple(round(channel * strength) for channel in color)
        draw.ellipse(
            [center[0] - r, center[1] - r, center[0] + r, center[1] + r],
            fill=fill,
        )
    image.paste(Image.blend(image, Image.new("RGB", image.size, (0, 0, 0)), 0.0))
    from PIL import ImageChops

    merged = ImageChops.add(image, overlay)
    image.paste(merged)


def header() -> Image.Image:
    image = vertical_gradient((150, 57), (23, 28, 40), INK)
    glow(image, (18, 12), 46, ACCENT)
    mark = render_mark(34)
    image.paste(mark, (9, 11), mark)
    draw = ImageDraw.Draw(image)
    draw.text((50, 13), "ZEIDEX", font=font(FONT_BOLD, 14), fill=TEXT)
    draw.text((51, 32), "S E A R C H", font=font(FONT_REGULAR, 7), fill=MUTED)
    # Hairline along the bottom, the same accent rule the panel uses.
    draw.line([(0, 56), (150, 56)], fill=(40, 48, 64))
    draw.line([(0, 56), (74, 56)], fill=ACCENT)
    return image


def sidebar() -> Image.Image:
    image = vertical_gradient((164, 314), (17, 21, 31), INK)
    glow(image, (30, 34), 120, ACCENT)
    glow(image, (140, 250), 110, IRIS)
    mark = render_mark(76)
    image.paste(mark, ((164 - 76) // 2, 42), mark)
    draw = ImageDraw.Draw(image)
    title = "ZEIDEX"
    title_font = font(FONT_BOLD, 24)
    width = draw.textlength(title, font=title_font)
    draw.text(((164 - width) / 2, 136), title, font=title_font, fill=TEXT)
    sub = "S E A R C H"
    sub_font = font(FONT_REGULAR, 9)
    width = draw.textlength(sub, font=sub_font)
    draw.text(((164 - width) / 2, 168), sub, font=sub_font, fill=ACCENT)
    draw.line([(52, 196), (112, 196)], fill=(48, 57, 76))
    lines = ["Offline file search", "OCR for Arabic and English", "PDF to Word and Excel"]
    line_font = font(FONT_REGULAR, 9)
    y = 214
    for line in lines:
        width = draw.textlength(line, font=line_font)
        draw.text(((164 - width) / 2, y), line, font=line_font, fill=MUTED)
        y += 17
    footer = "No account. No telemetry."
    width = draw.textlength(footer, font=line_font)
    draw.text(((164 - width) / 2, 288), footer, font=line_font, fill=(110, 118, 134))
    return image


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    header().save(OUT / "header.bmp", format="BMP")
    sidebar().save(OUT / "sidebar.bmp", format="BMP")
    for name in ("header.bmp", "sidebar.bmp"):
        with Image.open(OUT / name) as image:
            print(f"{name}: {image.size[0]}x{image.size[1]} {image.mode}")


if __name__ == "__main__":
    main()
