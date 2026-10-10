"""Wraith's heirloom kunai Cast/raw/SMD/QC/textures and exact local provenance (the Wingman's export,
for the holstered mode: viewmodel `heirloom_wraith_v18_kunai_v.rmdl`, rig
`heirloom_wraith_v18_kunai_v_animRig.rrig`)."""
import argparse
from pathlib import Path
import sys
sys.dont_write_bytecode=True
import weapons_assets_common as wc
OUT=wc.oa.REPO/'apex-data/assets/kunai'
# no earlier kunai export: the comparison finds nothing and lists every file as added
PREVIOUS=wc.oa.REPO/'apex-data/weapons/kunai/pov'
if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--out',type=Path,default=OUT);p.add_argument('--analyze-only',action='store_true');a=p.parse_args()
    out=wc.checked(a.out,OUT)
    report=wc.analyze(out,PREVIOUS) if a.analyze_only else wc.export(out,[('kunai_viewmodel','01c5d6cb2c60253e')],PREVIOUS,'2b6070a5a637a395')
    print('PASS kunai export: '+wc.json.dumps(report,ensure_ascii=False),flush=True)
