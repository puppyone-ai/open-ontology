#!/usr/bin/env python3
"""Materialize an admission request from actual owned PG page/permission observations.

Does not issue decisions or receipts. Permissions, including REPLICATION, remain
visible. Producer paths use the operator's declared container mount namespace.
"""
import argparse
import hashlib
import json
from pathlib import Path


def write(path, value):
    content = json.dumps(value, indent=2) + '\n'
    if path.exists():
        if path.read_text() != content:
            raise ValueError(f'immutable admission input changed: {path}')
        return
    path.write_text(content)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--output-root', required=True, type=Path)
    parser.add_argument('--visible-root', required=True)
    parser.add_argument('--config', required=True, type=Path)
    parser.add_argument('--permissions', required=True, type=Path)
    parser.add_argument('--driver', required=True, type=Path)
    parser.add_argument('--visible-driver', required=True)
    parser.add_argument('--transaction-only', action='store_true')
    args = parser.parse_args()
    config = json.loads(args.config.read_text())
    facts = [json.loads(line) for line in args.permissions.read_text().splitlines()]
    role = facts[0]
    assert role['role'] == config['capture']['username']
    table_facts = facts[1:]
    approved = {(item['schema'], item['table']) for item in config['approved_objects']}
    assert {(item['schema'], item['table']) for item in table_facts} == approved
    permissions = []
    for name, key in [('SELECT', 'select'), ('USAGE', 'usage')]:
        permissions.append({'name': name, 'status': 'known', 'granted': all(item[key] for item in table_facts)})
    for name, key in [('INSERT','insert'), ('UPDATE','update'), ('DELETE','delete'), ('TRUNCATE','truncate'), ('CREATE','create')]:
        permissions.append({'name': name, 'status': 'known', 'granted': any(item[key] for item in table_facts)})
    for name, key in [('CONNECT','connect'), ('TEMP','temp'), ('REPLICATION','replication'), ('SUPERUSER','superuser'), ('BYPASSRLS','bypassrls'), ('CREATEDB','createdb'), ('CREATEROLE','createrole')]:
        permissions.append({'name': name, 'status': 'known', 'granted': role[key]})
    driver_hash = hashlib.sha256(args.driver.read_bytes()).hexdigest()
    root = args.output_root / 'admission-inputs'
    root.mkdir(exist_ok=True)
    permissions_path = root/'observed-permissions.jsonl'
    permissions_bytes = args.permissions.read_bytes()
    if permissions_path.exists() and permissions_path.read_bytes() != permissions_bytes:
        raise ValueError('observed permissions changed; use a new capture generation root')
    permissions_path.write_bytes(permissions_bytes)
    for capture_path in sorted((args.output_root/'native-admit').rglob('*.capture.json')):
        capture = json.loads(capture_path.read_text())
        if args.transaction_only and 'transaction_scope' not in capture:
            continue
        request = json.loads(capture_path.with_name(capture_path.name.replace('.capture.', '.request.')).read_text())
        table = capture['logical_table']
        schema = capture['logical_schema']
        assert (schema, table) in approved
        columns = json.loads(json.dumps(request['columns']))
        source_columns = {item['name']: item for item in capture.get('source_columns', [])}
        for column in columns:
            source = source_columns.get(column['name'])
            if source is not None:
                assert source['arrow_type'] == column['arrow_type'] and source['nullable'] == column['nullable']
                for field in ('source_oid', 'source_typmod', 'source_format'):
                    column[field] = source[field]
        target = root / f'{schema}.{table}' / str(capture['source_generation'])
        target.mkdir(parents=True, exist_ok=True)
        visible = args.visible_root.rstrip('/') + '/admission-inputs/' + f'{schema}.{table}/' + str(capture['source_generation'])
        schema_preimage = [{'table': table, 'columns': columns}]
        type_preimage = []
        for column in columns:
            row = {'table': table, 'name': column['name'], 'arrow_type': column['arrow_type']}
            for field in ('precision','scale','source_oid','source_typmod','source_format'):
                if field in column:
                    row[field] = column[field]
            type_preimage.append(row)
        write(target/'schema-preimage.json', schema_preimage)
        write(target/'type-preimage.json', type_preimage)
        witness = {'protocol':'oo.source-admission.capture-witness/v1','source_engine':'postgresql','source_id':config['source_id'],'source_generation':capture['source_generation'],'snapshot':capture['snapshot'],'run_id':request['batch_id'],'logical_schema':schema,'logical_table':table,'capturer_path':args.visible_root.rstrip('/')+'/native-admit/'+str(capture_path.relative_to(args.output_root/'native-admit')),'arrow_path':args.visible_root.rstrip('/')+'/native-admit/'+str(capture_path.parent.relative_to(args.output_root/'native-admit'))+'/'+capture['arrow'],'driver_path':args.visible_driver,'schema_preimage_path':visible+'/schema-preimage.json','type_preimage_path':visible+'/type-preimage.json'}
        write(target/'witness.json', witness)
        identity={'target_id':'oo-dl01w1-46-src/PG17.10','source_id':config['source_id'],'secret_ref':'secret://oo-dl01w1-46/cdc','compat_mode':'postgres-native'}
        capabilities=[]
        for name, write_class, consistency, cdc, budget_unit in [('Catalog','R0','S0',None,None),('BatchFull','R0','S1',None,None),('Cdc','R1','S1','LogicalDecoding',None),('Budget','R0','B',None,'Rows')]:
            capabilities.append({'name':name,'cursor':None,'feed':None,'cdc':cdc,'token_order':None,'budget_unit':budget_unit,'level':'experimental','write_class':write_class,'consistency':consistency})
        report={'protocol':'oo.source-admission.report/v1',**identity,'driver':{'kind':'native-rust','name':'postgres-preview','version':'0.1.0','digest':driver_hash},'license_flags':[],'permissions':permissions,'capabilities':capabilities,'capture_methods':[],'cost_model':{'unit':'Rows','confidence':'known','estimate':'actual bounded source snapshot pages; configured hard stop'},'r1w':None,'capture_witness':visible+'/witness.json'}
        write(target/'report.json',report)
        source_config={'protocol':'oo.source-admission.config/v1',**identity,'permissions':permissions,'budget':{'unit':'Rows','hard_limit':config['budget']['max_rows'],'confidence':'known'},'write_options':{'signal_table':False,'heartbeat_write':False,'create_stream':False,'create_trigger':False},'debezium':{'signal_data_collection':None,'heartbeat_action_query':None,'snapshot_mode':'never','create_stream':False,'create_trigger':False},'requested_write_class':'R1','r1w':None}
        write(target/'config.json',source_config)
    print(json.dumps({'protocol':'oo.postgres-preview.admission-inputs/v1','requests':len(list(root.rglob('report.json'))),'decisions_issued':0,'source_permissions_observed':True,'replication':role['replication']}))


if __name__ == '__main__':
    main()
