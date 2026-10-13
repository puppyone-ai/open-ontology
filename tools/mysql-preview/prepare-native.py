#!/usr/bin/env python3
"""Prepare input witnesses for the real admission sealer; emits no trust receipt.

Run in the same filesystem namespace as capture and loader. No credentials are read.
"""
import argparse
import hashlib
import json
from pathlib import Path

def write(path, value):
    path.write_text(json.dumps(value, indent=2) + '\n')

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--capture', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--secret-ref', required=True)
    args = parser.parse_args()
    capture = args.capture.absolute()
    facts = json.loads((capture / 'capturer.json').read_text())
    descriptor = json.loads((capture / 'driver.json').read_text())
    driver_path = Path(descriptor['executable_path'])
    driver_digest = hashlib.sha256(driver_path.read_bytes()).hexdigest()
    if driver_digest != descriptor['executable_sha256']:
        raise SystemExit('executed runtime driver changed after capture')
    args.out.mkdir(parents=True, exist_ok=False)
    for table in facts['tables']:
        dest = args.out / table['name']
        dest.mkdir()
        witness_path = dest / 'witness.json'
        witness = dict(protocol='oo.source-admission.capture-witness/v1', source_engine='mysql',
            source_id=facts['source_id'], source_generation=facts['source_generation'],
            snapshot=facts['snapshot'], run_id=facts['run_id'], logical_schema=table['schema'],
            logical_table=table['name'], capturer_path=str(capture/'capturer.json'),
            arrow_path=table.get('arrow_path', str(capture/table['arrow'])), driver_path=str(driver_path),
            schema_preimage_path=str(capture/'schema-preimage.json'),
            type_preimage_path=str(capture/'type-preimage.json'))
        write(witness_path, witness)
        permissions = [{'name':'SELECT','status':'known','granted':True,
                        'scope':{'database':table['schema'],'table':table['name']}}]
        shared = dict(target_id=facts['source_id'], source_id=facts['source_id'],
                      secret_ref=args.secret_ref, compat_mode='mysql-native',permissions=permissions,r1w=None)
        report = dict(shared, protocol='oo.source-admission.report/v1',
            driver={'kind':'native','name':'mysql_async','version':'0.36.1','digest':driver_digest},
            license_flags=['GPL-2.0'], capabilities=[dict(name='BatchFull',cursor=None,feed=None,
                cdc=None,token_order=None,budget_unit='Rows',level='experimental',write_class='R0',consistency='S1')],
            capture_methods=[dict(method='C5',write_class='R0',consistency='S1',token_order='None',
                retention_window=None,cursor=None,feed=None,cdc=None,delete_detection='reconcile')],
            cost_model={'unit':'Rows','confidence':'unknown','estimate':'bounded actual source rows; no cloud billing assertion'},
            capture_witness=str(witness_path.absolute()))
        config = dict(shared, protocol='oo.source-admission.config/v1',
            budget={'unit':'Rows','hard_limit':100000,'confidence':'unknown'},
            write_options=dict(signal_table=False,heartbeat_write=False,create_stream=False,create_trigger=False),
            debezium=dict(signal_data_collection=None,heartbeat_action_query=None,snapshot_mode='never',
                          create_stream=False,create_trigger=False),requested_write_class='R0')
        write(dest/'report.json',report)
        write(dest/'config.json',config)

if __name__ == '__main__':
    main()
