"""T020 local sources, FPOV v1/v2 IO, QC metadata and independent prop branches.

No input is repaired. Source data is read-only and Python bytecode is disabled.
"""
from __future__ import annotations

import json
from pathlib import Path
import re
import struct
import sys

sys.dont_write_bytecode = True
import numpy as np
import bake_pov as bp
import pov_sequences as ps
from cast import Model
from verify_pov_pack import Reader, require

ROOT = bp.REPO
ASSETS = ROOT / 'apex-data/assets/octane'
PACK_ROOT = ROOT / 'apex-data/pov/octane_ability'
MODEL_ROOT = ROOT / 'er-data/s3/octane_pov_ability'
BASE_PACK = ROOT / 'apex-data/pov/octane/fuse_pov.anim'
BASE_MODEL = ROOT / 'er-data/s3/octane_pov'
LIVE_SKELETON = ROOT / 'er-data/skeleton/c0000_live_skeleton.json'
XTRA_NAMES = tuple(f'Xtra_Multipurpose_Bone{chain:02d}_{i:02d}' for chain in (1, 2) for i in range(1, 11))
R301_SEQUENCES = ('holster', 'draw', 'raise', 'lower', 'sprintholster',
                  'sprintdraw', 'switch_to_onehanded', 'switch_to_twohanded',
                  'idle_onehanded', 'sprint_onehanded', 'fire_onehanded',
                  'ads_in_onehanded', 'ads_out_onehanded', 'jump_onehanded',
                  'land_onehanded', 'sprintraise_onehanded')
CONFIGS = {}
for key, folder, prefix, part, group in [('epipen', 'octane_epipen', 'stim', 'hd', 2),
                                      ('jumppad', 'octane_jump_pad', 'pad', 'lg', 3)]:
    stem = 'ptpov_' + folder + '_held'
    CONFIGS[key] = dict(key=key, prefix=prefix, part=part, group=group,
        model=ASSETS/f'cast/mdl/Weapons/{folder}/{stem}_LOD0.cast',
        rig=ASSETS/f'cast/animrig/weapons/{folder}/{stem}.cast',
        anims=ASSETS/f'cast/animrig/weapons/{folder}/anims_{stem}',
        qc=ASSETS/f'smd/animrig/weapons/{folder}/{stem}.qc',
        metadata=ASSETS/f'sequences/{stem}.json')


def checked(path, root, create=True):
    path = Path(path).resolve()
    require(path.is_relative_to(root.resolve()), f'Output must stay inside {root}')
    if create:
        path.mkdir(parents=True, exist_ok=True)
    return path


def save(path, data):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, ensure_ascii=False, indent=2)+'\n', encoding='utf8')


def info(path):
    path = Path(path)
    return dict(path=path.relative_to(ROOT).as_posix(), size=path.stat().st_size)


def read_pack(path):
    data = Path(path).read_bytes()
    r = Reader(data)
    require(r.take(4) == b'FPOV', 'Not FPOV')
    version, nb = r.unpack('<II')
    require(version in (1, 2) and 0 < nb <= 512, 'Bad FPOV version/bone count')
    bones = []
    for i in range(nb):
        start = r.offset
        name, parent, rest = r.name(), r.unpack('<h')[0], r.unpack('<7f')
        require(-1 <= parent < i, f'Invalid parent: {name}')
        bones.append(dict(name=name, parent=parent, rest=rest, raw=data[start:r.offset]))
    camera, nc = r.unpack('<II')
    require(camera < nb and nc <= 256, 'Invalid camera/carrier count')
    carriers = []
    for i in range(nc):
        start = r.offset
        name, owner = r.name(), r.unpack('<I')[0]
        inverse, er = r.unpack('<7f'), r.unpack('<7f')
        raw = data[start:r.offset]
        group = r.unpack('<B')[0] if version == 2 else 0
        require(owner < nb and group <= 3, f'Invalid carrier: {name}')
        carriers.append(dict(name=name, owner=owner, inverse_mesh_bind=inverse,
                             er_bind=er, group=group, raw=raw))
    clips = {}
    count = r.unpack('<I')[0]
    require(0 < count <= 256, 'Invalid clip count')
    for i in range(count):
        start = r.offset
        name = r.name()
        fps, frames, loop, additive = r.unpack('<fIBB')
        header = data[start:r.offset]
        require(0 < fps < 1000 and 0 < frames <= 100000 and loop in (0, 1) and additive in (0, 1), 'Invalid clip header')
        weights = np.frombuffer(r.take(nb*8), '<f4').reshape(nb, 2)
        poses = np.frombuffer(r.take(frames*nb*28), '<f4').reshape(frames, nb, 7)
        require(name not in clips and np.isfinite(poses).all() and np.isfinite(weights).all(), 'Invalid clip data')
        require(np.max(abs(np.linalg.norm(poses[..., 3:], axis=-1)-1)) < .01, f'Bad quaternion: {name}')
        require((weights >= 0).all() and (weights <= 1).all(), f'Bad weights: {name}')
        clips[name] = dict(name=name, fps=fps, frames=frames, loop=bool(loop), additive=bool(additive),
                           weights=weights, poses=poses, header=header)
    require(r.offset == len(data), 'Trailing bytes')
    require(len({b['name'] for b in bones}) == nb and len({c['name'] for c in carriers}) == nc, 'Duplicate bones/carriers')
    require(np.isfinite([b['rest'] for b in bones]).all() and np.isfinite([c['inverse_mesh_bind']+c['er_bind'] for c in carriers]).all(), 'Nonfinite binding')
    return dict(path=str(path), data=data, version=version, bones=bones, camera=camera,
                carrier_records=carriers, clips=clips)


