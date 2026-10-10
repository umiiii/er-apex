"""Your own fonts for the Apex HUD's digits and English letters: each .ttf/.otf in a folder (default
apex-data/fonts) rendered into one atlas the HUD reads beside Apex's own (src/hud/font.rs).

The atlas holds printable ASCII (33..126) of every font, in the format of Apex's: one 8-bit channel
whose values the HUD turns into coverage with `clip((v - 176) * 255 / 16)`, so a glyph's coverage c
is stored as 176 + 16 c (0 where there is no ink); each cell is the glyph's ink with a 2 px margin.
`meta.json` lists the fonts (`name`: the file's stem, lower case) like Apex's `source.meta.json`.

  python tools/apexhud/custom_font.py [--fonts <folder>] [--out apex-data/hud/custom_font]

No font files are in the repository: the fonts are yours.
"""
import argparse
import json
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[2]
SIZE = 96
MARGIN = 2
WIDTH = 2048


def render(path, size=SIZE):
    font = ImageFont.truetype(str(path), size)
    glyphs = {}
    for cp in range(33, 127):
        ch = chr(cp)
        left, top, right, bottom = font.getbbox(ch)
        w, h = right - left, bottom - top
        if w <= 0 or h <= 0:
            continue
        img = Image.new('L', (w + 2 * MARGIN, h + 2 * MARGIN), 0)
        ImageDraw.Draw(img).text((MARGIN - left, MARGIN - top), ch, font=font, fill=255)
        cov = np.asarray(img, np.float32) / 255.0
        if not (cov > 0).any():
            continue
        glyphs[cp] = np.where(cov > 0, np.rint(176.0 + 16.0 * cov), 0).astype(np.uint8)
    return glyphs


def build(fonts_dir, out):
    files = sorted(p for p in Path(fonts_dir).iterdir() if p.suffix.lower() in ('.ttf', '.otf'))
    if not files:
        raise SystemExit(f'no .ttf or .otf in {fonts_dir}')
    out.mkdir(parents=True, exist_ok=True)
    rendered = [(p, render(p)) for p in files]
    # shelf packing, row by row
    x = y = row = 0
    places = []
    for i, (_, glyphs) in enumerate(rendered):
        for cp, g in glyphs.items():
            h, w = g.shape
            if x + w > WIDTH:
                x, y, row = 0, y + row + 1, 0
            places.append((i, cp, x, y, g))
            x += w + 1
            row = max(row, h)
    atlas = np.zeros((y + row + 1, WIDTH), np.uint8)
    tables = [[] for _ in rendered]
    for i, cp, gx, gy, g in places:
        h, w = g.shape
        atlas[gy:gy + h, gx:gx + w] = g
        tables[i].append([cp, 0, gx, gy, w, h])
    Image.fromarray(atlas).save(out / 'atlas.png')
    meta = dict(format='er-apex-custom-font', version=1, size=SIZE, encoding='v = 176 + 16 x coverage',
                fonts=[dict(font_index=i, name=p.stem.lower(), file=p.name, glyphs=len(t), unicode_to_texture_rect=t) for i, ((p, _), t) in enumerate(zip(rendered, tables))])
    (out / 'meta.json').write_text(json.dumps(meta, ensure_ascii=False, indent=1), encoding='utf8')
    print(f'PASS custom font atlas: {atlas.shape[1]}x{atlas.shape[0]}, ' + ', '.join(f'{f["name"]} {f["glyphs"]}' for f in meta['fonts']), flush=True)


if __name__ == '__main__':
    a = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    a.add_argument('--fonts', type=Path, default=ROOT / 'apex-data/fonts')
    a.add_argument('--out', type=Path, default=ROOT / 'apex-data/hud/custom_font')
    args = a.parse_args()
    build(args.fonts, args.out)
