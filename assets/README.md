# Assets

| Path | What it is |
| --- | --- |
| `brand/zeidex-mark.svg` | The mark. Pure geometry, transparent background, validated for repository-safe rendering. |
| `brand/zeidex-mark.png` | 512 px render of the mark, used by the Tauri icon generator. |
| `brand/zeidex-hero.svg` | README banner master (1200 × 400). Text, mark, and the page card. |
| `brand/zeidex-hero-static.png` | Static banner — the fallback for `prefers-reduced-motion` and slow connections. |
| `brand/zeidex-hero-animated.gif` | Animated banner: drifting aurora, sweeping scan line, breathing nodes. |
| `screenshots/` | Real UI captures, produced from the components (see below). |
| `legacy-ui/` | Screenshots of the pre-Tauri macOS shell. Historical only. |
| `docs/` | Architecture and build docs. |
| `icon-source.png` | 1024 px master for the platform icon, rendered from the mark. |
| `icon.png` | 512 px copy. |

## Regenerating the banner

Motion is a deterministic function of the frame index, so the GIF is reproducible:

```bash
python3 tools/render-hero.py
```

`rsvg-convert` rasterizes each frame, `ffmpeg` builds the palette, and `gifsicle`
(optional) does the lossy pass. The script keeps the GIF around 400 KB; keep it under
1 MB, or GitHub's image proxy starts to crawl.

## Regenerating the screenshots

The screenshots are not mockups. `app/src/demo/mock.ts` installs a fake Tauri IPC bridge
in front of the real React tree, so the components, fonts, spacing and RTL behaviour are
the shipping ones:

```bash
cd app && npm run dev            # then open http://localhost:5173/?demo=launcher
```

`?demo=ocr` starts in the Studio. To re-capture with Playwright, point a Chromium
instance at `http://localhost:5173`, click through search, calculator, clipboard,
translation and the OCR flow, and save `.panel` element screenshots into
`screenshots/`.

## Updating the app icon

1. Edit `brand/zeidex-mark.svg`.
2. Render `brand/zeidex-mark.png` at 512 px and copy it to `icon-source.png` (1024 px)
   and `icon.png` (512 px):

   ```bash
   rsvg-convert -w 1024 -h 1024 -o assets/icon-source.png assets/brand/zeidex-mark.svg
   rsvg-convert -w 512  -h 512  -o assets/icon.png        assets/brand/zeidex-mark.svg
   ```

3. Regenerate every platform icon:

   ```bash
   cd app && npx tauri icon ../assets/brand/zeidex-mark.png --output ../src-tauri/icons
   ```

4. Commit the changed files under `src-tauri/icons/`.
