"""The Wingman, the R-99 and the kunai: losslessly extend T022's pack (`wm:` / `r9:` / `kn:` branches,
carrier groups 9 / 10 / 11, clips `wm_*` / `r9_*` / `kn_*`)."""
import argparse
from pathlib import Path
import sys
sys.dont_write_bytecode=True
import numpy as np
import wingman_common as wm
from weapons_common import ac


def generate():
 base=wm.read_pack(wm.BASE_PACK);data=wm.layout(base);bones=data['bones'];old=len(base['bones']);count=len(bones)
 rest=np.asarray([b['rest'] for b in bones],'<f4');identity=np.tile([0,0,0,0,0,0,1],(count-old,1)).astype('<f4');clips={}
 for name,c in base['clips'].items():
  poses=np.empty((c['frames'],count,7),'<f4');poses[:,:old]=c['poses'];poses[:,old:]=identity if c['additive'] else rest[old:]
  clips[name]=dict(c,poses=poses,weights=np.concatenate([c['weights'],np.zeros((count-old,2),'<f4')]))
 tables={}
 for key,config in wm.CONFIGS.items():
  qc=wm.read_qc(config['qc']);rows,excluded=wm.sequence_rows(qc,key);records=[];rig=data['sources'][key]
  for unique,seq,local in rows:
   for sample in local['blends']:
    i=sample['blend_index'];path=config['assets']/sample['cast_file'];rawpath=config['assets']/local['raw_file']
    try:
     # the kunai's twirls scale the knife (0.64..1): played without the squash
     ac.DROP_SCALE=key=='kn'
     source=wm.wc.decode_source(path,rig,seq,qc,sample,rawpath)
    except ValueError as e:
     # a scaled bone (the R-99's drawfirst shows a prop by scale): the pack has no scale, the
     # sequence is left out (the runtime plays draw for it)
     if 'nonunit scale' not in str(e):raise
     excluded.append(dict(sequence=seq['sequence'],occurrence=seq['occurrence'],reason=f'nonunit scale in {path.name}'));continue
    wm.require((source['frames'],source['fps'])==(sample['frame_count'],sample['framerate']),'Cast/RSEQ timing mismatch')
    wm.require(source['additive']==bool(sample['flags']&4) and source['loop']==bool(sample['flags']&1),'RSEQ flags mismatch')
    name=f'{key}_{unique}_{i}';clip=wm.remap_clip(source,data,key);clip['name']=name;wm.require(name not in clips,'Clip collision');clips[name]=clip
    row=wm.metadata(name,seq['sequence'],i,seq,source,path,config['qc'],qc)
    row.update(rig=key,sequence_occurrence=seq['occurrence'],sequence_guid=local['guid'],sequence_asset=local['asset_path'],rseq_source=wm.info(rawpath),snap=seq.get('snap',False))
    records.append(row)
  if key=='wm':
   # the Charge Rifle's inspect (T022 left it out: EXCLUDED_CR), on T022's own layout, for the inspect key
   cr=cr_inspect(data)
   for c in cr:wm.require(c['name'] not in clips,'Clip collision');clips[c['name']]=c
   records+=[c.pop('record') for c in cr]
  tables[key]=dict(format='octane-wingman-sequences',version=1,rig=key,clips=records,excluded_sequences=excluded,absent_qc_values='null means absent from local data; 待定',sound_events=[dict(sequence=r['sequence'],sequence_occurrence=r['sequence_occurrence'],blend_index=r['blend_index'],**e) for r in records for e in r['events'] if 'SOUND' in e['name']])
 table=tables
 modelbind=wm.model_world(wm.model(wm.CONFIG['model']))
 carriers=dict(format='octane-wingman-carriers',version=1,carriers=data['new_carriers'],ancestor_fallbacks=data['ancestor_fallbacks'],units='Model bind poses: inches, Apex axes; ER bind: meters, ER axes; quaternion xyzw',pistol_model_bind={n:dict(matrix=modelbind[n].tolist(),translation_rotation=wm.bp.rigid(modelbind[n],n).tolist()) for n in ('muzzle_flash','shell','def_c_base') if n in modelbind})
 audit=dict(format='octane-wingman-pack-audit',version=1,source=wm.info(wm.BASE_PACK),bones=count,carriers=len(data['carrier_records']),clips=len(clips),original_bones=old,original_carriers=wm.BASE_COUNTS[1],original_clips=wm.BASE_COUNTS[2],added_bones=data['added'],parent_differences=data['parent_differences'],new_carriers=data['new_carriers'],ancestor_fallbacks=data['ancestor_fallbacks'],bodygroups=data['bodygroups'],omitted_meshes=data['omitted_meshes'],weight_totals=data['weight_totals'],all_T022_bytes_preserved=True)
 return dict(bones=bones,camera=base['camera'],carrier_records=data['carrier_records'],clips=clips),table,audit,carriers


