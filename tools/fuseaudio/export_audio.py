"""Export evidence-backed local R-301, Octane or Charge Rifle sources and a playback manifest.

Run from any directory: python tools/fuseaudio/export_audio.py --set r301|octane|defender|frag [--root CHECKOUT]
Requires T001 inventories + patched RSX executable, and numpy. No game launch.
"""
import sys

# An export must not leave import caches beside read-only inputs or the tools.
sys.dont_write_bytecode = True

import argparse
import csv
import gzip
import json
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import gamedirs  # noqa: E402  (tools/gamedirs.py: the games' install folders)
import s3record  # noqa: E402  (tools/s3record.py: S3's evidence without R5Reloaded)
import re
import struct
import subprocess
import time

import numpy as np

from miles import Bank

REPO = Path(__file__).resolve().parents[2]
OUT = REPO / 'apex-data/audio'
OCTANE_OUT = OUT / 'octane'
# U3: the Charge Rifle (S3 mp_weapon_defender) and the weapon switch's sounds
DEFENDER_OUT = OUT / 'defender'
# U9: the frag grenade's own folder (src/audio.rs loads it next to octane/)
FRAG_OUT = OUT / 'frag_grenade'
# the Wingman in the R-301's place
WINGMAN_OUT = OUT / 'wingman'
R99_OUT = OUT / 'r99'
KUNAI_OUT = OUT / 'kunai'
FLATLINE_OUT = OUT / 'flatline'
SENTINEL_OUT = OUT / 'sentinel'
HITS_OUT = OUT / 'hits'
GRAPPLE_OUT = OUT / 'grapple'
R5_SCRIPTS = s3record.R5_ROOT / 'platform/scripts'  # S3's scripts, as tools/s3_evidence.json recorded them
GAME = gamedirs.apex()
RSX = REPO / 'tools/apexassets/rsx_source/bin/Release_NoGui/rsx.exe'
DEFAULT_MATRIX = [[1, 0, 0.70710678, 0, 0.70710678, 0],
                  [0, 1, 0.70710678, 0, 0, 0.70710678]]


def dump(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2, allow_nan=False), encoding='utf8')


def read_wave(path, *, include_loop_data=False):
    data = path.read_bytes()
    if len(data) < 12 or data[:4] != b'RIFF' or data[8:12] != b'WAVE' or len(data) != struct.unpack_from('<I', data, 4)[0] + 8:
        raise ValueError(f'Invalid RIFF header/length: {path}')
    cursor, fmt, payload, loops = 12, None, None, []
    has_smpl = False
    while cursor + 8 <= len(data):
        kind, size = struct.unpack_from('<4sI', data, cursor)
        start = cursor + 8
        if start + size > len(data):
            raise ValueError(f'Truncated WAV chunk: {path}')
        if kind == b'fmt ':
            if size < 16 or fmt is not None:
                raise ValueError(f'Invalid/duplicate WAV format chunk: {path}')
            fmt = struct.unpack_from('<HHIIHH', data, start)
        elif kind == b'data':
            if payload is not None:
                raise ValueError(f'Duplicate WAV data chunk: {path}')
            payload = data[start:start + size]
        elif kind == b'smpl' and include_loop_data:
            if size < 36 or has_smpl:
                raise ValueError(f'Invalid/duplicate WAV sampler chunk: {path}')
            has_smpl = True
            count = struct.unpack_from('<I', data, start + 28)[0]
            if 36 + count * 24 > size:
                raise ValueError(f'Truncated WAV sampler loops: {path}')
            for ordinal in range(count):
                values = struct.unpack_from('<6I', data, start + 36 + ordinal * 24)
                loops.append(dict(zip(('id', 'type', 'start_frame', 'end_frame_inclusive',
                                       'fraction', 'play_count'), values)))
        cursor = start + size + (size & 1)
    if cursor != len(data) or fmt is None or payload is None:
        raise ValueError(f'Missing WAV chunks: {path}')
    encoding, channels, rate, byte_rate, align, bits = fmt
    if encoding != 3 or bits != 32 or channels not in (1, 2, 6) or rate == 0:
        raise ValueError(f'Unsupported RSX WAV format: {fmt}')
    if align != channels * 4 or byte_rate != rate * align or not payload or len(payload) % align:
        raise ValueError(f'Invalid WAV frame alignment: {path}')
    samples = np.frombuffer(payload, dtype='<f4').reshape(-1, channels)
    if not np.isfinite(samples).all():
        raise ValueError(f'Non-finite WAV: {path}')
    info = {'channels': channels, 'sample_rate': rate, 'frames': len(samples),
            'seconds': len(samples) / rate, 'encoding': 'IEEE float32',
            'peak': float(np.abs(samples).max()), 'rms': float(np.sqrt(np.mean(samples.astype(np.float64) ** 2))),
            'bytes': len(data)}
    if include_loop_data:
        for loop in loops:
            if not 0 <= loop['start_frame'] <= loop['end_frame_inclusive'] < len(samples):
                raise ValueError(f'WAV sampler loop outside sample frames: {path}')
        info['wav_loop_data'] = {'smpl_chunk_present': has_smpl, 'loops': loops}
    return samples, info


def write_wave(path, samples, rate):
    payload = np.asarray(samples, dtype='<f4').tobytes()
    channels = samples.shape[1]
    fmt = struct.pack('<HHIIHH', 3, channels, rate, rate * channels * 4, channels * 4, 32)
    body = b'WAVEfmt ' + struct.pack('<I', len(fmt)) + fmt + b'data' + struct.pack('<I', len(payload)) + payload
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(b'RIFF' + struct.pack('<I', len(body)) + body)


def references():
    weapon = next(w for w in json.loads((REPO / 'apex-data/fuse_data.json').read_text(encoding='utf8'))['weapons']
                  if w['classname'] == 'mp_weapon_rspn101')
    refs = []
    # These logical names are API choices, not invented game names.
    fields = {
        'fire_1p': 'burst_or_looping_fire_sound_start_1p',
        'fire_loop_1p': 'burst_or_looping_fire_sound_middle_1p',
        'fire_tail_1p': 'burst_or_looping_fire_sound_end_1p',
        'fire_first_3p': 'burst_or_looping_fire_sound_start_3p',
        'fire_loop_3p': 'burst_or_looping_fire_sound_middle_3p',
        'fire_tail': 'burst_or_looping_fire_sound_end_3p',
        'fire_loop_npc': 'burst_or_looping_fire_sound_middle_npc',
        'fire_secondary_1p': 'fire_sound_2_player_1p',
        'fire_secondary_3p': 'fire_sound_2_player_3p',
        'fire_secondary_npc': 'fire_sound_2_npc',
        'low_ammo': 'low_ammo_sound_name_1',
        'dry_fire': 'sound_dryfire', 'ads_in': 'sound_zoom_in', 'ads_out': 'sound_zoom_out'}
    for logical, field in fields.items():
        refs.append({'logical': logical, 'field': f'weapons[mp_weapon_rspn101].all_params.{field}',
                     'input': 'apex-data/fuse_data.json', 'event': weapon['all_params'].get(field, '')})
    # Preserve scope: altfire is an actual weapon modifier, not default auto mode.
    raw = (REPO / 'apex-data/export/weapon/mp_weapon_rspn101.txt').read_text(encoding='utf8')
    alt = re.search(r'"altfire"\s*\{([^{}]+)\}', raw).group(1)
    for logical, field in [('fire_single_1p', 'fire_sound_2_player_1p'), ('fire_3p', 'fire_sound_2_player_3p')]:
        event = re.search(r'"' + field + r'"\s*"([^"]*)"', alt).group(1)
        refs.append({'logical': logical, 'field': f'Mods.altfire.{field}',
                     'input': 'apex-data/export/weapon/mp_weapon_rspn101.txt', 'event': event})
    qc = json.loads((REPO / 'apex-data/assets/qc_metadata.json').read_text(encoding='utf8'))
    names = {'MagPull': 'reload_magout', 'MagGrab': 'reload_maggrab', 'MagInsert': 'reload_magin',
             'BoltBack': 'reload_boltback', 'BoltForward': 'reload_boltforward', 'HandRest': 'reload_handrest'}
    for file in qc['files']:
        for sequence in file['sequences']:
            if sequence['name'] != 'mp_pt_medium_reload_rspn101':
                continue
            for frame, event in re.findall(r'event "AE_CL_PLAYSOUND" (\d+) "([^"]+)"', sequence['raw_qc']):
                logical = names.get(event.rsplit('_', 1)[-1])
                if event.startswith('Weapon_RE45'):
                    logical = 'reload_magin_re45'
                if logical is None:
                    raise ValueError(f'Unclassified QC event {event}')
                refs.append({'logical': logical, 'field': f'mp_pt_medium_reload_rspn101.AE_CL_PLAYSOUND@{frame}',
                             'input': f'apex-data/assets/{file["file"]}', 'qc_frame': int(frame), 'qc_fps': 30,
                             'event': event})
    return refs


