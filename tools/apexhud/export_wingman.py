"""The Wingman in weapon slot 1 (in the R-301's place): its `hud_icon` and `shortprintname` (the
retail weapon settings, apex-data/export/weapon/mp_weapon_wingman.txt), added to the HUD pack's
extra images and strings (export_extra.py's; run it first). The icon from the local ui.rpak, the name
from the local localization, as export_extra.py does.

Run from the repository root: python tools/apexhud/export_wingman.py --legend octane
"""
import argparse
import csv
from pathlib import Path
import shutil
import sys
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import gamedirs  # noqa: E402

sys.dont_write_bytecode = True

from export_extra import string_to_guid, ensure_tool, run_rsx, read_json, write_json, localization, LEGENDS, output_path, Image  # noqa: E402

REPO = Path(__file__).resolve().parents[2]
# the weapons of slot 1 beyond the R-301 (whose icon and name the HUD pack has): settings, icon, name
WEAPONS = {'wingman': ('mp_weapon_wingman.txt', 'rui/weapon_icons/r5/weapon_wingman', '#WPN_WINGMAN_SHORT'),
           'r99': ('mp_weapon_r97.txt', 'rui/weapon_icons/r5/weapon_r97', '#WPN_R97_SHORT'),
           'flatline': ('mp_weapon_vinson.txt', 'rui/weapon_icons/r5/weapon_flatline', '#WPN_VINSON_SHORT'),
           'sentinel': ('mp_weapon_sentinel.txt', 'rui/weapon_icons/r5/weapon_sentinel', '#WPN_SENTINEL_SHORT'),
           # the kunai (the kill feed's icon for a swing): its melee skin's `equipImage` (item flavour
           # settings/itemflav/melee_skin/kunai.rpak; its weapon settings' hud_icon is the generic fist)
           'kunai': ('melee_wraith_kunai.txt', 'rui/menu/buttons/melee_skins/wraith_kunai', None),
           # Pathfinder's grapple (Q's other ability): its item flavour's icon
           'grapple': ('mp_ability_grapple.txt', 'rui/hud/tactical_icons/tactical_pathfinder', None)}
# icons that are not their weapon settings' hud_icon: where they are named
ICON_SOURCES = {'rui/menu/buttons/melee_skins/wraith_kunai': 'settings/itemflav/melee_skin/kunai.rpak equipImage',
                'rui/hud/tactical_icons/tactical_pathfinder': 'settings/itemflav/ability/pathfinder_tac_grapple.rpak icon'}
SETTINGS = REPO / 'apex-data/export/weapon/mp_weapon_wingman.txt'
ICON = 'rui/weapon_icons/r5/weapon_wingman'
NAME_KEY = '#WPN_WINGMAN_SHORT'


def settings_line(field, value):
    for i, line in enumerate(SETTINGS.read_text(encoding='utf8').splitlines(), 1):
        if f'"{field}"' in line and f'"{value}"' in line:
            return {'kind': 'retail_weapon_settings', 'source': f'{SETTINGS.relative_to(REPO).as_posix()}:{i}'}
    raise RuntimeError(f'{SETTINGS}: no "{field}" "{value}"')


def export(game, out):
    images = out / 'extra_images.json'
    strings = out / 'extra_localization.json'
    if not images.exists() or not strings.exists():
        raise FileNotFoundError('run tools/apexhud/export_extra.py for this legend first')
    rows = {}
    with (out / 'lists/images.csv').open(encoding='utf8', errors='replace', newline='') as f:
        for r in csv.DictReader(f):
            rows[int(r['guid'], 16)] = r
    guid = string_to_guid('ui_image/' + ICON + '.rpak')
    row = rows.get(guid)
    if not row or row['type'] != 'uiia' or row['file_name'] != 'ui.rpak':
        raise RuntimeError(f'{ICON}: GUID {guid:016x} not in the local ui.rpak')
    raw = out / ('raw/images_' + Path(ICON).name)
    if raw.exists():
        shutil.rmtree(raw)
    manifest = out / 'run_manifest.json'
    kept = manifest.read_bytes() if manifest.exists() else None
    try:
        run_rsx(ensure_tool(), game, out, 'images_' + Path(ICON).name, 'raw/images_' + Path(ICON).name, 'uiia', ['--exportexact', row['asset_name']], packages=['ui.rpak'])
    finally:
        if manifest.exists():
            shutil.move(manifest, out / (Path(ICON).name + '_run_manifest.json'))
        if kept is not None:
            manifest.write_bytes(kept)
    meta = next((p for p in raw.rglob('*.meta.json') if read_json(p)['guid'] == f'{guid:016x}'), None)
    source = meta.with_name(meta.name.removesuffix('.meta.json') + '.png') if meta else None
    if source is None or not source.exists():
        raise RuntimeError(f'{ICON}: RSX gave no PNG')
    target = out / 'extra' / (ICON + '.png')
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, target)
    with Image.open(target) as im:
        im.load()
        size = list(im.size)
    index = read_json(images)
    index[ICON] = {'guid': f'{guid:016x}', 'payload_image': ('extra/' + ICON + '.png'), 'size': size,
                   'evidence': [{'kind': 'item_flavor', 'source': ICON_SOURCES[ICON]} if ICON in ICON_SOURCES else settings_line('hud_icon', ICON)], 'package': 'ui.rpak'}
    write_json(images, index)
    if NAME_KEY is None:
        print(f'PASS HUD icon: {ICON} {size[0]}x{size[1]}')
        return
    entries = localization(out, keys=[NAME_KEY])
    if not entries[NAME_KEY]['values']:
        raise RuntimeError(f'{NAME_KEY}: not in the local localization')
    texts = read_json(strings)
    texts[NAME_KEY] = {'key_guid': entries[NAME_KEY]['key_guid'], 'values': entries[NAME_KEY]['values'],
                       'evidence': [settings_line('shortprintname', NAME_KEY)]}
    write_json(strings, texts)
    print(f'PASS slot 1 HUD: {ICON} {size[0]}x{size[1]}; {NAME_KEY} = {entries[NAME_KEY]["values"].get("english")!r}')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--game', type=Path, help='Apex Legends install folder (default: APEX_LEGENDS_DIR)')
    parser.add_argument('--legend', choices=LEGENDS, default='octane')
    parser.add_argument('--out', type=Path)
    parser.add_argument('--weapon', choices=tuple(WEAPONS), default='wingman')
    args = parser.parse_args()
    global SETTINGS, ICON, NAME_KEY
    settings, ICON, NAME_KEY = WEAPONS[args.weapon]
    SETTINGS = REPO / 'apex-data/export/weapon' / settings
    export(args.game or gamedirs.apex(), output_path(args.legend, args.out))


if __name__ == '__main__':
    main()
