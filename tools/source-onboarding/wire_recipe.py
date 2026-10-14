#!/usr/bin/env python3
"""Write a private whole recipe that runs a source-owned script inside onboard.

The script must spawn this tree's product_flow.py and dialogue.py during the
step. Copied receipts are not accepted. This file does not copy source code.
"""
import argparse
import os
import stat
import sys
from pathlib import Path
from onboard import DIALOGUE, ENGINES, PRODUCT_FLOW, Refused, digest, private, require, save

def owned_program(path):
    p = Path(path)
    st = p.lstat()
    require(p.is_absolute() and stat.S_ISREG(st.st_mode) and not stat.S_ISLNK(st.st_mode) and st.st_uid == os.getuid(), 'program_path')
    return str(p.resolve())

def build(engine, script, template):
    script = owned_program(script)
    template = str(private(template))
    pins = [dict(path=path, sha256=digest(path)) for path in (sys.executable, script, str(PRODUCT_FLOW), str(DIALOGUE))]
    return {'protocol':'oo.source-onboarding.recipe/v1','engine':engine,'owned_fixture':True,'mode':'whole_source',
            'completion_artifact':'whole.json','config_template':template,
            'connection_map':{f'/{k}':k for k in ('host','port','database','user','secret_ref','password_file')},
            'pins':pins,'steps':[{'name':'whole','deadline_seconds':3600,
                'argv':[sys.executable, script, '{config}', '{run}'],
                'artifacts':[{'path':path} for path in ('whole.json','product/product-receipt.json','dialogue/verdict.json','dialogue/transcript.json')]}]}

def main(argv=None):
    p = argparse.ArgumentParser()
    p.add_argument('--engine', required=True, choices=sorted(ENGINES))
    p.add_argument('--script', required=True)
    p.add_argument('--template', required=True)
    p.add_argument('--out', required=True)
    a = p.parse_args(argv)
    out = Path(a.out)
    if out.exists():
        print('wire-recipe: refused', file=sys.stderr); return 2
    try:
        save(out, build(a.engine, a.script, a.template))
    except (Refused, OSError):
        print('wire-recipe: refused', file=sys.stderr); return 2
    print('wired', a.engine)
    return 0

if __name__ == '__main__':
    raise SystemExit(main())