def write_pack(path, pack):
    out = bytearray(b'FPOV'+struct.pack('<II', 2, len(pack['bones'])))
    for b in pack['bones']:
        out += b.get('raw', bp.name(b['name'])+struct.pack('<h7f', b['parent'], *b['rest']))
    out += struct.pack('<II', pack['camera'], len(pack['carrier_records']))
    for c in pack['carrier_records']:
        out += c.get('raw', bp.name(c['name'])+struct.pack('<I14f', c['owner'], *c['inverse_mesh_bind'], *c['er_bind']))
        out += struct.pack('<B', c['group'])
    out += struct.pack('<I', len(pack['clips']))
    for c in pack['clips'].values():
        out += c.get('header', bp.name(c['name'])+struct.pack('<fIBB', c['fps'], c['frames'], c['loop'], c['additive']))
        out += np.asarray(c['weights'], '<f4').tobytes()
        out += np.asarray(c['poses'], '<f4').tobytes()
    Path(path).write_bytes(out)


def read_qc(path):
    """Parse every local sequence, animation and explicit bone weight list."""
    text = Path(path).read_text(encoding='utf-8-sig')
    weights = {}
    for m in re.finditer(r'\$(defaultweightlist|weightlist)(?:\s+"([^"]+)")?\s*\{([^}]+)\}', text):
        weights[m[2] or 'defaultweightlist'] = {b: float(w) for b, w in re.findall(r'"([^"]+)"\s+([-\d.eE+]+)', m[3])}
    sequences, animations = {}, {}
    for kind, name, raw, tokens, line in ps._blocks(text):
        if kind == 'animation':
            animations[name] = dict(file=ps._value(tokens[2]), fps=float(re.search(r'\bfps\s+([\d.]+)', raw)[1]),
                                    loop='loop' in tokens, delta='delta' in tokens)
            continue
        seq = dict(sample_animation_names=[], blendwidth=None, blend=[], activity=None,
                   activitymodifiers=[], fadein=None, fadeout=None, loop=False, delta=False,
                   autoplay=False, addlayer=[], node=None, transition=None, events=[], posecycle=None,
                   weightlist='defaultweightlist', qc_line=line, raw_qc=raw)
        i = 2
        while i < len(tokens):
            token = tokens[i]; i += 1
            if token in ('{', '}'):
                continue
            if token.startswith('"'):
                seq['sample_animation_names'].append(ps._value(token))
            elif token in ('loop', 'delta', 'autoplay'):
                seq[token] = True
            elif token in ('blendwidth', 'fadein', 'fadeout'):
                seq[token] = (int if token == 'blendwidth' else float)(tokens[i]); i += 1
            elif token == 'blend':
                seq['blend'].append(dict(parameter=ps._value(tokens[i]), min=float(tokens[i+1]), max=float(tokens[i+2]))); i += 3
            elif token == 'activity':
                seq['activity'] = dict(name=ps._value(tokens[i]), weight=int(tokens[i+1])); i += 2
            elif token in ('weightlist', 'node', 'posecycle'):
                seq[token] = ps._value(tokens[i]); i += 1
            elif token in ('activitymodifier', 'addlayer'):
                seq['activitymodifiers' if token == 'activitymodifier' else token].append(ps._value(tokens[i])); i += 1
            elif token == 'transition':
                seq['transition'] = [ps._value(t) for t in tokens[i:i+2]]; i += 2
            elif token == 'event':
                event = dict(name=ps._value(tokens[i]), frame=int(tokens[i+1]), options=[]); i += 2
                while i < len(tokens) and tokens[i] != '}':
                    event['options'].append(ps._value(tokens[i])); i += 1
                seq['events'].append(event)
            else:
                raise ValueError(f'{path}:{line}: unsupported QC option {token}')
        require(bool(seq['sample_animation_names']), f'No samples: {name}')
        sequences[name] = seq
    poses = re.findall(r'^\$poseparameter\s+"([^"]+)"', text, re.MULTILINE)
    return dict(sequences=sequences, animations=animations, weightlists=weights, poseparameters=poses)


