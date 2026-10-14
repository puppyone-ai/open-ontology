#!/usr/bin/env python3
"""Operator installation of a fixed source-owned recipe, never a user prerequisite."""
import argparse
import sys
from onboard import install_engine, Refused, ENGINES

def main(argv=None):
    p=argparse.ArgumentParser()
    p.add_argument('--engine',required=True,choices=sorted(ENGINES))
    p.add_argument('--recipe',required=True)
    p.add_argument('--secret-registry',required=True)
    p.add_argument('--metadata-recipe')
    p.add_argument('--whole-run')
    a=p.parse_args(argv)
    try:
        record=install_engine(a.engine,a.recipe,a.secret_registry,a.metadata_recipe,a.whole_run)
    except (Refused,OSError,KeyError,TypeError):
        print('source-onboarding-register: refused',file=sys.stderr);return 2
    print('installed_engine',a.engine,'whole='+str(record['whole']).lower())
    return 0

if __name__=='__main__': raise SystemExit(main())