def octane_references():
    """Check the task's event names against actual local weapon/script references."""
    heal = R5_SCRIPTS / 'weapons/mp_ability_heal.txt'
    stim = REPO / 'apex-data/export/weapon/mp_ability_octane_stim.txt'
    pad_s3 = R5_SCRIPTS / 'weapons/mp_weapon_jump_pad.txt'
    pad = REPO / 'apex-data/export/weapon/mp_weapon_jump_pad.txt'
    stim_script = R5_SCRIPTS / 'vscripts/weapons/mp_ability_heal.nut'
    pad_script = R5_SCRIPTS / 'vscripts/mp/_jump_pads.gnut'
    pad_client = R5_SCRIPTS / 'vscripts/mp/cl_jump_pads.gnut'
    callbacks = R5_SCRIPTS / 'vscripts/mp/_codecallbacks.gnut'
    # the shield battery (D-032): S3's consumable weapon (mod shield_large) and its script
    consumable_s3 = R5_SCRIPTS / 'weapons/mp_ability_consumable.txt'
    consumable = REPO / 'apex-data/export/weapon/mp_ability_consumable.txt'
    consumable_script = R5_SCRIPTS / 'vscripts/weapons/mp_ability_consumable.nut'
    # T021's record of the battery's QC (ptpov_shield_battery_held): its sequences' sound events
    battery_qc = REPO / 'apex-data/assets/battery/ability_sequences.json'
    # T020's record of the injector's QC (ptpov_octane_epipen_held): its sequences' sound events
    injector = REPO / 'apex-data/assets/octane/ability_sequences.json'
    # Battle chatter: scripts name the line ("bc_tactical"); the event is
    # "diag_mp_" + voice + "_" + line + "_" + perspective (cl_survival_commentary.gnut
    # GetBattleChatterAlias1P3P), the voice "octane". Its sources are in the language streams.
    chatter = {'diag_mp_octane_bc_tactical_1p': 'bc_tactical'}
    specs = [
        ('octane_stimpack_activate_1P', 'stim starts',
         [(heal, 'charge_sound_1p'), (stim, 'charge_sound_1p')]),
        ('octane_stimpack_loop_1P', 'stim lasts', [(stim_script, 'EmitWeaponSound_1p3p')]),
        ('octane_stimpack_deactivate_1P', '2 s before stim stops', [(stim_script, 'EmitWeaponSound_1p3p')]),
        ('survival_ui_tactical_ready', 'tactical ready',
         [(heal, 'sound_weapon_ready'), (stim, 'sound_weapon_ready')]),
        ('Survival_UI_Ability_NotReady', 'ability not ready',
         [(heal, 'sound_dryfire'), (stim, 'sound_dryfire'),
          (pad_s3, 'sound_dryfire'), (pad, 'sound_dryfire')]),
        ('JumpPad_Throw', 'pad toss', [(pad_s3, 'sound_throw_1p'), (pad, 'sound_throw_1p')]),
        ('JumpPad_Deploy_Unpack', 'pad opens on ground', [(pad_script, 'EmitSoundOnEntity')]),
        ('JumpPad_LaunchPlayer_1p', 'pad launches player', [(pad_client, 'EmitSoundOnEntity')]),
        ('JumpPad_Ascent_Windrush', 'launch flight',
         [(pad_client, 'EmitSoundOnEntity'), (pad_script, 'EmitSoundOnEntityExceptToPlayer')]),
        ('JumpPad_DoubleJump_1P', 'double jump', [(pad_script, 'EmitSoundOnEntityOnlyToPlayer')]),
        ('survival_ui_ultimate_ready', 'ultimate ready',
         [(pad_s3, 'sound_weapon_ready'), (pad, 'sound_weapon_ready')]),
        # 2026-10-05 user's R5R video: the stim's voice line and the knock-down sound.
        ('diag_mp_octane_bc_tactical_1p', 'stim voice line (battle chatter, 20-40 s apart)',
         [(stim_script, 'PlayBattleChatterLineToSpeakerAndTeam')]),
        ('flesh_bulletimpact_downedshot_1p_vs_3p', 'killing shot on a non-player, to the attacker',
         [(callbacks, 'EmitSoundOnEntityOnlyToPlayer')]),
        # 2026-10-05 user: the shield break, to the attacker when a shield runs out (the mod plays it
        # when a boss falls below 70 % health: the MVP has no shields)
        ('humanshield_break_1p_vs_3p', 'shield broken, to the attacker',
         [(callbacks, 'EmitSoundOnEntityOnlyToPlayer')]),
        # the shield battery (D-032): the charge (after the 1 s raise), the fire's two sounds (the
        # shield filled), the cancel (OnWeaponDeactivate_Consumable: cancelSoundName)
        ('Shield_Battery_Charge', 'shield battery charging',
         [(consumable_s3, 'charge_sound_1p'), (consumable, 'charge_sound_1p'), (consumable_script, 'chargeSoundName')]),
        ('Shield_Battery_Primary', 'shield battery used (shield filled)',
         [(consumable_s3, 'fire_sound_1_player_1p'), (consumable, 'fire_sound_1_player_1p')]),
        ('Shield_Battery_Holster', 'shield battery put away after use',
         [(consumable_s3, 'fire_sound_2_player_1p'), (consumable, 'fire_sound_2_player_1p')]),
        ('Shield_Battery_Failure', 'shield battery use cancelled',
         [(consumable_script, 'cancelSoundName')]),
        # the battery's view model QC: raise frames 0 and 25, charge frame 10
        ('ShieldBattery_DrawFoley_fr02', 'shield battery out (raise frame 0)', [(battery_qc, 'raise')]),
        ('ShieldBattery_DrawFoley_fr27', 'shield battery out (raise frame 25)', [(battery_qc, 'raise')]),
        ('ShieldBattery_DrawFoley_fr49', 'shield battery charging (charge frame 10)', [(battery_qc, 'charge')]),
        # The injector's throw: each flourish (holster_*) plays its own release at frame 0. The user's
        # R5R video has Release_Throw's third source at 30.94 s, as it goes. The QC's common
        # Octane_Stim_Release_1P (frame 0 too) is not taken: its sources are the deactivate event's
        # canister layer, played 0.27 s before, and the video does not tell a second one apart.
        ('Octane_Stim_Release_BackToss_1P', 'injector thrown: backToss', [(injector, 'holster_backToss')]),
        ('Octane_Stim_Release_MicDrop_1P', 'injector thrown: micDrop', [(injector, 'holster_micDrop')]),
        ('Octane_Stim_Release_Rocker_1P', 'injector thrown: rocker', [(injector, 'holster_rocker')]),
        ('Octane_Stim_Release_Spin_1P', 'injector thrown: spin', [(injector, 'holster_spin')]),
        ('Octane_Stim_Release_Throw_1P', 'injector thrown: throwAway', [(injector, 'holster_throwAway')]),
        ('Octane_Stim_Release_Throw_Akimbo_1P', 'injector thrown: throwAway_fast',
         [(injector, 'holster_throwAway_fast')]),
    ]
    return _refs_from_specs(specs, chatter)


def _evidence(path, field, literal, requested, read):
    """read(file, input_path, field, literal, requested) on a local file; for an R5Reloaded one, S3's
    record (tools/s3record.py: no R5Reloaded copy needed)."""
    input_path = path.relative_to(REPO).as_posix() if path.is_relative_to(REPO) else path.as_posix()
    if s3record.under(path):
        return s3record.get('audio', f'{input_path}|{field}|{literal}|{requested}',
                            lambda live: read(s3record.real(live, path), input_path, field, literal, requested))
    return read(path, input_path, field, literal, requested)


