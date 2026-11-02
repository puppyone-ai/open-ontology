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
    parser.add_argument('--multipart', action='store_true')
    parser.add_argument('--source-audit-policy', choices=['grant_enforced'])
    args = parser.parse_args()
    capture = args.capture.absolute()
    facts = json.loads((capture / 'capturer.json').read_text())
    part_scope = facts.get('range_scope', {})
    range_part = part_scope.get('protocol') == 'oo.pipeline.native-c5-range-part/v1'
    descriptor_path = capture / 'driver.json'
    schema_path = capture / 'schema-preimage.json'
    type_path = capture / 'type-preimage.json'
    if range_part:
        index = Path(part_scope['parts_index']['path'])
        ordinal = part_scope['part']['ordinal']
        if (type(ordinal) is not int or ordinal < 0 or not index.is_absolute()
                or capture != index.parent / f'part-{ordinal}.capture'
                or Path(part_scope['part']['arrow']['path']) != index.parent / f'part-{ordinal}.arrow'
                or args.multipart):
            raise SystemExit('range part witness location or protocol mismatch')
        descriptor_path, schema_path, type_path = (
            Path(facts[key]) for key in ('driver_path', 'schema_preimage_path', 'type_preimage_path'))
        for path, name in [(descriptor_path, 'driver.json'), (schema_path, 'schema-preimage.json'),
                           (type_path, 'type-preimage.json')]:
            if path != index.parent / name or path.is_symlink() or not path.is_file():
                raise SystemExit('range part shared preimage mismatch')
    descriptor = json.loads(descriptor_path.read_text())
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
            schema_preimage_path=str(schema_path),
            type_preimage_path=str(type_path))
        anchor = args.multipart and 'multipart_scope' in facts
        if anchor:
            witness['multipart_anchor'] = True
            witness['arrow_path'] = str(capture/'0.arrow')
        write(witness_path, witness)
        grade = table['consistency']
        permissions = [dict(name=name, status='known', granted=True,
                            scope={'database':table['schema'],'table':table['name']} if name == 'SELECT' else None)
                       for name in facts.get('observed_privileges', ['SELECT'])]
        shared = dict(target_id=facts['source_id'], source_id=facts['source_id'],
                      secret_ref=args.secret_ref, compat_mode='mysql-native',permissions=permissions,r1w=None)
        report = dict(shared, protocol='oo.source-admission.report/v1',
            driver={'kind':'native','name':'mysql_async','version':'0.36.1','digest':driver_digest},
            license_flags=['GPL-2.0'], capabilities=[dict(name='BatchFull',cursor=None,feed=None,
                cdc=None,token_order=None,budget_unit='Rows',level='experimental',write_class='R0',consistency=grade)],
            capture_methods=[dict(method='C5',write_class='R0',consistency=grade,token_order='None',
                retention_window=None,cursor=None,feed=None,cdc=None,delete_detection='reconcile')],
            cost_model={'unit':'Rows','confidence':'unknown','estimate':'bounded actual source rows; no cloud billing assertion'},
            capture_witness=str(witness_path.absolute()))
        config = dict(shared, protocol='oo.source-admission.config/v1',
            budget={'unit':'Rows','hard_limit':facts['chunk_budget']['max_rows'] if anchor else facts.get('max_rows', 100000),'confidence':'unknown'},
            write_options=dict(signal_table=False,heartbeat_write=False,create_stream=False,create_trigger=False),
            debezium=dict(signal_data_collection=None,heartbeat_action_query=None,snapshot_mode='never',
                          create_stream=False,create_trigger=False),requested_write_class='R0')
        write(dest/'report.json',report)
        if args.source_audit_policy is not None:
            config['source_audit_policy'] = args.source_audit_policy
        write(dest/'config.json',config)

if __name__ == '__main__':
    main()
