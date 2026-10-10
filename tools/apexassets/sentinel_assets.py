"""Sentinel (`mp_weapon_sentinel`) Cast/raw/SMD/QC/textures and exact local provenance (the
Wingman's export): its view model `sentinel_base_v` on `sentinel_base_v_animRig.rrig` and its
sequences."""
import argparse
from pathlib import Path
import sys
sys.dont_write_bytecode=True
import weapons_assets_common as wc
OUT=wc.oa.REPO/'apex-data/assets/sentinel'
# no earlier Sentinel export: the comparison finds nothing and lists every file as added
PREVIOUS=wc.oa.REPO/'apex-data/weapons/sentinel/pov'
if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--out',type=Path,default=OUT);p.add_argument('--analyze-only',action='store_true');a=p.parse_args()
    out=wc.checked(a.out,OUT)
    report=wc.analyze(out,PREVIOUS) if a.analyze_only else wc.export(out,[('sentinel_viewmodel','5bd888e21ee84b87')],PREVIOUS,'9ccf987198ed01ab')
    print('PASS sentinel export: '+wc.json.dumps(report,ensure_ascii=False),flush=True)