def _spec_evidence(path, input_path, field, literal, requested):
    """The lines of one input that name the event (_refs_from_specs): weapon settings, scripts, QC."""
    evidence = []
    if not path.is_file():
        return evidence
    text = path.read_text(encoding='utf8')
    if path.suffix == '.json':
        # a QC sound event: the field is the sequence, its raw QC plays the event on a frame
        head = '$sequence "' + field + '"'
        for model in json.loads(text)['models']:
            for sequence in model['sequences']:
                for qc in sequence.get('qc', []):
                    raw = qc.get('raw_qc', '')
                    if not raw.startswith(head):
                        continue
                    at = text.find(json.dumps(head)[1:-1])
                    for frame, event in re.findall(r'"AE_CL_PLAYSOUND"\s+(\d+)\s+"([^"]+)"', raw):
                        if event.lower() == literal.lower():
                            evidence.append({'input': input_path, 'field': f'{field}.AE_CL_PLAYSOUND@{frame}',
                                             'line': text.count('\n', 0, at) + 1 if at >= 0 else 0,
                                             'event': event, 'qc_frame': int(frame)})
        return evidence
    if path.suffix == '.qc':
        # a QC sound event in a raw QC: the field is the sequence
        head = '$sequence "' + field + '"'
        at = text.find(head)
        if at < 0:
            return evidence
        end = text.find('\n}', at)
        block = text[at:end if end >= 0 else len(text)]
        for m in re.finditer(r'"AE_CL_PLAYSOUND"\s+(\d+)\s+"([^"]+)"', block):
            if m.group(2).lower() == literal.lower():
                evidence.append({'input': input_path, 'field': f'{field}.AE_CL_PLAYSOUND@{m.group(1)}',
                                 'line': text.count('\n', 0, at + m.start()) + 1,
                                 'event': m.group(2), 'qc_frame': int(m.group(1))})
        return evidence
    if path.suffix == '.txt' and '.' in field:
        # an impact table: the material's entry in the named block (as frag_references reads them)
        block_name, key = field.split('.', 1)
        for block in re.finditer(r'^\s*' + re.escape(block_name) + r'\s*\{(.*?)\}', text, re.M | re.S):
            for m in re.finditer(r'"' + re.escape(key) + r'"\s+"([^"]+)"', block.group(1)):
                if m.group(1).lower() == literal.lower():
                    at = block.start(1) + m.start()
                    evidence.append({'input': input_path, 'field': field, 'line': text.count('\n', 0, at) + 1,
                                     'event': m.group(1)})
        return evidence
    if path.suffix == '.txt':
        pattern = r'"' + re.escape(field) + r'"\s*"([^"\r\n]*)"'
    else:
        # a call naming the event, or an assignment (`info.cancelSoundName = "..."`)
        pattern = r'\b' + re.escape(field) + r'\s*(?:\([^;\r\n]*?|=\s*)"(' + re.escape(literal) + r')"'
    for match in re.finditer(pattern, text, re.IGNORECASE):
        if match.group(1).lower() != literal.lower():
            continue
        evidence.append({'input': input_path, 'field': field,
                         'line': text.count('\n', 0, match.start()) + 1,
                         'event': requested if literal != requested else match.group(1)})
        if literal != requested:
            evidence[-1]['chatter_line'] = match.group(1)
    return evidence


def _refs_from_specs(specs, chatter=None):
    """Each requested event with the local lines that name it (weapon settings, scripts, QC)."""
    chatter = chatter or {}
    refs = []
    for requested, use, inputs in specs:
        # what the script literally names: the event, or a battle chatter line
        literal = chatter.get(requested, requested)
        evidence = []
        for path, field in inputs:
            evidence += _evidence(path, field, literal, requested, _spec_evidence)
        refs.append({'logical': requested.lower(), 'requested_event': requested, 'use': use,
                     'input': evidence[0]['input'] if evidence else str(inputs[0][0]),
                     'field': evidence[0]['field'] if evidence else inputs[0][1],
                     'event': evidence[0]['event'] if evidence else '', 'evidence': evidence})
    return refs


def defender_references():
    """U3: the Charge Rifle's sounds as S3 names them (R5Reloaded weapon settings and script), the
    view models' QC sounds (the local retail QCs: tools/apexpov/export_defender_pov.py, T016), and
    the R-301's draw and holster for the weapon switch."""
    defender = R5_SCRIPTS / 'weapons/mp_weapon_defender.txt'
    sniper = R5_SCRIPTS / 'weapons/_base_sniper.txt'
    script = R5_SCRIPTS / 'vscripts/weapons/mp_weapon_defender.nut'
    retail = REPO / 'apex-data/export/weapon/mp_weapon_defender.txt'
    qc = REPO / ('apex-data/weapons/defender/pov/smd/animrig/techart/mshop/weapons/class/sniper/'
                 'chargerifle/chargerifle_base_v_animRig.qc')
    r301_qc = REPO / 'apex-data/pov/smd/animrig/weapons/rspn101/ptpov_rspn101.qc'
    # the old beam version the mod uses since 2026-10-05 23:00 (user): its settings name these sounds too
    sustained = R5_SCRIPTS / 'weapons/mp_weapon_defender_sustained.txt'
    impacts = R5_SCRIPTS / 'impacts'
    specs = [
        ('Weapon_ChargeRifle_Fire_1P', 'a shot',
         [(sustained, 'fire_sound_1_player_1p'), (defender, 'fire_sound_1_player_1p'), (retail, 'fire_sound_1_player_1p')]),
        ('Weapon_ChargeRifle_WindUp_1P', 'charging (stops when full)',
         [(sustained, 'charge_sound_1p'), (defender, 'charge_sound_1p'), (retail, 'charge_sound_1p')]),
        ('Weapon_ChargeRifle_WindDown_1P', 'the charge draining (stops when empty)',
         [(sustained, 'charge_drain_sound_1p'), (defender, 'charge_drain_sound_1p'), (retail, 'charge_drain_sound_1p')]),
        ('Weapon_ChargeRifle_TriggerOn', 'a discharge starts (sound_trigger_pull); a charge from empty (OnWeaponChargeBegin)',
         [(sustained, 'sound_trigger_pull'), (script, 'EmitWeaponSound_1p3p')]),
        ('Weapon_ChargeRifle_FastIdle_MechFwdBack_1P', 'the sustained discharge (QC sustained_discharge frames 0, 24, 52, 78)',
         [(qc, 'sustained_discharge')]),
        ('ChargeRifle_SmallBeam_Bulletimpact_1p_vs_3p', 'a laser pulse on a surface, to the shooter (exp_defender_small Sound_attacker C)',
         [(impacts / 'exp_defender_small.txt', 'Sound_attacker.C')]),
        ('flesh_bulletimpact_chargerifle_beam_1p_vs_3p', 'a laser pulse on flesh, to the shooter (exp_defender_small Sound_attacker F)',
         [(impacts / 'exp_defender_small.txt', 'Sound_attacker.F')]),
        ('ChargeRifle_FullShot_Bulletimpact_1p_vs_3p', 'the shot on a surface, to the shooter (exp_defender Sound_attacker C)',
         [(impacts / 'exp_defender.txt', 'Sound_attacker.C')]),
        ('flesh_bulletimpact_chargerifle_shot_1p_vs_3p', 'the shot on flesh, to the shooter (exp_defender Sound_attacker F)',
         [(impacts / 'exp_defender.txt', 'Sound_attacker.F')]),
        ('weapon_chargerifle_chargeupclick_1p', 'a charge part way (OnWeaponChargeBegin)', [(script, 'EmitWeaponSound_1p3p')]),
        ('Weapon_ChargeRifle_TriggerOff', 'the trigger let go',
         [(sustained, 'sound_trigger_release'), (defender, 'sound_trigger_release'), (retail, 'sound_trigger_release')]),
        ('Weapon_ChargeRifle_ADS_In', 'aim in', [(sustained, 'sound_zoom_in'), (defender, 'sound_zoom_in'), (retail, 'sound_zoom_in')]),
        ('Weapon_ChargeRifle_ADS_Out', 'aim out', [(sustained, 'sound_zoom_out'), (defender, 'sound_zoom_out'), (retail, 'sound_zoom_out')]),
        ('rifle_dryfire', 'an empty trigger', [(sniper, 'sound_dryfire')]),
        ('Weapon_ChargeRifle_Equip', 'drawn', [(qc, 'draw'), (qc, 'drawfirst')]),
        ('Weapon_ChargeRifle_UnEquip', 'put away', [(qc, 'holster')]),
        ('weapon_chargerifle_firstdraw_1p', 'drawn the first time', [(qc, 'drawfirst')]),
        ('Weapon_R101_Equip', 'the R-301 drawn', [(r301_qc, 'draw')]),
        ('Weapon_R101_UnEquip', 'the R-301 put away', [(r301_qc, 'holster')]),
    ]
    for event in ['Wpn_ChargeRifle_1P_Reload_ArmLift_FR04', 'Wpn_ChargeRifle_1P_Reload_EjectMag_FR13',
                  'Wpn_ChargeRifle_1P_Reload_SteamRelease_FR18', 'Wpn_ChargeRifle_1P_Reload_ArmLift_FR41',
                  'Wpn_ChargeRifle_1P_Reload_InsertMag_FR51', 'Wpn_ChargeRifle_1P_Reload_TwistLever_FR61',
                  'Wpn_ChargeRifle_1P_Reload_PullOut_FR77', 'Wpn_ChargeRifle_1P_Reload_ArmLift_FR93',
                  'Wpn_ChargeRifle_1P_Reload_SlapClosed_FR105', 'Wpn_ChargeRifle_1P_Reload_HandSettle_FR121']:
        specs.append((event, 'reload (QC frame)', [(qc, 'reload'), (qc, 'reload_empty')]))
    for event in ['Wpn_ChargeRifle_1P_ReloadEmpty_SwitchFlip_FR100', 'Wpn_ChargeRifle_1P_ReloadEmpty_SpinUp_FR113',
                  'Wpn_ChargeRifle_1P_ReloadEmpty_SpinUp_FR136_Pt2']:
        specs.append((event, 'empty reload (QC frame)', [(qc, 'reload_empty')]))
    return _refs_from_specs(specs)


