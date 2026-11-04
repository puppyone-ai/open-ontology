#!/usr/bin/env python3
"""Live retained-proof grant plus a fail-closed data-only Builder boundary probe."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import sys

import lifecycle

ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'tools/pg-builder/scripts'))
import library_semantics


def builder_boundary(cut):
    observation={'protocol':'oo.builder.oracle-source-observation/v1','source_bound':None,
        'source_semantics':{'measurement_protocol':'oo.builder.oracle-target-observation/v1',
            'publication_receipt':cut['receipt']['shards'][0]['receipt']}}
    result={}
    for name,call in [
        ('oracle_retained',lambda:library_semantics.oracle_data_only(observation)),
        ('postgres_route',lambda:library_semantics.data_only_digest({
            'tables':[{'source_engine':'postgres'}]})),
    ]:
        try:
            call()
        except ValueError as error:
            result[name]=str(error)
        else:
            raise AssertionError('boundary changed; rerun the complete lite cycle')
    assert result=={'oracle_retained':'data_only_oracle_c1_required',
                    'postgres_route':'data_only_source_route_required'},result
    return result


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--run-id',required=True)
    parser.add_argument('--oracle-cut',type=Path,required=True)
    args=parser.parse_args()
    assert args.run_id and all(c.isalnum() or c=='-' for c in args.run_id)
    os.umask(0o077)
    out=ROOT/'evidence'/args.run_id
    out.mkdir(mode=0o700)
    spec=importlib.util.spec_from_file_location('live_cycles',ROOT/'tools/oo-load/test-gateway-cycles.py')
    module=importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    live=module.Live(out)
    report={'status':'failed','seven_healthy':False,'scheduled_cycle':'not_run'}
    try:
        cut=json.loads(args.oracle_cut.read_text())
        report['builder_blockers']=builder_boundary(cut)
        report['oracle_cut_sha256']=hashlib.sha256(args.oracle_cut.read_bytes()).hexdigest()
        report['boundary_basis']='actual Oracle retained receipt; minimal route-dispatch inputs, not a full Builder run'
        live.start()
        assert live.http()['status']==200
        live.db.sql('REVOKE EXECUTE ON FUNCTION _oo.native_library_retained_table(text) FROM oo_f22_login;')
        refused=live.http()
        assert refused['status']!=200,refused
        live.db.sql(lifecycle.retained_login_grant('oo_f22_login'))
        granted=live.http()
        assert granted['status']==200 and granted['body']['rows']==[['10']],granted
        report.update(status='grant_passed_cycle_blocked',before_grant=refused,after_grant=granted)
    finally:
        live.close()
        module.write_json(out/'boundary-result.json',report)
        module.checksums(out)


if __name__=='__main__':
    main()
