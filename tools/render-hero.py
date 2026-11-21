#!/usr/bin/env python3
"""Renders the animated README banner from assets/brand/zeidex-hero.svg.

Motion is a deterministic function of the frame index (no randomness), so the
GIF is reproducible: aurora pools drift, the scan beam sweeps the page, and the
three nodes breathe. rsvg-convert rasterizes each frame; ffmpeg encodes the
palette GIF.
"""
from __future__ import annotations

import math
import shutil
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SVG = ROOT / "assets" / "brand" / "zeidex-hero.svg"
GIF = ROOT / "assets" / "brand" / "zeidex-hero-animated.gif"
PNG = ROOT / "assets" / "brand" / "zeidex-hero-static.png"
W, H, FPS, FRAMES = 1000, 333, 10, 24


def strip_ancillary_chunks(path: Path) -> None:
    """Drop PNG ancillary chunks (rsvg writes bKGD).

    GitHub's README renderer accepts them, but the animated-README contract
    requires a plain raster the audit can verify; bKGD is the only offender and
    carries no pixels.
    """
    data = path.read_bytes()
    out = bytearray(data[:8])
    offset = 8
    while offset < len(data):
        length = struct.unpack(">I", data[offset : offset + 4])[0]
        kind = data[offset + 4 : offset + 8]
        end = offset + 12 + length
        if kind[:1].isupper() or kind in (b"tRNS", b"gAMA"):
            out += data[offset:end]
        offset = end
        if kind == b"IEND":
            break
    path.write_bytes(bytes(out))


def frame(index: int, source: str) -> str:
    phase = 2 * math.pi * index / FRAMES
    ax = 150 + 26 * math.sin(phase)
    ay = 40 + 18 * math.cos(phase)
    bx = 1050 + 30 * math.cos(phase + 0.8)
    by = 380 - 20 * math.sin(phase + 0.8)
    beam = 150 + 170 * (0.5 - 0.5 * math.cos(phase))
    source = source.replace('cx="150" cy="40" r="260"', f'cx="{ax:.1f}" cy="{ay:.1f}" r="260"')
    source = source.replace('cx="1050" cy="380" r="280"', f'cx="{bx:.1f}" cy="{by:.1f}" r="280"')
    source = source.replace('y="192" width="224" height="18"', f'y="{beam - 7:.1f}" width="224" height="18"')
    source = source.replace('y="199" width="224" height="3"', f'y="{beam:.1f}" width="224" height="3"')
    return source


def main() -> int:
    source = SVG.read_text(encoding="utf-8")
    with tempfile.TemporaryDirectory() as tmp:
        work = Path(tmp)
        for index in range(FRAMES):
            (work / f"f{index:03d}.svg").write_text(frame(index, source), encoding="utf-8")
            subprocess.run(
                ["rsvg-convert", "-w", str(W), "-h", str(H),
                 "-o", str(work / f"f{index:03d}.png"), str(work / f"f{index:03d}.svg")],
                check=True,
            )
        subprocess.run(["cp", str(work / "f000.png"), str(PNG)], check=True)
        strip_ancillary_chunks(PNG)
        subprocess.run(
            ["ffmpeg", "-y", "-loglevel", "error", "-framerate", str(FPS),
             "-i", str(work / "f%03d.png"), "-vf",
             "split[a][b];[a]palettegen=max_colors=64:stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4:diff_mode=rectangle",
             "-loop", "0", str(GIF)],
            check=True,
        )
    # Lossy GIF pass: the aurora gradients are flat, so a high loss budget
    # removes dithering noise without visible artefacts, and the banner stays
    # inside GitHub's comfortable image weight.
    if shutil.which("gifsicle"):
        subprocess.run(
            ["gifsicle", "-O3", "--lossy=90", "--colors", "64", str(GIF), "-o", str(GIF)],
            check=True,
        )
    print(f"wrote {PNG.name} and {GIF.name}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
