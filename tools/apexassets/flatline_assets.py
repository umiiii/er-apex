"""VK-47 Flatline (`mp_weapon_vinson`) Cast/raw/SMD/QC/textures and exact local provenance (the
Wingman's export): its Teal Zeal model (the legendary `flatline_v20_trshunter_v`, the user's
2026-10-09 pick) on the Flatline's view-model rig `ptpov_vinson.rrig` and its sequences."""
import argparse
from pathlib import Path
import sys
sys.dont_write_bytecode=True
import weapons_assets_common as wc
OUT=wc.oa.REPO/'apex-data/assets/flatline'
# no earlier Flatline export: the comparison finds nothing and lists every file as added
PREVIOUS=wc.oa.REPO/'apex-data/weapons/flatline/pov'
if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--out',type=Path,default=OUT);p.add_argument('--analyze-only',action='store_true');a=p.parse_args()
    out=wc.checked(a.out,OUT)
    report=wc.analyze(out,PREVIOUS) if a.analyze_only else wc.export(out,[('flatline_viewmodel','ddfb463ed23173ef')],PREVIOUS,'b524a4f031ccf3f8')
    print('PASS flatline export: '+wc.json.dumps(report,ensure_ascii=False),flush=True)
