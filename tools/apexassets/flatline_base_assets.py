"""VK-47 Flatline (`mp_weapon_vinson`) Cast/raw/SMD/QC/textures and exact local provenance (the
Wingman's export): its default model (retail `flatline_base_v`, the weapon settings' viewmodel) on
the Flatline's view-model rig `ptpov_vinson.rrig` and its sequences. The Teal Zeal model is
flatline_assets.py (export-assets.ps1 -SkinModels)."""
import argparse
from pathlib import Path
import sys
sys.dont_write_bytecode=True
import weapons_assets_common as wc
OUT=wc.oa.REPO/'apex-data/assets/flatline_base'
# no earlier export of it: the comparison finds nothing and lists every file as added
PREVIOUS=wc.oa.REPO/'apex-data/weapons/flatline_base/pov'
if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--out',type=Path,default=OUT);p.add_argument('--analyze-only',action='store_true');a=p.parse_args()
    out=wc.checked(a.out,OUT)
    report=wc.analyze(out,PREVIOUS) if a.analyze_only else wc.export(out,[('flatline_viewmodel','d27afeb90ea34c30')],PREVIOUS,'b524a4f031ccf3f8')
    print('PASS flatline base export: '+wc.json.dumps(report,ensure_ascii=False),flush=True)
