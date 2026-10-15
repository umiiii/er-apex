"""The kill-streak badge's three pictures (hud/apex.rs `streak_badge`: D1, M1, P1) from the local
ui.rpak: Apex's Diamond, Master and Apex Predator rank emblems (`rui/menu/ranked/rank_emblem_*_flat`,
the names the local game data gives them). The game keeps each emblem's right half and draws it
mirrored; here the two halves are joined into one picture. Written to `<pack>/rank/D1.png` etc.; your own pictures in
`apex-data/hud/rank/` are used instead when they are there (hud/mod.rs).

Run from the repository root after tools/apexhud/export_hud.py: python tools/apexhud/export_rank.py
"""
import argparse
import csv
from pathlib import Path
import shutil
import sys
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import gamedirs  # noqa: E402

sys.dont_write_bytecode = True

from export_extra import string_to_guid, ensure_tool, run_rsx, read_json, LEGENDS, output_path, Image  # noqa: E402

# the badge's picture: the emblem
EMBLEMS = {'D1': 'rui/menu/ranked/rank_emblem_diamond_flat',
           'M1': 'rui/menu/ranked/rank_emblem_master_flat',
           'P1': 'rui/menu/ranked/rank_emblem_apex_flat'}


def export(game, out):
    rows = {}
    with (out / 'lists/images.csv').open(encoding='utf8', errors='replace', newline='') as f:
        for r in csv.DictReader(f):
            rows[int(r['guid'], 16)] = r
    wanted = {}
    for badge, name in EMBLEMS.items():
        guid = string_to_guid('ui_image/' + name + '.rpak')
        row = rows.get(guid)
        if not row or row['type'] != 'uiia' or row['file_name'] != 'ui.rpak':
            raise RuntimeError(f'{name}: GUID {guid:016x} not in the local ui.rpak')
        wanted[badge] = (f'{guid:016x}', row['asset_name'])
    raw = out / 'raw/images_rank'
    if raw.exists():
        shutil.rmtree(raw)
    # run_rsx records its runs in run_manifest.json: keep the pack's and file this run separately
    manifest = out / 'run_manifest.json'
    kept = manifest.read_bytes() if manifest.exists() else None
    try:
        names = ','.join(sorted(a for _, a in wanted.values()))
        run_rsx(ensure_tool(), game, out, 'images_rank', 'raw/images_rank', 'uiia', ['--exportexact', names], packages=['ui.rpak'])
    finally:
        if manifest.exists():
            shutil.move(manifest, out / 'rank_run_manifest.json')
        if kept is not None:
            manifest.write_bytes(kept)
    meta_by_guid = {read_json(p)['guid']: p for p in raw.rglob('*.meta.json')}
    target_dir = out / 'rank'
    target_dir.mkdir(parents=True, exist_ok=True)
    for badge, (guid, _) in wanted.items():
        meta = meta_by_guid.get(guid)
        source = meta.with_name(meta.name.removesuffix('.meta.json') + '.png') if meta else None
        if source is None or not source.exists():
            raise RuntimeError(f'{EMBLEMS[badge]}: RSX gave no PNG')
        # its mirror image, then the half (the right one: the centre line is its left edge), trimmed to what is drawn (the HUD fits the picture's
        # own size into the badge's box)
        with Image.open(source) as im:
            half = im.convert('RGBA')
        whole = Image.new('RGBA', (half.width * 2, half.height), (0, 0, 0, 0))
        whole.paste(half.transpose(Image.Transpose.FLIP_LEFT_RIGHT), (0, 0))
        whole.paste(half, (half.width, 0))
        box = whole.getchannel('A').getbbox()
        (whole.crop(box) if box else whole).save(target_dir / (badge + '.png'))
        with Image.open(target_dir / (badge + '.png')) as im:
            print(f'{badge}: {EMBLEMS[badge]} {im.size[0]}x{im.size[1]}')
    print('PASS rank emblems')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--game', type=Path, help='Apex Legends install folder (default: APEX_LEGENDS_DIR)')
    parser.add_argument('--legend', choices=LEGENDS, default='octane')
    parser.add_argument('--out', type=Path)
    args = parser.parse_args()
    export(args.game or gamedirs.apex(), output_path(args.legend, args.out))


if __name__ == '__main__':
    main()