def model(path):
    return next(n for n in bp.ea.nodes(path) if isinstance(n, Model))


def selected(key):
    mdl = model(CONFIGS[key]['model'])
    meshes = [m for m in mdl.Meshes() if m.Name().startswith('Base_1_')] if key == 'epipen' else mdl.Meshes()
    require(len(meshes) == (2 if key == 'epipen' else 1), f'Unexpected ability meshes: {key}')
    return mdl, meshes


def world7(bones, pose):
    """Parent-first FPOV/Cast rig FK, with normalized rotations."""
    result = []
    for i, b in enumerate(bones):
        local = bp.trs(pose[i, :3], pose[i, 3:7])
        result.append(result[b['parent']]@local if b['parent'] >= 0 else local)
    return np.asarray(result)


def model_world(mdl):
    """Model bone order differs from the animation rig; recurse by actual indices."""
    bones = mdl.Skeleton().Bones()
    result, active = {}, set()
    def visit(i):
        if i not in result:
            require(i not in active, 'Cyclic model skeleton')
            active.add(i)
            b = bones[i]
            local = bp.trs(b.LocalPosition(), b.LocalRotation(), b.Scale() or (1, 1, 1))
            result[i] = visit(b.ParentIndex())@local if b.ParentIndex() >= 0 else local
            active.remove(i)
        return result[i]
    return {b.Name(): visit(i) for i, b in enumerate(bones)}


def live_skeleton():
    live = json.loads(LIVE_SKELETON.read_text(encoding='utf8'))
    require(live['bone_count'] == len(live['bones']) == 150 and live['reference_pose_valid'], 'Invalid live skeleton/reference pose')
    require(len({b['name'] for b in live['bones']}) == 150, 'Duplicate live bones')
    return live


def forbidden_prop_carrier(name):
    return (name in ('Master', 'RootPos', 'RootRotY', 'RootRotXZ', 'L_Weapon', 'R_Weapon', 'L_Shield', 'R_Shield', 'Pelvis', 'Spine', 'Spine1')
            or bool(re.search(r'_(Target|Dummy)|_(Hip|Thigh|Knee|Calf|Foot|Toe)(?!.*_(Skirt|Mantle)$)', name)))


def xtra_binds(part):
    """World binds in the authorized template copies: existing Master + live Xtra locals."""
    nodes = json.loads((ROOT/f'er-data/json/parts/{part.upper()}_M_1280.json').read_text())['Nodes']
    master = next(n for n in nodes if n['Name'] == 'Master')
    require(master['ParentIndex'] == -1, 'Template Master must be a root')
    matrices = {'Master': bp.trs(master['Translation'], master['RotationQuaternion'], master['Scale'])}
    live = live_skeleton()
    names = [b['name'] for b in live['bones']]
    for b in live['bones']:
        if b['name'] not in XTRA_NAMES:
            continue
        ref = b['ref']
        parent = names[b['parent']]
        require(parent in matrices, f'Xtra parent missing: {parent}')
        matrices[b['name']] = matrices[parent]@bp.trs(ref['t'], ref['r'], ref['s'])
    return {n: matrices[n] for n in XTRA_NAMES}