def cr_inspect(data):
 """The Charge Rifle's `inspect_basic` as T022 bakes its other clips (`cr_inspect_basic_<k>`), the
 Wingman's bones at rest."""
 wc=wm.wc;key='cr';config=wc.CONFIGS[key];t22=wc.layout();qc=wc.read_qc(config['qc'])
 local=wm.json.loads(config['metadata'].read_text(encoding='utf8'))['sequences']
 seq=qc['sequences']['inspect_basic'];files=[wm.Path(qc['animations'][a]['file']).stem for a in seq['sample_animation_names']]
 row=next(s for s in local if s['name']=='inspect_basic' and [b['name'] for b in s['blends']]==files)
 rig=t22['sources'][key];out=[];rest=np.asarray([b['rest'] for b in data['bones']],'<f4');n22=len(t22['bones'])
 for sample in row['blends']:
  i=sample['blend_index'];path=config['assets']/sample['cast_file'];rawpath=config['assets']/row['raw_file']
  source=wc.decode_source(path,rig,seq,qc,sample,rawpath);clip=wc.remap_clip(source,t22,key)
  poses=np.empty((clip['poses'].shape[0],len(data['bones']),7),'<f4');poses[:,:n22]=clip['poses'];poses[:,n22:]=rest[n22:] if not clip['additive'] else [0,0,0,0,0,0,1]
  weights=np.concatenate([np.asarray(clip['weights'],'<f4'),np.zeros((len(data['bones'])-n22,2),'<f4')])
  name=f'{key}_inspect_basic_{i}'
  record=wm.metadata(name,seq['sequence'],i,seq,source,path,config['qc'],qc);record.update(rig=key,sequence_occurrence=0,sequence_guid=row['guid'],sequence_asset=row['asset_path'],rseq_source=wm.info(rawpath))
  out.append(dict(clip,name=name,poses=poses,weights=weights,record=record))
 return out


def bake(out=None):
 out=wm.checked(out or wm.PACK_ROOT,wm.PACK_ROOT);pack,table,audit,carriers=generate();path=out/'fuse_pov.anim';ac.write_pack(path,pack)
 # read back: every T022 byte of the bone, carrier and clip headers kept, clips readable
 back=wm.read_pack(path);base=wm.read_pack(wm.BASE_PACK)
 wm.require(all(a['raw']==b['raw'] for a,b in zip(back['bones'],base['bones'])) and all(a['raw']==b['raw'] and a['group']==b['group'] for a,b in zip(back['carrier_records'],base['carrier_records'])),'T022 records changed')
 old=len(base['bones'])
 for n,c in base['clips'].items():wm.require(np.array_equal(back['clips'][n]['poses'][:,:old],c['poses']) and np.array_equal(back['clips'][n]['weights'][:old],c['weights']),f'T022 clip changed: {n}')
 audit['output']=wm.info(path);wm.save(out/'wingman-pack-audit.json',audit);wm.save(out/'wingman_carriers.json',carriers);wm.save(out/'wingman_sequences.json',table['wm']);wm.save(out/'r99_sequences.json',table['r9']);wm.save(out/'kunai_sequences.json',table['kn']);wm.save(out/'flatline_sequences.json',table['fl']);wm.save(out/'sentinel_sequences.json',table['sn'])
 print(f'PASS wingman bake: {len(pack["bones"])} bones / {len(pack["carrier_records"])} carriers / {len(pack["clips"])} clips; {path.stat().st_size} bytes',flush=True);return path


if __name__=='__main__':
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--out',type=Path);bake(p.parse_args().out)
