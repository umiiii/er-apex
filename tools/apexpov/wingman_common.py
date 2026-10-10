"""Wingman (replaces the R-301 in the hands): T022's Charge Rifle recipe for `wingman_base_v`, one more
independent weapon branch `wm:` and carrier group 9 on part BD, appended to T022's pack and 998."""
from __future__ import annotations
import json
import os
from pathlib import Path
import re
import sys
sys.dont_write_bytecode=True
import numpy as np
import weapons_common as wc
from weapons_common import ac, bp, require, checked, save, info, model, model_world, world7, metadata, live_skeleton, read_qc
ROOT=wc.ROOT
PACK_ROOT=ROOT/'apex-data/pov/octane_wingman'
MODEL_ROOT=ROOT/'er-data/s3/octane_pov_wingman'
BASE_PACK=wc.PACK_ROOT/'fuse_pov.anim'
BASE_MODEL=wc.MODEL_ROOT
# The default models (retail `r99_base_v`, `flatline_base_v`: every Apex install has them) unless
# ERAPEX_SKIN_MODELS=1 (export-assets.ps1 -SkinModels): then the R-99's Cutting Edge and the
# Flatline's Teal Zeal, the user's 2026-10-09 picks, which `--r99-skin` / `--flatline-skin` texture.
SKIN_MODELS=os.environ.get('ERAPEX_SKIN_MODELS','')=='1'


def _config(key,assets,part,group,model_relative,rig_relative,stem,meshes,owners,carriers,note):
 c=dict(key=key,assets=ROOT/assets,part=part,group=group,model_relative=model_relative,rig_relative=rig_relative,stem=stem,meshes=meshes,owners=owners,carriers=carriers,note=note)
 c.update(model=c['assets']/('cast/'+model_relative+'_LOD0.cast'),rig=c['assets']/('cast/'+rig_relative+'.cast'),qc=c['assets']/('smd/'+rig_relative+'.qc'),model_qc=c['assets']/('smd/'+model_relative+'.qc'),metadata=c['assets']/('sequences/'+stem+'.json'))
 return c