def wingman_references():
    """The Wingman in the R-301's place: its retail weapon settings' sounds and its view model's QC
    sounds (tools/apexassets/wingman_assets.py: the sequences' raw QC)."""
    retail = REPO / 'apex-data/export/weapon/mp_weapon_wingman.txt'
    qc = REPO / 'apex-data/assets/wingman/ability_sequences.json'
    specs = [
        ('Weapon_Wingman_Fire_1P', 'a shot', [(retail, 'fire_sound_2_player_1p')]),
        ('Weapon_Wingman_ADS_In', 'aim in', [(retail, 'sound_zoom_in')]),
        ('Weapon_Wingman_ADS_Out', 'aim out', [(retail, 'sound_zoom_out')]),
        ('pistol_dryfire', 'an empty trigger', [(retail, 'sound_dryfire')]),
        ('Weapon_Wingman_Equip', 'drawn (draw frame 0)', [(qc, 'draw')]),
        ('weapon_wingman_firstpullout', 'drawn the first time (drawfirst frame 5)', [(qc, 'drawfirst')]),
        ('Weapon_Wingman_UnEquip', 'put away (holster frame 0)', [(qc, 'holster')]),
    ]
    for event in ['Wpn_Wingman_Reload_Open', 'Wpn_Wingman_Reload_Eject', 'Wpn_Wingman_Reload_InsertMag',
                  'Wpn_Wingman_Reload_Close', 'Wpn_Wingman_Reload_HandGrab']:
        specs.append((event, 'reload (QC frame)', [(qc, 'reload'), (qc, 'reload_empty')]))
    # the inspect key: the Wingman's `inspect` and the Charge Rifle's `inspect_basic` (T022's QC record)
    for event in ['weapon_wingman_inspect_part01', 'weapon_wingman_inspect_part02', 'weapon_wingman_inspect_part03',
                  'weapon_wingman_inspect_part04', 'weapon_wingman_inspect_end']:
        specs.append((event, 'inspect (QC frame)', [(qc, 'inspect')]))
    defender_qc = REPO / 'apex-data/assets/defender/ability_sequences.json'
    for event in ['Weapon_Inspect_Sniper_Start', 'Weapon_Inspect_Sniper_Mid', 'Weapon_Inspect_Sniper_End']:
        specs.append((event, 'the Charge Rifle\'s inspect (QC frame)', [(defender_qc, 'inspect_basic')]))
    return _refs_from_specs(specs)


def r99_references():
    """The R-99 in slot 1 (the weapon wheel): its retail weapon settings' sounds and its view model's
    QC sounds (tools/apexassets/r99_assets.py)."""
    retail = REPO / 'apex-data/export/weapon/mp_weapon_r97.txt'
    qc = REPO / 'apex-data/assets/r99/ability_sequences.json'
    specs = [
        ('Weapon_R97_Fire_First_1P', 'a burst starts', [(retail, 'burst_or_looping_fire_sound_start_1p')]),
        ('Weapon_R97_Fire_Loop_1P', 'a burst goes on', [(retail, 'burst_or_looping_fire_sound_middle_1p')]),
        ('Weapon_R97_Fire_Last_1P', 'a burst ends', [(retail, 'burst_or_looping_fire_sound_end_1p')]),
        ('Weapon_R97_SecondShot_1P', 'a shot', [(retail, 'fire_sound_2_player_1p')]),
        ('Weapon_R97_ADS_In', 'aim in', [(retail, 'sound_zoom_in')]),
        ('Weapon_R97_ADS_Out', 'aim out', [(retail, 'sound_zoom_out')]),
        ('assault_rifle_dryfire', 'an empty trigger', [(retail, 'sound_dryfire')]),
        ('Weapon_R97_Equip', 'drawn (draw frame 0)', [(qc, 'draw')]),
        ('Weapon_R97_UnEquip', 'put away (holster frame 0)', [(qc, 'holster')]),
        ('Weapon_R97_Inspect', 'inspect (frame 0)', [(qc, 'inspect_new')]),
    ]
    for event in ['Wpn_R97_Reload_PullMag', 'Wpn_R97_Reload_InsertMag', 'Wpn_R97_Reload_HandGrab']:
        specs.append((event, 'reload (QC frame)', [(qc, 'reload_seq'), (qc, 'reload_empty_seq')]))
    for event in ['Wpn_R97_Reload_ChargeBack', 'Wpn_R97_Reload_ChargeForward']:
        specs.append((event, 'empty reload (QC frame)', [(qc, 'reload_empty_seq')]))
    return _refs_from_specs(specs)


def grapple_references():
    """Pathfinder's grapple (spike/grapple.rs): its fire, the hook's catch on a surface, the reel's
    loop and the retract, named here (the ability's weapon settings name only its UI sounds)."""
    events = ['pilot_grapple_fire', 'default_grapple_impact_1p_vs_3p', 'pilot_grapple_traverse_1p', 'pilot_grapple_retract_1p']
    return [{'logical': e.lower(), 'requested_event': e, 'use': 'the grapple', 'input': 'stand-in', 'field': '', 'event': e, 'evidence': []} for e in events]


def hits_references():
    """A damaging hit's sound (the user's 2026-10-09 ask: Apex's armour-break): the shield break as
    the attacker hears it (`humanshield_break_1p_vs_3p`), named here, not from a weapon's settings."""
    events = ['humanshield_break_1p_vs_3p']
    return [{'logical': e.lower(), 'requested_event': e, 'use': 'a hit that deals damage', 'input': 'stand-in', 'field': '', 'event': e, 'evidence': []} for e in events]


