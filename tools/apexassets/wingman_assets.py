"""Wingman Cast/raw/SMD/QC/textures and exact local provenance (the Charge Rifle's T022 export, for
`mp_weapon_wingman`: viewmodel `wingman_base_v.rmdl`, rig `wingman_base_v_animRig.rrig`)."""
import argparse
from pathlib import Path
import sys
sys.dont_write_bytecode=True
import weapons_assets_common as wc
OUT=wc.oa.REPO/'apex-data/assets/wingman'
# no earlier Wingman export: the comparison finds nothing and lists every file as added
PREVIOUS=wc.oa.REPO/'apex-data/weapons/wingman/pov'
if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--out',type=Path,default=OUT);p.add_argument('--analyze-only',action='store_true');a=p.parse_args()
    out=wc.checked(a.out,OUT)
    report=wc.analyze(out,PREVIOUS) if a.analyze_only else wc.export(out,[('wingman_viewmodel','4f5246307def57a6')],PREVIOUS,'352247e247ed6a85')
    print('PASS wingman export: '+wc.json.dumps(report,ensure_ascii=False),flush=True)