# Each weapon: its default meshes (studio 0, and the iron sights when they are an option alone), its
# carriers: its weighted bones that move most on live leaf bones enabled in the part's template that
# no earlier carrier uses (nobody's parent: see T022's Claude rework on L_Forearm/R_Forearm); its other
# weighted bones ride on their ancestors (rigid).
CONFIGS={
 # the Wingman on part BD (its only three free leaves): the frame, taclight (2.2 cm in the reloads,
 # its child taclight1 spins 180 deg), the magazine (the cylinder's speed loader: 36 cm); detailB
 # (the hammer, 45 deg in the fire), toprail (1.7 cm), detailC (2.2 cm) and trigger ride on them
 'wm':_config('wm','apex-data/assets/wingman','bd',9,'mdl/techart/mshop/weapons/class/pistol/wingman/wingman_base_v','animrig/techart/mshop/weapons/class/pistol/wingman/wingman_base_v_animRig','wingman_base_v',('body_0_','b3wing_magazine_0_'),('def_c_base','def_c_taclight','def_c_magazine'),('R_ThighTwist','R_Hip','L_Hip'),'Wingman'),
 # the R-99 on part LG (its twist leaves): its Cutting Edge model (the reactive `r99_react_v20_ascension_v`,
 # the user's skin's, 2026-10-09) on the base rig's sequences: the frame, the magazine (the `clip`
 # bodygroup), detailD; detailC, the iron sights and the reactive fins (`def_lb_fin_*`, bones of the
 # reactive rig only) ride on them
 'r9':_config('r9','apex-data/assets/r99_ascension','lg',10,'mdl/techart/mshop/weapons/class/smg/r99/r99_react_v20_ascension_v','animrig/techart/mshop/weapons/class/smg/r99/r99_base_v_animRig','r99_react_v20_ascension_v',('body_0_','clip_0_','sight_front_1_','sight_rear_1_'),('def_c_base','def_c_magazine','def_c_detailD'),('L_ThighTwist1','L_CalfTwist1','R_ThighTwist1'),'R-99') if SKIN_MODELS else
     _config('r9','apex-data/assets/r99','lg',10,'mdl/techart/mshop/weapons/class/smg/r99/r99_base_v','animrig/techart/mshop/weapons/class/smg/r99/r99_base_v_animRig','r99_base_v',('body_0_','clip_0_','sight_front_1_','sight_rear_1_'),('def_c_base','def_c_magazine','def_c_detailD'),('L_ThighTwist1','L_CalfTwist1','R_ThighTwist1'),'R-99'),
 # Wraith's heirloom kunai (the holstered mode, 2026-10-09) on part LG's next free twist leaf: one
 # mesh, all of it on def_magazine (the knife; knife_base, its child, is not weighted)
 # the VK-47 Flatline (2026-10-09) on part LG's next free leaves: its Teal Zeal model (the legendary
 # latline_v20_trshunter_v) on the Flatline's rig ptpov_vinson: the frame, the magazine, the
 # top cover (def_front_top: the charging handle side); the release and the barrel ride on them
 'fl':_config('fl','apex-data/assets/flatline','lg',12,'mdl/techart/mshop/weapons/class/assault/flatline/flatline_v20_trshunter_v','animrig/weapons/vinson/ptpov_vinson','flatline_v20_trshunter_v',('body_0_','sight_rear_on_1_','_0_'),('def_c_base','def_magazine','def_front_top'),('L_ThighTwist','L_CalfTwist','R_CalfTwist'),'Flatline') if SKIN_MODELS else
     _config('fl','apex-data/assets/flatline_base','lg',12,'mdl/techart/mshop/weapons/class/assault/flatline/flatline_base_v','animrig/weapons/vinson/ptpov_vinson','flatline_base_v',('body_0_','magazine_0_','sight_rear_on_1_'),('def_c_base','def_magazine','def_front_top'),('L_ThighTwist','L_CalfTwist','R_CalfTwist'),'Flatline'),
 # the Sentinel (2026-10-09) on part LG's last free leaves (the feet's: the controller writes them in
 # model space, the game's foot IK under them does not reach them): its base model, the frame, the
 # bolt, the magazine; the bullet, the second magazine, its clip and the charge bolts ride on them
 'sn':_config('sn','apex-data/assets/sentinel','lg',13,'mdl/techart/mshop/weapons/class/sniper/sentinel/sentinel_base_v','animrig/techart/mshop/weapons/class/sniper/sentinel/sentinel_base_v_animRig','sentinel_base_v',('MAINBODY_0_','sight_front_1_'),('def_c_base','def_c_boltA','def_c_magazineA'),('L_FootTwist','R_FootTwist','L_Toe0'),'Sentinel'),
 'kn':_config('kn','apex-data/assets/kunai','lg',11,'mdl/techart/mshop/weapons/class/heirloom/wraith/v18_kunai/heirloom_wraith_v18_kunai_v','animrig/techart/mshop/weapons/class/heirloom/wraith/v18_kunai/heirloom_wraith_v18_kunai_v_animRig','heirloom_wraith_v18_kunai_v',('projectile_0_',),('def_magazine',),('R_CalfTwist1',),'kunai'),
}
def _braced_qc(c):
 """The rig QC with a block on every `$sequence` (the R-99's has some on one line, which the shared
 QC reader does not take): a copy beside the original, which stays as RSX wrote it."""
 text=c['qc'].read_text(encoding='utf-8-sig')
 fixed=re.sub(r'^(\$sequence\s+"[^"]+"\s+"[^"]+")\s*$',r'\1 {\n}',text,flags=re.M)
 if fixed!=text:
  out=c['qc'].with_name(c['qc'].stem+'_braced.qc')
  if not out.is_file() or out.read_text(encoding='utf-8')!=fixed:out.write_text(fixed,encoding='utf-8')
  c['qc']=out