def sentinel_references():
    """The Sentinel in slot 1 (the weapon wheel): its amped shot (the shield-charged mod's
    `fire_sound_1_player_1p`, the user's 2026-10-09 pick), its retail settings' other sounds and its
    view model's QC sounds (tools/apexassets/sentinel_assets.py)."""
    retail = REPO / 'apex-data/export/weapon/mp_weapon_sentinel.txt'
    qc = REPO / 'apex-data/assets/sentinel/ability_sequences.json'
    specs = [
        ('weapon_sentinel_fire_alt_1p', 'a shot (amped)', [(retail, 'fire_sound_1_player_1p')]),
        ('weapon_sentinel_ads_in', 'aim in', [(retail, 'sound_zoom_in')]),
        ('weapon_sentinel_ads_out', 'aim out', [(retail, 'sound_zoom_out')]),
        ('rifle_dryfire', 'an empty trigger', [(retail, 'sound_dryfire')]),
        ('weapon_sentinel_draw', 'drawn (draw frame 0)', [(qc, 'draw')]),
        ('weapon_sentinel_drawfirst', 'drawn the first time (drawfirst frame 0)', [(qc, 'drawfirst')]),
        ('weapon_sentinel_holster', 'put away (holster frame 0)', [(qc, 'holster')]),
    ]
    for event in ['weapon_sentinel_boltback', 'weapon_sentinel_boltfront']:
        specs.append((event, 'the bolt after a shot (rechamber QC frame)', [(qc, 'rechamber')]))
    for event in ['weapon_sentinel_reload_gunup', 'weapon_sentinel_reload_magout', 'weapon_sentinel_reload_maggrab',
                  'weapon_sentinel_reload_magslot', 'weapon_sentinel_reload_maginsert', 'weapon_sentinel_reload_gundown']:
        specs.append((event, 'reload (QC frame)', [(qc, 'reload')]))
    # the amped shot's own layers (its electric crack, `Wpn_Sentinel_1P_Fire_Alt_Electrical`, and
    # the gun's report): sub-events the bank's `weapon_sentinel_fire_alt_1p` plays by the
    # interior/exterior state, which its resolve leaves out; the outdoor one, named here
    stand_ins = ['weapon_sentinel_fire_alt_1p_ExtBase', 'weapon_sentinel_fire_1p_ExtBase']
    return _refs_from_specs(specs) + [{'logical': e.lower(), 'requested_event': e, 'use': 'a shot (amped), its own layers', 'input': 'stand-in', 'field': '', 'event': e, 'evidence': []} for e in stand_ins]


def flatline_references():
    """The VK-47 Flatline in slot 1 (the weapon wheel): its retail weapon settings' sounds and its view
    model's QC sounds (tools/apexassets/flatline_assets.py)."""
    retail = REPO / 'apex-data/export/weapon/mp_weapon_vinson.txt'
    qc = REPO / 'apex-data/assets/flatline/ability_sequences.json'
    specs = [
        ('Weapon_Vinson_FirstShot_1P', 'a burst starts', [(retail, 'burst_or_looping_fire_sound_start_1p')]),
        ('Weapon_Vinson_Loop_1P', 'a burst goes on', [(retail, 'burst_or_looping_fire_sound_middle_1p')]),
        ('Weapon_Vinson_LoopEnd_1P', 'a burst ends', [(retail, 'burst_or_looping_fire_sound_end_1p')]),
        ('Weapon_Vinson_SecondShot_1P', 'a shot', [(retail, 'fire_sound_2_player_1p')]),
        ('Weapon_R101_ADS_In', 'aim in', [(retail, 'sound_zoom_in')]),
        ('Weapon_R101_ADS_Out', 'aim out', [(retail, 'sound_zoom_out')]),
        ('Weapon_Vinson_Trigger', 'an empty trigger', [(retail, 'sound_dryfire')]),
        ('Weapon_R101_Equip', 'drawn (draw frame 0)', [(qc, 'draw')]),
        ('Weapon_Vinson_FirstPullout', 'drawn the first time (drawfirst frame 6)', [(qc, 'drawfirst')]),
        ('Weapon_R101_UnEquip', 'put away (holster frame 0)', [(qc, 'holster')]),
        ('weapon_vinson_inspect_basicNew', 'inspect (frame 0)', [(qc, 'inspect_basic_new')]),
    ]
    for event in ['Weapon_Vinson_Reload_MagOut', 'Weapon_Vinson_Reload_MagIn']:
        specs.append((event, 'reload (QC frame)', [(qc, 'reload'), (qc, 'reload_empty')]))
    specs.append(('Weapon_Vinson_ReloadEmpty_Charge', 'empty reload (QC frame)', [(qc, 'reload_empty')]))
    return _refs_from_specs(specs)


def kunai_references():
    """Wraith's heirloom kunai (the holstered mode, key 3): its view model's QC sounds
    (tools/apexassets/kunai_assets.py)."""
    qc = REPO / 'apex-data/assets/kunai/ability_sequences.json'
    specs = [
        ('Wraith_Mvmt_Kunai_Grip_Reverse2Standard', 'drawn (draw frame 0)', [(qc, 'draw')]),
        ('Mvmt_Melee_Kunai_Swipe_1P', 'a swing (melee_idle_swipe frame 0)', [(qc, 'melee_idle_swipe')]),
        ('effort_melee_1p', 'a swing\'s effort (melee_idle_swipe frame 2)', [(qc, 'melee_idle_swipe')]),
    ]
    for event in ['Wraith_Mvmt_Kunai_Inspect_Basic_P1', 'Wraith_Mvmt_Kunai_Inspect_Basic_P2', 'Wraith_Mvmt_Kunai_Inspect_Basic_P3']:
        specs.append((event, 'inspect (QC frame)', [(qc, 'inspect')]))
    for event in ['Wraith_Mvmt_Kunai_Inspect_Fly_P1', 'Wraith_Mvmt_Kunai_Inspect_Fly_P2', 'Wraith_Mvmt_Kunai_Inspect_Fly_P3', 'Wraith_Mvmt_Kunai_Inspect_Fly_P4']:
        specs.append((event, 'inspect_fly (QC frame)', [(qc, 'inspect_fly')]))
    specs.append(('Wraith_Mvmt_Kunai_FirstDraw', 'the sprint twirl (drawsprint_twirl frame 0)', [(qc, 'drawsprint_twirl')]))
    for event in ['Wraith_Mvmt_Kunai_Inspect_Insignia_P1', 'Wraith_Mvmt_Kunai_Inspect_Insignia_P2', 'Wraith_Mvmt_Kunai_Inspect_Insignia_Charged', 'Wraith_Mvmt_Kunai_Inspect_Insignia_W_Appears']:
        specs.append((event, 'inspect_insignia (QC frame)', [(qc, 'inspect_insignia')]))
    refs = [r for r in _refs_from_specs(specs) if r['event']]
    # the QC's swing sounds (`Mvmt_Melee_Kunai_Swipe_1P`, `effort_melee_1p`) are not in the local
    # bank's event table: stand-ins of the same kind, named here (推断, not from the QC)
    for event, use in [('Karambit_Mvmt_Melee_Idle_Swipe_1p', 'a swing (stand-in for Mvmt_Melee_Kunai_Swipe_1P)'),
                       ('octane_effort_melee_1p', 'a swing\'s effort (Octane\'s, for effort_melee_1p)'),
                       ('Generic_KunaiImpact_1p_vs_3p', 'a swing that hits'),
                       ('Wraith_Mvmt_Kunai_Grip_Standard2Reverse', 'put away (the holster has no QC sound)')]:
        refs.append({'logical': event.lower(), 'requested_event': event, 'use': use, 'input': 'stand-in', 'field': '', 'event': event, 'evidence': []})
    return refs


def _frag_evidence(path, input_path, field, literal, requested):
    """The lines of one input that name the event (frag_references)."""
    evidence = []
    if not path.is_file():
        return evidence
    text = path.read_text(encoding='utf8', errors='replace')
    if path.suffix == '.json':
        for sequence in json.loads(text)['sequences']:
            if sequence['name'] != field:
                continue
            for block in sequence['qc']:
                for event in block['events']:
                    if event['event'] == 'AE_CL_PLAYSOUND' and (event['options'] or '').lower() == literal.lower():
                        evidence.append({'input': input_path, 'field': f'{field}.AE_CL_PLAYSOUND@{event["frame"]}',
                                         'line': 0, 'event': event['options'], 'qc_frame': event['frame']})
        return evidence
    if '.' in field:
        # an impact table: the material's entry in the named block
        block_name, key = field.split('.', 1)
        for block in re.finditer(r'^\s*' + re.escape(block_name) + r'\s*\{(.*?)\}', text, re.M | re.S):
            for m in re.finditer(r'"' + re.escape(key) + r'"\s+"([^"]+)"', block.group(1)):
                if m.group(1).lower() == literal.lower():
                    at = block.start(1) + m.start()
                    evidence.append({'input': input_path, 'field': field, 'line': text.count('\n', 0, at) + 1,
                                     'event': m.group(1)})
        return evidence
    for m in re.finditer(r'"' + re.escape(field) + r'"\s*"([^"\r\n]*)"', text, re.IGNORECASE):
        if m.group(1).lower() != literal.lower():
            continue
        evidence.append({'input': input_path, 'field': field, 'line': text.count('\n', 0, m.start()) + 1,
                         'event': requested if literal != requested else m.group(1)})
        if literal != requested:
            evidence[-1]['chatter_line'] = m.group(1)
    return evidence