def layout(base=None):
    base = base or read_pack(BASE_PACK)
    require(len(base['bones']) == 102 and len(base['carrier_records']) == 51 and len(base['clips']) == 39, 'Baseline counts differ')
    bones = [dict(b) for b in base['bones']]
    pack_index = {b['name']: i for i, b in enumerate(bones)}
    sources = {key: bp.skeleton(c['rig']) for key, c in CONFIGS.items()}
    maps, added, differences, carriers, fallback = {}, [], [], [], []
    used_carriers = {c['name'] for c in base['carrier_records']}
    for key, config in CONFIGS.items():
        rig = sources[key]
        idx = {b['name']: i for i, b in enumerate(rig)}
        mdl, meshes = selected(key)
        positive = {mdl.Skeleton().Bones()[int(b)].Name() for m in meshes for b, w in
                    zip(m.VertexWeightBoneBuffer(), m.VertexWeightValueBuffer()) if w > 0}
        prop = set(positive)
        # The shared weapon-only ancestor chain also moves each prop (ja_c_propGun
        # has nonidentity ability tracks). Copy it up to the first body branch;
        # otherwise changing an ability would also move the rifle's parent.
        for n in positive:
            parent = rig[idx[n]]['parent']
            while parent >= 0:
                pname = rig[parent]['name']
                if pname in pack_index and sum(b['parent'] == parent for b in rig) > 1:
                    break
                prop.add(pname)
                parent = rig[parent]['parent']
        other_names = {b['name'] for k, rows in sources.items() if k != key for b in rows}
        mapping = {n: pack_index[n] for n in idx if n in pack_index and n not in prop}
        rest = bp.ea.rest_pose(rig)
        for i, b in enumerate(rig):
            n = b['name']
            if n not in prop:
                continue
            output = key+':'+n if n in pack_index or n in other_names else n
            require(output not in {x['name'] for x in bones}, f'Bone collision: {output}')
            parent_name = rig[b['parent']]['name'] if b['parent'] >= 0 else None
            parent = mapping[parent_name] if parent_name else -1
            mapping[n] = len(bones)
            bones.append(dict(name=output, parent=parent, rest=tuple(rest[i, :7])))
            added.append(dict(index=mapping[n], name=output, source_bone=n, parent=bones[parent]['name'] if parent >= 0 else None,
                              rig=key, copied=output != n, positive_mesh_weight=n in positive))
        for b in base['bones']:
            if b['name'] not in idx:
                continue
            oldparent = base['bones'][b['parent']]['name'] if b['parent'] >= 0 else None
            source_parent = rig[idx[b['name']]]['parent']
            newparent = rig[source_parent]['name'] if source_parent >= 0 else None
            if oldparent != newparent:
                differences.append(dict(rig=key, bone=b['name'], pack_parent=oldparent, source_parent=newparent))
        maps[key] = mapping
        # Revision 4 plus the lead's explicit template-copy authorization:
        # add all twenty live Xtra nodes in each HD/LG copy; use distinct names.
        live_names = {b['name'] for b in live_skeleton()['bones']}
        available = [n for n in XTRA_NAMES if n in live_names and n not in used_carriers and not forbidden_prop_carrier(n)]
        binds = xtra_binds(config['part'])
        mesh_bind = model_world(mdl)
        by_source = {}
        for b in rig:
            n = b['name']
            if n not in positive:
                continue
            if available:
                carrier = available.pop(0)
                used_carriers.add(carrier)
                by_source[n] = carrier
                carriers.append(dict(name=carrier, owner=mapping[n], owner_name=bones[mapping[n]]['name'], source_bone=n,
                    rig=key, part=config['part'], group=config['group'],
                    inverse_mesh_bind=bp.rigid(np.linalg.inv(mesh_bind[n]), n).tolist(),
                    er_bind=bp.rigid(binds[carrier], carrier).tolist()))
            else:
                parent = b['parent']
                while parent >= 0 and rig[parent]['name'] not in by_source:
                    parent = rig[parent]['parent']
                require(parent >= 0, f'No carrier or ancestor: {key}/{n}')
                by_source[n] = by_source[rig[parent]['name']]
                fallback.append(dict(rig=key, source_bone=n, ancestor=rig[parent]['name'], carrier=by_source[n]))
        config_map = dict(by_source)
        maps[key+'-carriers'] = config_map
    arm_names = {b['name'] for b in bp.skeleton(bp.OCTANE_ARMS)}
    gun_names = {b['name'] for b in bp.skeleton(bp.MODELS[1])}
    old_carriers = []
    for c in base['carrier_records']:
        owner = bones[c['owner']]['name']
        require(owner in arm_names or owner in gun_names, f'Unknown existing carrier group: {c["name"]}/{owner}')
        old_carriers.append(dict(c, group=0 if owner in arm_names else 1))
    require(all(c['name'] in live_names for c in old_carriers+carriers), 'Carrier missing from live player skeleton')
    return dict(bones=bones, sources=sources, maps=maps, added=added, parent_differences=differences,
                carrier_records=old_carriers+carriers, new_carriers=carriers, ancestor_fallbacks=fallback)