for _c in CONFIGS.values():_braced_qc(_c)
KEYS=tuple(CONFIGS)
GROUP=max(c['group'] for c in CONFIGS.values())
# the Wingman's names, as the scripts written for it alone use them
KEY='wm'
CONFIG=CONFIGS[KEY]
BASE_COUNTS=(170,87,301)
# T022's Charge Rifle exclusions (attachments, optics, wallrun, inspect, sprint extras) plus the TEMP/late ones
EXCLUDED=wc.EXCLUDED_CR


def bodygroups(key=KEY):
 text=CONFIGS[key]['model_qc'].read_text(encoding='utf-8-sig');groups=[]
 for m in re.finditer(r'\$bodygroup\s+"([^"]+)"\s*\{([^}]+)\}',text):
  options=[s or 'blank' for s,_ in re.findall(r'\bstudio\s+"([^"]+)"|\b(blank)\b',m[2])]
  groups.append(dict(name=m[1],options=options,selected_index=None,note='attachment: not shown'))
 for name,studio in re.findall(r'\$body\s+"([^"]+)"\s+"([^"]+)"',text):groups.append(dict(name=name,options=[studio],selected_index=0,qc_first_index=0))
 return groups


def selected(key=KEY):
 c=CONFIGS[key];mdl=model(c['model']);keep=[];omitted=[]
 for mesh in mdl.Meshes():
  row=dict(name=mesh.Name(),vertices=mesh.VertexCount(),triangles=len(mesh.FaceBuffer())//3,material=mesh.Material().Name())
  if mesh.Name().startswith(c['meshes']):keep.append(mesh)
  else:omitted.append(dict(row,reason=f'{c["note"]}: attachment bodygroup, not shown'))
 # every default prefix found (the Sentinel's MAINBODY has two meshes: its frame and its charge lights)
 require(all(any(m.Name().startswith(p) for m in keep) for p in c['meshes']) and len(keep)>=len(c['meshes']),f'Unexpected {c["note"]} default mesh count');return mdl,keep,omitted

def read_pack(path):
 """weapons_common.read_pack with group 9 allowed."""
 data=Path(path).read_bytes();r=wc.Reader(data)
 require(r.take(4)==b'FPOV','Not FPOV');version,nb=r.unpack('<II');require(version==2 and 0<nb<=512,'Expected FPOV v2')
 bones=[]
 for i in range(nb):
  start=r.offset;name,parent,rest=r.name(),r.unpack('<h')[0],r.unpack('<7f');require(-1<=parent<i,f'Invalid parent: {name}');bones.append(dict(name=name,parent=parent,rest=rest,raw=data[start:r.offset]))
 camera,nc=r.unpack('<II');require(camera<nb and nc<=256,'Invalid camera/carrier count');carriers=[]
 for _ in range(nc):
  start=r.offset;name,owner=r.name(),r.unpack('<I')[0];inverse,er=r.unpack('<7f'),r.unpack('<7f');raw=data[start:r.offset];group=r.unpack('<B')[0]
  require(owner<nb and group<=GROUP,f'Invalid carrier: {name}');carriers.append(dict(name=name,owner=owner,inverse_mesh_bind=inverse,er_bind=er,group=group,raw=raw))
 clips={};count=r.unpack('<I')[0];require(0<count<=4096,'Invalid clip count')
 for _ in range(count):
  start=r.offset;name=r.name();fps,frames,loop,additive=r.unpack('<fIBB');header=data[start:r.offset]
  weights=np.frombuffer(r.take(nb*8),'<f4').reshape(nb,2);poses=np.frombuffer(r.take(frames*nb*28),'<f4').reshape(frames,nb,7)
  require(name not in clips and np.isfinite(poses).all(),'Invalid clip data');clips[name]=dict(name=name,fps=fps,frames=frames,loop=bool(loop),additive=bool(additive),weights=weights,poses=poses,header=header)
 require(r.offset==len(data),'Trailing bytes')
 return dict(path=str(path),data=data,version=version,bones=bones,camera=camera,carrier_records=carriers,clips=clips)


def part_binds(part):
 """The part's node binds (world): BD and HD as T022 reads them; LG from T020's build, whose live
 template it is."""
 if part!='lg':return wc.carrier_binds(part)
 nodes=json.loads((ac.MODEL_ROOT/'readback/export_lg/flver.json').read_text(encoding='utf8'))['Nodes']
 return wc.node_world(nodes)


def layout(base=None):
 base=base or read_pack(BASE_PACK);require((len(base['bones']),len(base['carrier_records']),len(base['clips']))==BASE_COUNTS,'Unexpected T022 baseline')
 bones=[dict(b) for b in base['bones']];old=len(bones);added=[];diffs=[];new=[];fallback=[];omitted=[];sources={};maps={};groups={};totals={}
 for key,c in CONFIGS.items():
  pack_index={b['name']:i for i,b in enumerate(bones)}
  rig=bp.skeleton(c['rig']);idx={b['name']:i for i,b in enumerate(rig)};sources[key]=rig
  mdl,meshes,skip=selected(key);omitted+=skip;groups[key]=bodygroups(key);weights={}
  # a model bone the rig lacks (the reactive fins) counts as its nearest ancestor the rig has
  mbones=mdl.Skeleton().Bones();alias={}
  def in_rig(i):
   j=i
   while j>=0 and mbones[j].Name() not in idx:j=mbones[j].ParentIndex()
   require(j>=0,f'No rig ancestor: {mbones[i].Name()}');return mbones[j].Name()
  for mesh in meshes:
   for b,w in zip(mesh.VertexWeightBoneBuffer(),mesh.VertexWeightValueBuffer()):
    if w>0:
     name=mbones[b].Name()
     if name not in idx:alias[name]=in_rig(b);name=alias[name]
     weights[name]=weights.get(name,0.)+w
  totals[key]=weights
  prop=set(weights)
  for n in list(prop):
   require(n in idx,f'Weighted bone missing from rig: {n}');p=rig[idx[n]]['parent']
   while p>=0:
    name=rig[p]['name']
    if name in pack_index and sum(b['parent']==p for b in rig)>1 and name=='def_c_spineC':break
    prop.add(name);p=rig[p]['parent']
  # the rig's own other bones (the Sentinel's shield clamps on the left forearm) are copied up to
  # the first bone the pack has: walking on took the left arm into the copy, and the arms' left
  # arm was left at rest (the user, 2026-10-09: "the Sentinel has no left hand")
  for b in rig:
   n=b['name']
   if n in pack_index or n in prop:continue
   prop.add(n);p=b['parent']
   while p>=0 and rig[p]['name'] not in pack_index and rig[p]['name'] not in prop:
    prop.add(rig[p]['name']);p=rig[p]['parent']
  # the R-301's own bones in the pack: a weapon's same-named ones are copies, never mapped onto the rifle
  masked_r301={'def_c_bolt','def_c_magazine','def_dust_cover_l','ja_ads_attachment','def_c_trigger','def_c_detailA','def_c_detailB','def_c_detailC','def_c_detailD','def_c_base','weapon_bone'}
  mapping={n:pack_index[n] for n in idx if n in pack_index and n not in prop and n not in masked_r301};rest=bp.ea.rest_pose(rig)
  for i,b in enumerate(rig):
   n=b['name']
   if n not in prop and n not in masked_r301:continue
   if n in mapping:continue
   output=key+':'+n;parentname=rig[b['parent']]['name'] if b['parent']>=0 else None
   require(parentname is None or parentname in mapping,f'Parent not placed before child: {n}')
   parent=mapping[parentname] if parentname else -1
   mapping[n]=len(bones);bones.append(dict(name=output,parent=parent,rest=tuple(rest[i,:7])))
   added.append(dict(index=mapping[n],name=output,source_bone=n,parent=bones[parent]['name'] if parent>=0 else None,rig=key,copied=n in pack_index,positive_mesh_weight=n in weights))
  for b in base['bones']:
   if b['name'] not in idx:continue
   p=rig[idx[b['name']]]['parent'];sp=rig[p]['name'] if p>=0 else None;pp=base['bones'][b['parent']]['name'] if b['parent']>=0 else None
   if sp!=pp:diffs.append(dict(rig=key,bone=b['name'],pack_parent=pp,source_parent=sp,copied_for_rig=b['name'] in prop or b['name'] in masked_r301))
  by_source={};binds=part_binds(c['part']);meshbind=model_world(mdl)
  for owner,name in zip(c['owners'],c['carriers']):
   require(owner in weights and mapping[owner]>=old,f'Invalid carrier owner: {key}/{owner}')
   by_source[owner]=name;new.append(dict(name=name,owner=mapping[owner],owner_name=bones[mapping[owner]]['name'],source_bone=owner,rig=key,part=c['part'],group=c['group'],total_vertex_weight=weights[owner],inverse_mesh_bind=bp.rigid(np.linalg.inv(meshbind[owner]),owner).tolist(),er_bind=bp.rigid(binds[name],name).tolist()))
  for n in weights:
   if n in by_source:continue
   p=rig[idx[n]]['parent']
   while p>=0 and rig[p]['name'] not in by_source:p=rig[p]['parent']
   require(p>=0,'No ancestor carrier');ancestor=rig[p]['name'];by_source[n]=by_source[ancestor]
   fallback.append(dict(rig=key,source_bone=n,ancestor=ancestor,carrier=by_source[n],total_vertex_weight=weights[n]))
  for n,a in alias.items():
   by_source[n]=by_source[a];fallback.append(dict(rig=key,source_bone=n,ancestor=a,carrier=by_source[a],total_vertex_weight=None,note='model bone absent from the rig'))
  for carrier in new:
   if carrier['rig']==key:carrier['mapped_total_vertex_weight']=sum(weights.get(n,0.) for n,cname in by_source.items() if cname==carrier['name'])
  maps[key]=mapping;maps[key+'-carriers']=by_source
 require(len({c['name'] for c in base['carrier_records']+new})==BASE_COUNTS[1]+len(new),'Carrier collision')
 live={b['name'] for b in live_skeleton()['bones']};require(all(c['name'] in live for c in new),'Carrier absent from live')
 return dict(bones=bones,sources=sources,maps=maps,added=added,parent_differences=diffs,new_carriers=new,carrier_records=[dict(c) for c in base['carrier_records']]+new,ancestor_fallbacks=fallback,bodygroups=groups,omitted_meshes=omitted,weight_totals=totals)


def remap_clip(source,data,key=KEY):
 active=[d for d in data['parent_differences'] if not d['copied_for_rig'] and d['rig']==key]
 return ac.remap_clip(source,dict(data,parent_differences=active),key)


def sequence_rows(qc,key=KEY):
 c=CONFIGS[key];local=json.loads(c['metadata'].read_text(encoding='utf8'))['sequences'];seen={};rows=[];excluded=[]
 for name,seq in qc['sequences'].items():
  sequence=seq['sequence'];occurrence=seq['occurrence']
  if sequence in EXCLUDED or any(t in sequence for t in ('_sniper','_TEMP','_late')):excluded.append(dict(sequence=sequence,occurrence=occurrence));continue
  files=[Path(qc['animations'][a]['file']).stem for a in seq['sample_animation_names']]
  candidates=[s for s in local if s['name']==sequence and s['blend_count']==len(files) and [b['name'] for b in s['blends']]==files and
    (not seq['activity'] or (s['activity']==seq['activity']['name'] and s['activity_weight']==seq['activity']['weight']))]
  require(bool(candidates),f'QC has no matching RSEQ: {key}/{name}/{files}')
  candidates.sort(key=lambda s:s['asset_path']);prior=seen.get(sequence,0)
  s=candidates[min(prior,len(candidates)-1)] if len(candidates)>1 else candidates[0];seen[sequence]=prior+1
  rows.append((name,seq,s))
 return rows,excluded