def frag_references():
    """U9: the frag grenade's events, each checked against what names it locally: the S3 weapon
    settings (R5R) and retail's export, S3's impact tables (R5R platform/scripts/impacts: the
    explosion to its thrower, a bounce; the material "C", concrete and rock, as Elden Ring's
    surfaces have no Apex material: 推断) and the view model's QC (tools/apexassets/frag_grenade_assets.py)."""
    s3 = R5_SCRIPTS / 'weapons/mp_weapon_frag_grenade.txt'
    retail = REPO / 'apex-data/export/weapon/mp_weapon_frag_grenade.txt'
    impacts = R5_SCRIPTS / 'impacts'
    qc = REPO / 'apex-data/assets/frag_grenade/sequences/ptpov_frag_grenade_held.json'
    # Battle chatter: the setting names the line ("bc_frag"); the event is "diag_mp_" + voice + "_" +
    # line + "_1p" (cl_survival_commentary.gnut GetBattleChatterAlias1P3P), the voice "octane".
    chatter = {'diag_mp_octane_bc_frag_1p': 'bc_frag'}
    specs = [
        ('weapon_fraggrenade_draw_1P', 'grenade drawn (draw_seq frame 0)', [(qc, 'draw_seq')]),
        ('Weapon_FragGrenade_PinPull', 'pin pulled (OnWeaponTossPrep: sound_deploy_1p)',
         [(s3, 'sound_deploy_1p'), (retail, 'sound_deploy_1p')]),
        ('Weapon_FragGrenade_Throw', 'thrown (OnWeaponToss: sound_throw_1p)',
         [(s3, 'sound_throw_1p'), (retail, 'sound_throw_1p')]),
        ('diag_mp_octane_bc_frag_1p', 'voice on the throw (battle_chatter_event)',
         [(s3, 'battle_chatter_event'), (retail, 'battle_chatter_event')]),
        ('Phys_Imp_FragGrenade_Concrete', 'bounce (bounce_effect_table bounce_small, Sound C)',
         [(impacts / 'bounce_small.txt', 'Sound.C')]),
        ('Explo_FragGrenade_Impact_1P', 'explosion, to its thrower (impact_effect_table exp_frag_grenade, Sound_attacker C)',
         [(impacts / 'exp_frag_grenade.txt', 'Sound_attacker.C')]),
        ('Weapon_P2011_UnEquip', 'grenade put away (holster_seq frame 0)', [(qc, 'holster_seq')]),
    ]
    refs = []
    for requested, use, inputs in specs:
        literal = chatter.get(requested, requested)
        evidence = []
        for path, field in inputs:
            evidence += _evidence(path, field, literal, requested, _frag_evidence)
        refs.append({'logical': requested.lower(), 'requested_event': requested, 'use': use,
                     'input': evidence[0]['input'] if evidence else str(inputs[0][0]),
                     'field': evidence[0]['field'] if evidence else inputs[0][1],
                     'event': evidence[0]['event'] if evidence else '', 'evidence': evidence})
    return refs


def set_root(root):
    """Read apex-data/ and the RSX build of another checkout (a worktree has neither)."""
    global REPO, OUT, OCTANE_OUT, DEFENDER_OUT, FRAG_OUT, WINGMAN_OUT, R99_OUT, KUNAI_OUT, FLATLINE_OUT, SENTINEL_OUT, HITS_OUT, GRAPPLE_OUT, RSX
    REPO = Path(root).resolve()
    OUT = REPO / 'apex-data/audio'
    OCTANE_OUT = OUT / 'octane'
    DEFENDER_OUT = OUT / 'defender'
    FRAG_OUT = OUT / 'frag_grenade'
    WINGMAN_OUT = OUT / 'wingman'
    R99_OUT = OUT / 'r99'
    KUNAI_OUT = OUT / 'kunai'
    FLATLINE_OUT = OUT / 'flatline'
    SENTINEL_OUT = OUT / 'sentinel'
    HITS_OUT = OUT / 'hits'
    GRAPPLE_OUT = OUT / 'grapple'
    RSX = REPO / 'tools/apexassets/rsx_source/bin/Release_NoGui/rsx.exe'


