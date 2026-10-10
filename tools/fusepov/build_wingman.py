"""Full 998: T022's package with the Wingman appended to BD and the R-99 to LG; AM/HD bytes kept."""
import argparse
import shutil
import sys
import time
from pathlib import Path
sys.dont_write_bytecode=True
from common import PARTS,MODELS,TOOL,EXTRACT,run,read,save
from mesh_wingman import wm,convert,PARTS as WEAPON_PARTS
from material_bundle import helper,merge
sys.path.insert(1,str(Path(__file__).resolve().parents[1]/'apexpov'))
from bake_wingman import generate
DEFAULT_MATBIN=wm.BASE_MODEL/'package/material/allmaterial.matbinbnd.dcx'
KEPT=tuple(p for p in ('am','bd','hd','lg') if p not in WEAPON_PARTS)


def build(out=None,matbin_bnd=DEFAULT_MATBIN):
 out=wm.checked(out or wm.MODEL_ROOT,wm.MODEL_ROOT);start=time.monotonic();source=Path(matbin_bnd).resolve()
 for p in (source,TOOL,EXTRACT,wm.BASE_PACK):
  if not p.is_file():raise FileNotFoundError(p)
 canonical=wm.PACK_ROOT/'fuse_pov.anim'
 if not canonical.is_file():raise FileNotFoundError(f'{canonical}: run tools/apexpov/bake_wingman.py first')
 summary=convert(out)
 snapshot=out/'inputs/material/allmaterial.matbinbnd.dcx';snapshot.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(source,snapshot);wm.require(snapshot.read_bytes()==source.read_bytes(),'Material snapshot changed')
 save(out/'inputs/source-material.json',dict(source=str(source),size=source.stat().st_size))
 names=[f'P[{part.upper()}_M_0998]_'+m['name'] for part in WEAPON_PARTS for m in read(out/f'fusemesh/{part}/mesh.json')['materials']]
 targets=out/'inputs/material-targets.json';save(targets,names);staged=snapshot.with_name('build-input.matbinbnd.dcx');helper(out,'prepare',snapshot,staged,targets)
 command=[TOOL,'build-armor','--model','998','--matbin-bnd',staged,'--out',out/'package']
 for part in PARTS:
  template=wm.ac.BASE_MODEL/f'inputs/parts/{part}_m_{MODELS[part]:04d}.partsbnd.dcx'
  if part=='hd':template=wm.BASE_MODEL/'inputs/live-templates/hd_m_1280.partsbnd.dcx'
  if part=='lg':template=wm.wc.bc.BASE_MODEL/'inputs/live-templates/lg_m_1280.partsbnd.dcx'
  command += [f'--{part}-template',template]
  if part in WEAPON_PARTS:command += [f'--{part}-mesh',out/f'fusemesh/{part}']
 run(command,out,'build_wingman_package');merge(out,snapshot)
 # the other parts exactly as T022 built them
 for part in KEPT:
  for lod in ('','_l'):
   name=f'{part}_m_0998{lod}.partsbnd.dcx';shutil.copyfile(wm.BASE_MODEL/'package/parts'/name,out/'package/parts'/name)
 manifest=read(out/'package/build-manifest.json');base=read(wm.BASE_MODEL/'package/build-manifest.json')
 manifest.update(task='wingman',base_package=str(wm.BASE_MODEL/'package'),animation_pack=str(canonical),preserved_parts=[p+l for p in KEPT for l in ('','_l')],parts=[p for p in manifest['parts'] if p['piece'] in WEAPON_PARTS]+[dict(p,copied_byte_identical=True) for p in base['parts'] if p['piece'] in KEPT])
 manifest['meshes']=base['meshes']+len(summary['meshes']);manifest['vertices']=base['vertices']+sum(m['vertices'] for m in summary['meshes'])
 manifest['textures']=[t for t in base['textures'] if not t['name'].startswith(tuple(f'{p.upper()}_M_0998_' for p in WEAPON_PARTS))]+manifest['textures'];save(out/'package/build-manifest.json',manifest)
 report=verify(out);report['elapsed_seconds']=round(time.monotonic()-start,2);save(out/'wingman-verification.json',report)
 print(f'PASS wingman build: full 998; {"/".join(p.upper() for p in WEAPON_PARTS)} + weapons; {", ".join(p.upper() for p in KEPT)} byte-identical; {report["elapsed_seconds"]}s',flush=True);return report


def verify(out):
 """Read the weapons' parts back: their meshes, the weapons' carriers in their skeletons; the other parts unchanged."""
 from verify_fuse import read_json_command
 result=dict(status='PASS',parts={})
 for part in WEAPON_PARTS:
  rb=out/f'readback/export_{part}';rb.mkdir(parents=True,exist_ok=True)
  run([EXTRACT,'unpack',out/f'package/parts/{part}_m_0998.partsbnd.dcx','--out',rb/'unpacked'],out,f'wingman_unpack_{part}')
  flver=next((rb/'unpacked').rglob('*.flver'));dump=read_json_command(run,[TOOL,'flver',flver,'--samples','0'],out,f'wingman_flver_{part}');save(rb/'flver.json',dump)
  nodes=[n['Name'] for n in dump['Nodes']];doc=read(out/f'fusemesh/{part}/mesh.json')
  wm.require(len(dump['Meshes'])==len(doc['submeshes']),f'{part} mesh count {len(dump["Meshes"])} != {len(doc["submeshes"])}')
  carriers=[n for c in wm.CONFIGS.values() if c['part']==part for n in c['carriers']]
  wm.require(all(c in nodes for c in carriers),f'Weapon carrier missing from {part} nodes')
  result['parts'][part]=dict(meshes=len(dump['Meshes']),nodes=len(nodes),carriers=carriers)
 for part in KEPT:
  for lod in ('','_l'):
   name=f'{part}_m_0998{lod}.partsbnd.dcx';wm.require((out/'package/parts'/name).read_bytes()==(wm.BASE_MODEL/'package/parts'/name).read_bytes(),f'{name} changed')
 return result

if __name__=='__main__':
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--out',type=Path);p.add_argument('--matbin-bnd',type=Path,default=DEFAULT_MATBIN)
 p.add_argument('--skin',type=Path,help='folder with Wingman_Default_col (and _spc, _nml, _gls) .png or .dds: the base material\'s textures replaced')
 p.add_argument('--r99-skin',type=Path,help='folder with <size> COL SPC.dds: the R-99''s main material''s albedo and specular replaced')
 p.add_argument('--cr-skin',type=Path,help='folder with col/, nml/, gls/ <size>.dds: the Charge Rifle\'s main material replaced')
 p.add_argument('--flatline-skin',type=Path,help='folder with *COL*.dds and *SPC*.dds (the largest used): the Flatline\'s Teal Zeal albedo and specular replaced')
 p.add_argument('--kunai-skin',type=Path,help='folder with *_col.dds and *_spc.dds (any subfolders, the largest used): the kunai\'s textures replaced')
 a=p.parse_args()
 import mesh_wingman;mesh_wingman.SKIN['dir']=a.skin;mesh_wingman.CR_SKIN['dir']=a.cr_skin;mesh_wingman.R99_SKIN['dir']=a.r99_skin;mesh_wingman.KUNAI_SKIN['dir']=a.kunai_skin;mesh_wingman.FLATLINE_SKIN['dir']=a.flatline_skin
 build(a.out,a.matbin_bnd)