# bake_wingman.py sets it for the kunai's sequences
DROP_SCALE = False


def decode_source(path, rig, seq, qc):
    animation = ps.animation(path)
    if not animation.Curves():
        require(Path(path).parent == bp.ANIMS, f'Unproved empty ability Cast: {path}')
        name, blend = Path(path).stem.rsplit('_', 1)
        raw = ps.read_raw(name, len(rig))
        ps.validate_raw(qc, name, raw)
        sample = raw['samples'][int(blend)]
        require(sample['additive'] and not sample['has_data'] and not animation.CurveModeOverrides(), f'Not a constant delta: {path}')
        require(animation.Framerate() == sample['fps'] and bool(animation.Looping()) == sample['loop'], f'Empty Cast header differs: {path}')
        require([(b.Name(), b.ParentIndex()) for b in animation.Skeleton().Bones()] == [(b['name'], b['parent']) for b in rig], 'Empty Cast skeleton differs')
        listed = qc['weightlists'][seq['weightlist']]
        require(np.array_equal(raw['bone_weights'], [listed[b['name']] for b in rig]), 'Empty Cast RSEQ/QC weights differ')
        return dict(fps=sample['fps'], frames=sample['frames'], loop=sample['loop'], additive=True,
                    weights=np.repeat(np.asarray(raw['bone_weights'])[:, None], 2, axis=1),
                    poses=np.tile([0., 0., 0., 0., 0., 0., 1.], (sample['frames'], len(rig), 1)),
                    constant_delta_source=dict(rseq=info(ROOT/raw['file']), animdesc_offset=sample['animdesc_offset'],
                        reason='Local RSEQ ANIM_VALID clear + ANIM_DELTA set; RSX initializes identity deltas. Original empty Cast remains read-only.'))
    a, frames, tracks, unknown = bp.ea.decode(path, rig)
    require(not unknown, f'{path}: unmapped bones {unknown}')
    additive = {t['mode'] for t in tracks} == {1}
    require(additive == seq['delta'], f'{path}: QC delta/Cast mode differs')
    require(bool(a.Looping()) == (seq['loop'] or any(qc['animations'][s]['loop'] for s in seq['sample_animation_names'])), f'{path}: QC loop/Cast differs')
    listed = qc['weightlists'][seq['weightlist']]
    require(all(b['name'] in listed for b in rig), f'{path}: incomplete QC weights')
    weights = np.array([[listed[b['name']]]*2 for b in rig])
    rest = bp.ea.rest_pose(rig)
    require(np.max(abs(rest[:, 7:]-1)) < 1e-4, 'Rig scale')
    default = np.tile([0, 0, 0, 0, 0, 0, 1, 1, 1, 1], (len(rig), 1)) if additive else rest
    poses = np.tile(default[None], (frames, 1, 1)).astype(float)
    for t in tracks:
        if additive:
            require(np.allclose(weights[t['bone']], t['weights'][:2], atol=1e-6), f'{path}: Cast/QC weight differs')
        for channel, (offset, width) in zip(t['channels'], ((0, 3), (3, 4), (7, 3))):
            if channel is not None:
                poses[:, t['bone'], offset:offset+width] = channel
    # (DROP_SCALE: a scale the pack cannot hold is dropped instead: the kunai's twirl squashes the knife to 0.64)
    require(DROP_SCALE or np.max(abs(poses[..., 7:]-1)) < 1e-3, f'{path}: nonunit scale')
    return dict(fps=a.Framerate(), frames=frames, loop=bool(a.Looping()), additive=additive,
                weights=weights, poses=poses[..., :7])