def output_directory(set_name, requested=None):
    own = {'octane': OCTANE_OUT, 'defender': DEFENDER_OUT, 'frag': FRAG_OUT, 'wingman': WINGMAN_OUT, 'r99': R99_OUT, 'kunai': KUNAI_OUT, 'flatline': FLATLINE_OUT, 'sentinel': SENTINEL_OUT, 'hits': HITS_OUT, 'grapple': GRAPPLE_OUT}.get(set_name)
    output = (requested if requested is not None else own if own is not None else OUT).resolve()
    if own is not None and not output.is_relative_to(own.resolve()):
        raise ValueError(f'The {set_name} output directory must stay inside {own}')
    return output


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--set', choices=('r301', 'octane', 'defender', 'frag', 'wingman', 'r99', 'kunai', 'flatline', 'sentinel', 'hits', 'grapple'), default='r301', help='Sound set (default: r301)')
    parser.add_argument('--output-dir', type=Path, help='Override output root; Octane stays inside audio/octane/, the Charge Rifle inside audio/defender/, frag inside audio/frag_grenade/')
    parser.add_argument('--root', type=Path, help="Checkout whose apex-data/ and RSX build are used (default: this script's)")
    parser.add_argument('--analyze-only', action='store_true', help='Verify/rebuild manifests from exported raw WAVs')
    parser.add_argument('--matrix', type=Path, help='JSON 2x6 replacement for provisional six-channel matrix')
    args = parser.parse_args()
    if args.root is not None:
        set_root(args.root)
    out = output_directory(args.set, args.output_dir)
    # the ability-style sets (exact spellings, every play action kept, loop data): Octane's, the
    # Charge Rifle's, the frag's
    octane = args.set in ('octane', 'defender', 'frag', 'wingman', 'r99', 'kunai', 'flatline', 'sentinel', 'hits', 'grapple')
    defender = args.set in ('defender', 'wingman', 'r99', 'kunai', 'flatline', 'sentinel', 'hits', 'grapple')
    playback_folder = 'playback' if octane else 'r301'
    matrix = np.asarray(json.loads(args.matrix.read_text()) if args.matrix else DEFAULT_MATRIX, dtype=np.float64)
    if matrix.shape != (2, 6) or not np.isfinite(matrix).all():
        raise ValueError('Downmix matrix must be finite 2x6')
    # Row normalization provides conservative peak headroom.
    matrix /= np.maximum(1, np.abs(matrix).sum(axis=1))[:, None]
    for folder in (playback_folder, 'raw', 'logs', 'logs/rsx_runtime'):
        (out / folder).mkdir(parents=True, exist_ok=True)
    refs = grapple_references() if args.set == 'grapple' else hits_references() if args.set == 'hits' else sentinel_references() if args.set == 'sentinel' else flatline_references() if args.set == 'flatline' else kunai_references() if args.set == 'kunai' else r99_references() if args.set == 'r99' else wingman_references() if args.set == 'wingman' else defender_references() if defender else frag_references() if args.set == 'frag' else octane_references() if octane else references()
    events = {r['asset_name'].lower(): r for r in csv.DictReader(
        (REPO / 'apex-data/assets/lists/audio_events.csv').open(encoding='utf8'))}
    available = {(r['asset_name'], r['file_name']): r for r in csv.DictReader(
        (REPO / 'apex-data/assets/lists/audio_named.csv').open(encoding='utf8'))}
    bank = Bank(GAME / 'audio/ship/general.mbnk', preserve_action_bytes=octane)
    graph = {}
    sources = {}
    unresolved = []
    for ref in refs:
        name = ref['event']
        if not name or name.lower() not in events:
            ref['status'] = '待定：字段为空或事件表未收录'
            unresolved.append(ref.copy())
            continue
        if octane:
            # Apex matching is case insensitive; save the exact installed spelling.
            name = ref['event'] = events[name.lower()]['asset_name']
            if name.lower() not in bank.events:
                ref['status'] = '待定：本机银行未收录事件'
                unresolved.append(ref.copy())
                continue
        ref['event_guid'] = events[name.lower()]['guid']
        resolved, nodes = bank.resolve(name)
        ref['sources'] = resolved
        ref['status'] = 'resolved' if resolved else '待定：事件无可解析的源'
        for node in nodes:
            node['guid'] = events[node['name'].lower()]['guid']
            graph[node['name'].lower()] = node
        if octane:
            missing = [s for s in resolved if (s['name'], s['file_name']) not in available]
            if missing or not resolved:
                ref['status'] = '待定：本机缺少事件引用的源' if missing else ref['status']
                ref['missing_sources'] = missing
                unresolved.append(ref.copy())
                continue
        for source in resolved:
            key = (source['name'], source['file_name'])
            if key not in available:
                raise ValueError(f'Selector source absent from T001 available inventory: {source}')
            source['guid'] = available[key]['guid'].lower().zfill(16)
            sources[source['index']] = source
    bank.close()
    # Cross-check direct selector indices against T001's full, unfiltered source table.
    matched = set()
    with gzip.open(REPO / 'apex-data/assets/lists/audio_sources.csv.gz', 'rt', encoding='utf8', newline='') as file:
        for row in csv.DictReader(file):
            index = int(row['index'])
            if index not in sources:
                continue
            source = sources[index]
            if row['asset_name'] != source['name'] or row['file_name'] != source['file_name']:
                raise ValueError(f'T001 source index/name/stream mismatch: {index}')
            if row['guid'].lower().zfill(16) != source['guid']:
                raise ValueError(f'T001 source GUID mismatch: {index}')
            for field in ('sample_rate', 'sample_count', 'language_idx', 'patch_idx',
                          'stream_header_offset', 'stream_data_offset'):
                if int(row[field]) != source[field]:
                    raise ValueError(f'T001 source offset/format mismatch: {index}/{field}')
            matched.add(index)
    if matched != sources.keys():
        raise ValueError('Some selector indices missing from full T001 source table')
    dump(out / 'event_map.json', {'references': refs, 'event_graph': graph,
                                 'source_index_policy': 'MBNK source table index, never RSX filtered-name index'})
    if not args.analyze_only and sources:
        if not RSX.is_file():
            raise FileNotFoundError(f'T001 patched RSX missing: {RSX}; ask project lead to run T001 build')
        command = [str(RSX), '-nogui', '-export', '--loadwhitelist', 'x', '--exportthreads', '4',
                   '--parsethreads', '8', '--exportdir', str(out / 'raw'), '-exportfullpaths', '-nocachedb',
                   '--exporttypes', 'asrc', '--exportexact', ','.join(sorted({s['name'] for s in sources.values()})),
                   '--audiostreams', ','.join(sorted({s['file_name'] for s in sources.values()})),
                   str(GAME / 'audio/ship/general.mbnk')]
        started = time.perf_counter()
        with (out / 'logs/export.log').open('wb') as log:
            result = subprocess.run(command, cwd=out / 'logs/rsx_runtime', stdout=log, stderr=subprocess.STDOUT)
        dump(out / 'logs/run.json', {'command': command, 'cwd': str(out / 'logs/rsx_runtime'),
                                   'seconds': time.perf_counter() - started, 'exit_code': result.returncode})
        if result.returncode:
            raise RuntimeError(f'RSX failed ({result.returncode}); see {out / "logs/export.log"}')
    wavs = {(p.stem, p.parent.name + '.mstr'): p for p in (out / 'raw').rglob('*.wav')}
    inventory = {}
    for index, source in sorted(sources.items()):
        path = wavs[(source['name'], source['file_name'])]
        samples, info = read_wave(path, include_loop_data=octane)
        # Null is a real silent timing layer with explicit -96 dB action volume.
        silent = info['peak'] == 0
        if silent and source['name'] != 'null_12s':
            raise ValueError(f'Unexpected silent source: {path}')
        if info['sample_rate'] != source['sample_rate']:
            raise ValueError(f'Decoded rate does not match source record: {path}')
        if info['frames'] != source['sample_count']:
            raise ValueError(f'Decoded/source frame count discrepancy: {path}')
        if info['channels'] == 6:
            stereo = samples @ matrix.T
        elif info['channels'] == 1:
            stereo = np.repeat(samples, 2, axis=1)
        else:
            stereo = samples.copy()
        # Uniform attenuation only when required; no per-sample hard clipping in assets.
        peak = float(np.abs(stereo).max())
        gain = min(1.0, 0.98 / peak) if peak else 1.0
        stereo *= gain
        target = out / playback_folder / f'{source["name"]}.wav'
        write_wave(target, stereo, info['sample_rate'])
        output_samples, output_info = read_wave(target, include_loop_data=octane)
        if output_info['channels'] != 2 or output_info['sample_rate'] != info['sample_rate'] or output_info['frames'] != info['frames']:
            raise ValueError(f'Downmixed channels/rate/frame count mismatch: {target}')
        if not np.array_equal(output_samples, np.asarray(stereo, dtype='<f4')):
            raise ValueError(f'Downmixed WAV content mismatch: {target}')
        if output_info['peak'] > 0.980001 or (not silent and output_info['peak'] == 0):
            raise ValueError(f'Invalid downmixed peak/silence: {target}')
        record = {k: v for k, v in source.items() if k not in
                  ('selector_offset', 'weight', 'event_chain', 'event_action_offset', 'decoded_action_offset')}
        event_refs = [{'field': ref['field'], 'event': ref['event'],
                       **{k: edge[k] for k in ('selector_offset', 'weight', 'event_chain',
                                               'event_action_offset', 'decoded_action_offset')}}
                      for ref in refs for edge in ref.get('sources', []) if edge['index'] == index]
        inventory[index] = {**record, 'event_references': event_refs,
                            'raw_file': path.relative_to(out).as_posix(), 'raw_format': info,
                            'file': target.relative_to(out).as_posix(), **output_info,
                            'attenuation_gain': gain, 'silent_timing_layer': silent}
        if octane:
            inventory[index]['loop_data'] = {
                'raw_wav': info['wav_loop_data'], 'playback_wav': output_info['wav_loop_data'],
                'bank_marker_count': source['marker_count'], 'bank_markers': source['markers'],
                'miles_runtime': '待定：本机 RSX 动作结构未确认循环开关、次数及循环区间；不由事件名称推断',
                'library_playback': 'one complete WAV per play(); no automatic looping',
            }
    sounds = {}
    for ref in refs:
        if ref['status'] != 'resolved':
            continue
        node = graph[ref['event'].lower()]
        actions = [a for a in node['actions'] if a['sources']]
        # Core = first play action, except 3P loop where close is selector #1.
        selected = actions[0]['sources']
        if ref['logical'] == 'fire_loop_3p':
            tree = actions[0]['states'][0]['selectors'][1]
            indices = {c['source_index'] for c in tree['children']}
            selected = [s for s in selected if s['index'] in indices]
        group = 'ability' if octane else 'reload' if ref['logical'].startswith('reload') else 'weapon'
        if defender:
            group = 'reload' if '_reload' in ref['logical'] else 'weapon'
        entry = {'group': group,
                 'max_instances': 4 if ref['logical'].startswith('fire') else 2,
                 'event': ref['event'], 'event_guid': ref['event_guid'], 'evidence_field': ref['field'],
                 'playback_policy': 'core source round robin; full controller/layer semantics remain in event_map.json',
                 'variants': [{**inventory[s['index']], 'selector_offset': s['selector_offset'],
                               'weight': s['weight'], 'event_chain': [node['name']],
                               'event_action_offset': node['action_offset'],
                               'decoded_action_offset': actions[0]['decoded_offset']} for s in selected]}
        sounds[ref['logical']] = entry
        # Expose every action's distinct sources for audition, without merging layers into the core rotation.
        for ordinal, action in enumerate(actions):
            sounds[f'{ref["logical"]}_layer{ordinal}'] = {**entry,
                'variants': [{**inventory[s['index']], 'selector_offset': s['selector_offset'],
                              'weight': s['weight'], 'event_chain': [node['name']],
                              'event_action_offset': node['action_offset'],
                              'decoded_action_offset': action['decoded_offset']} for s in action['sources']]}
    if not octane:
        for name in ('hit', 'headshot', 'kill'):
            unresolved.append({'logical': name, 'status': '待定：武器定义及已导出 player/global settings 未找到明确声音引用；不按事件名猜测'})
    manifest = {'version': 1, 'sounds': sounds, 'unresolved': unresolved,
                'downmix': {'status': '待定：6ch 的实际游戏通道布局/下混未确认',
                            'assumed_order': ['FL', 'FR', 'FC', 'LFE', 'SL', 'SR'],
                            'policy': 'provisional default: center/surround -3 dB; omit LFE; normalize each row by absolute coefficient sum',
                            'matrix': matrix.tolist(), 'replacement': '--matrix PATH.json'},
                'scope': 'fire_3p uses real Mods.altfire single-shot close core; default automatic start_3p is empty; loops play once in this library'}
    if octane:
        manifest['scope'] = 'Octane ability events verified from local S3 scripts and retail settings; core = first play action; layers retain each play action; loops play once in this library'
    if args.set == 'frag':
        manifest['scope'] = 'U9 frag grenade events verified from local S3 weapon settings, impact tables and the view model QC; core = first play action; layers retain each play action; loops play once in this library'
    if args.set == 'hits':
        # the armour break's own shatter (its event's action lists three sources at equal weight,
        # the other two a lock and a whoosh: 推断 they are picked one at a time; the user asked for
        # the break, so only it plays)
        for sound in sounds.values():
            sound['variants'] = [v for v in sound['variants'] if 'markbreak' in v['name']] or sound['variants']
        manifest['scope'] = "a damaging hit's sound: the armour break heard by the attacker, its shatter only"
    dump(out / 'manifest.json', manifest)
    dump(out / 'source_inventory.json', list(inventory.values()))
    lines = ['# T007 本机事件与音源映射', '',
             '输入：`apex-data/fuse_data.json`、`export/weapon/mp_weapon_rspn101.txt`、T001 QC 与音频清单；银行 `audio/ship/general.mbnk` v49。', '',
             '源索引是完整 MBNK 表索引；全部与 `audio_sources.csv.gz` 的名称、语言、patch、采样率、帧数和 MSTR 偏移交叉核对。', '',
             '逻辑主声音只轮换核心层；每个事件的完整选择器树、权重、状态、动作类型及偏移见 `event_map.json`。控制器语义不等同于纯随机选择。', '']
    if defender:
        lines = ['# U3 本机充能步枪事件与音源映射', '',
                 '输入：本机 R5R S3 武器设置（mp_weapon_defender.txt、_base_sniper.txt）与脚本（mp_weapon_defender.nut）、正式版 `export/weapon/`、本机正式版第一人称 QC（chargerifle_base_v_animRig.qc、ptpov_rspn101.qc）；银行 `audio/ship/general.mbnk` v49。', '',
                 '源索引是完整 MBNK 表索引；全部与 `audio_sources.csv.gz` 的名称、GUID、语言、patch、采样率、帧数和 MSTR 偏移交叉核对。', '',
                 '主播放名为本机事件名的小写；主声音沿用第一个 play action 策略，每个 play action 另有 `_layerN` 名。', '']
    elif octane:
        lines = ['# T018 本机 Octane 事件与音源映射', '',
                 '输入：本机 R5R S3 武器设置与脚本、正式版 `export/weapon/`、T001 音频清单；银行 `audio/ship/general.mbnk` v49。', '',
                 '源索引是完整 MBNK 表索引；全部与 `audio_sources.csv.gz` 的名称、GUID、语言、patch、采样率、帧数和 MSTR 偏移交叉核对。', '',
                 '主播放名为本机事件名的小写；主声音沿用 R-301 的第一个 play action 策略，每个 play action 另有 `_layerN` 名。',
                 '同一 action 的不同选择器不代表已经确认的随机池；状态、选择器、权重、原始动作字节保留在 `event_map.json`。',
                 '循环数据记录 MBNK 源标记及 WAV `smpl` 标记；Miles 动作循环语义待定。当前 Rust 库的每次 `play()` 只播放一次完整 WAV。', '']
    if args.set == 'frag':
        lines[0] = '# U9 本机破片手雷事件与音源映射'
        lines[2] = '输入：本机 R5R S3 武器设置与撞击表（platform/scripts/impacts）、正式版 `export/weapon/`、视图模型 QC（`assets/frag_grenade`）、T001 音频清单；银行 `audio/ship/general.mbnk` v49。'
    for ref in refs:
        lines += [f'## {ref["logical"]}', '', f'- 字段：`{ref["input"]}` → `{ref["field"]}`',
                  f'- 原始事件：`{ref["event"]}`；GUID：`{ref.get("event_guid", "待定")}`；状态：{ref["status"]}', '']
        if octane:
            lines += [f'- 请求拼写：`{ref["requested_event"]}`；用途：{ref["use"]}',
                      '- 播放名：' + '、'.join(f'`{n}`' for n in sounds if n == ref['logical'] or n.startswith(ref['logical'] + '_layer')), '']
            for evidence in ref['evidence']:
                lines.append(f'- 引用证据：`{evidence["input"]}:{evidence["line"]}` → `{evidence["field"]}` → `{evidence["event"]}`')
            lines.append('')
        if 'sources' not in ref:
            continue
        lines += ['| 源名 | 表索引 | 源 GUID | MBNK 记录偏移 | MSTR 头/数据偏移 | 事件动作偏移/解压动作偏移/选择器偏移 | 原始格式 |',
                  '|---|---:|---|---:|---|---|---|']
        for source in ref['sources']:
            if source['index'] not in inventory:
                lines.append(f'| `{source["name"]}` | {source["index"]} | 待定：本机缺源 | | | | |')
                continue
            item = inventory[source['index']]
            info = item['raw_format']
            lines.append(f'| `{source["name"]}` | {source["index"]} | `{source["guid"]}` | {source["record_offset"]} | '
                         f'{source["stream_header_offset"]} / {source["stream_data_offset"]} | '
                         f'{source["event_action_offset"]} / {source["decoded_action_offset"]} / {source["selector_offset"]} | '
                         f'{info["sample_rate"]} Hz / {info["channels"]} ch / {info["seconds"]:.6f} s |')
        lines.append('')
        if octane and ref['status'] == 'resolved':
            lines += ['| 播放名 | 源索引（顺序保留） | WAV 循环标记 |', '|---|---|---|']
            for name, sound in sounds.items():
                if name == ref['logical'] or name.startswith(ref['logical'] + '_layer'):
                    indices = ', '.join(str(v['index']) for v in sound['variants'])
                    loop_marks = [v['loop_data']['raw_wav'] for v in sound['variants']]
                    lines.append(f'| `{name}` | {indices} | `{json.dumps(loop_marks, ensure_ascii=False)}` |')
            lines.append('')
    (out / 'mapping.md').write_text('\n'.join(lines), encoding='utf8')
    verification = {'status': 'passed', 'unique_sources': len(inventory),
                                     'logical_sounds': len(sounds), 'silent_timing_layers': sum(s['silent_timing_layer'] for s in inventory.values()),
                                     'all_riff_lengths_valid': True, 'all_samples_finite': True,
                                     'all_frame_counts_match_bank': True, 'all_sources_match_t001_full_table': True,
                                     'all_playback_wavs_stereo': True, 'all_playback_peaks_at_most': 0.980001}
    if octane:
        verification.update({
            'requested_events': len(refs), 'resolved_events': sum(r['status'] == 'resolved' for r in refs),
            'unresolved_events': len(unresolved), 'all_playback_wavs_float32': True,
            'all_playback_wavs_non_silent': all(s['peak'] > 0 and s['rms'] > 0 for s in inventory.values()),
            'all_playback_content_matches_downmix': True,
            'files': [{'file': s['file'], 'bytes': s['bytes'], 'source_index': s['index'],
                       'header_valid': True, 'float32_stereo': True, 'frames': s['frames'],
                       'expected_frames': s['sample_count'], 'sample_rate': s['sample_rate'],
                       'peak': s['peak'], 'rms': s['rms'], 'non_silent': s['peak'] > 0 and s['rms'] > 0,
                       'content_matches_downmix': True} for s in inventory.values()],
        })
    dump(out / 'verification.json', verification)
    print(f'passed: {len(inventory)} unique WAV sources, {len(sounds)} playback names; manifest: {out / "manifest.json"}')


if __name__ == '__main__':
    main()
