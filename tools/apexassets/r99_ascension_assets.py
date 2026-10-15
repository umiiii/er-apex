"""R-99 Cutting Edge (reactive `r99_react_v20_ascension_v`, its own rig) Cast/raw/SMD/QC/textures and exact local provenance (the Wingman's export, for
`mp_weapon_r97`: viewmodel `r99_base_v.rmdl`, rig `r99_base_v_animRig.rrig`)."""
import argparse
from pathlib import Path
import sys
sys.dont_write_bytecode=True
import weapons_assets_common as wc
OUT=wc.oa.REPO/'apex-data/assets/r99_ascension'
# no earlier R-99 export: the comparison finds nothing and lists every file as added
PREVIOUS=wc.oa.REPO/'apex-data/weapons/r99_ascension/pov'
if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--out',type=Path,default=OUT);p.add_argument('--analyze-only',action='store_true');a=p.parse_args()
    out=wc.checked(a.out,OUT)
    report=wc.analyze(out,PREVIOUS) if a.analyze_only else wc.export(out,[('r99_viewmodel','31039f4bbc293dcd')],PREVIOUS,'3e9f776082320984')
    print('PASS r99 ascension export: '+wc.json.dumps(report,ensure_ascii=False),flush=True)