def remap_clip(source, layout_data, key):
    bones, rig, mapping = layout_data['bones'], layout_data['sources'][key], layout_data['maps'][key]
    default = np.tile([0, 0, 0, 0, 0, 0, 1], (len(bones), 1)) if source['additive'] else np.array([b['rest'] for b in bones])
    poses = np.tile(default[None], (source['frames'], 1, 1)).astype(float)
    weights = np.zeros((len(bones), 2))
    for i, b in enumerate(rig):
        if b['name'] in mapping:
            dest = mapping[b['name']]
            poses[:, dest] = source['poses'][:, i]
            weights[dest] = source['weights'][i]
    if any(d['rig'] == key for d in layout_data['parent_differences']):
        # Local absolute values are reconstructed from the source worlds in the
        # fixed pack hierarchy. Deltas are the same reconstruction relative to bind.
        source_bind = world7(rig, bp.ea.rest_pose(rig)[:, :7])
        target_bind = world7(bones, np.array([b['rest'] for b in bones]))
        for f in range(source['frames']):
            p = source['poses'][f]
            if source['additive']:
                p = p.copy()
                for i in range(len(rig)):
                    p[i, :3] += bp.ea.rest_pose(rig)[i, :3]
                    p[i, 3:] = bp.ea.normalize(bp.ea.mul(bp.ea.rest_pose(rig)[i, 3:7], p[i, 3:]))
            sw = world7(rig, p)
            target = target_bind.copy()
            for i, b in enumerate(rig):
                if b['name'] in mapping:
                    target[mapping[b['name']]] = sw[i]
            for i, b in enumerate(rig):
                if b['name'] not in mapping:
                    continue
                dest = mapping[b['name']]; parent = bones[dest]['parent']
                local = np.linalg.inv(target[parent])@target[dest] if parent >= 0 else target[dest]
                value = bp.rigid(local, b['name'])
                if source['additive']:
                    rest = np.asarray(bones[dest]['rest'])
                    value[:3] -= rest[:3]
                    value[3:] = bp.ea.normalize(bp.ea.mul(rest[3:]*[-1, -1, -1, 1], value[3:]))
                poses[f, dest] = value
    return dict(source, weights=weights.astype('<f4'), poses=poses.astype('<f4'))


def metadata(name, sequence, blend_index, seq, source, path, qc_path, qc):
    samples = len(seq['sample_animation_names'])
    width = seq['blendwidth'] or samples
    coords = []
    for axis, blend in enumerate(seq['blend']):
        count = width if axis == 0 else samples//width
        # blendwidth=1 is a vertical two-sample layout in Source's export.
        if len(seq['blend']) == 1:
            count = samples
        k = blend_index % width if axis == 0 and len(seq['blend']) > 1 else blend_index//width if axis else blend_index
        coords.append(dict(parameter=blend['parameter'], value=blend['min']+(blend['max']-blend['min'])*(k/(count-1) if count > 1 else 0)))
    events = [dict(e, time_seconds=e['frame']/source['fps']) for e in seq['events']]
    animation = qc['animations'][seq['sample_animation_names'][blend_index]]
    require(source['fps'] == animation['fps'], f'{name}: QC fps differs')
    result = dict(name=name, sequence=sequence, activity=seq['activity'], blend_index=blend_index,
        blend_parameters=coords, fps=source['fps'], frames=source['frames'], loop=source['loop'],
        additive=source['additive'], weightlist=seq['weightlist'], fadein=seq['fadein'], fadeout=seq['fadeout'],
        events=events, activitymodifiers=seq['activitymodifiers'], addlayer=seq['addlayer'], node=seq['node'],
        transition=seq['transition'], posecycle=seq['posecycle'], source=info(path), qc_source=info(qc_path),
        source_animation=animation['file'], raw_qc=seq['raw_qc'])
    if 'constant_delta_source' in source:
        result['constant_delta_source'] = source['constant_delta_source']
    return result
