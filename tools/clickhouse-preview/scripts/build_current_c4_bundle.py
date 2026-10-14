#!/usr/bin/env python3
"""Make a source-specific sealed Selfbuild input from the actual C4 collection."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil

ROOT = Path('/Users/redux/puppy-one/open-ontology')
PROFILE = ROOT / 'evidence/4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/DL01/clickhouse-product/raw/run/native-20261013-c4-profile/source-profile.json'
NAMES = ('reading_id', 'version', 'amount', 'note', 'code', 'code_or_null', 'ts', 'tags', 'labels', 'qty', 'label', 'wide_u64')
# amount is decimal money. The source stores no currency, so the unit is unspecified.
TYPES = ('identifier', 'system', 'money', 'free_text', 'code', 'code', 'event_time', 'json', 'json', 'quantity', 'category', 'system')


def write(path, value):
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False) + '\n')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--collection', type=Path, required=True)
    parser.add_argument('--profile',type=Path)
    parser.add_argument('--connection-config',type=Path)
    parser.add_argument('--semantics', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    config=json.loads(args.connection_config.read_text()) if args.connection_config else {}
    database=config.get('source',{}).get('database','oo_dl01w40');table_name=config.get('source',{}).get('table','readings')
    dataset=database+'.'+table_name;project=config.get('project_id','clickhouse')
    collection = json.loads(args.collection.read_text())
    semantics = json.loads(args.semantics.read_text())
    if len(collection['entries']) != 1 or len(collection['binding_vector']) != 1:
        raise RuntimeError('approved ClickHouse scope must have one actual table')
    table = collection['entries'][0]['tables'][0]
    if (table['logical_schema'] != database or table['logical_table'] != table_name
            or {c['logical_name'] for c in table['columns']} != set(NAMES)):
        raise RuntimeError('full approved 12-column scope changed')
    if semantics['collection_sha256'] != hashlib.sha256(args.collection.read_bytes()).hexdigest():
        raise RuntimeError('source semantics are not bound to this exact collection')
    args.output.mkdir(parents=True, exist_ok=False)
    for name, source in [('catalog.json', args.collection), ('profile.json', args.profile or PROFILE), ('source_semantics.json', args.semantics)]:
        shutil.copyfile(source, args.output / name)
    write(args.output / 'read_logic.json', {
        'contract': 'oo.selfbuild-control-api/read-logic/v1',
        'executed': False,
        'approved_source': 'oo_dl01w40.readings',
        'capture_method': 'C4',
        'read_consistency': 'FINAL per-table S0',
    })
    write(args.output / 'rules.json', {
        'columns': [
            {'column': f'oo_dl01w40.readings.{name}', 'semantic_type': semantic,
             'confidence': 'high' if name in {'reading_id', 'version', 'ts', 'qty', 'wide_u64'} else 'low',
             'pii': 'none' if semantic == 'money' or name in {'reading_id', 'version', 'ts', 'qty', 'wide_u64'} else 'unknown',
             **({'unit': 'unspecified'} if semantic == 'money' else {})}
            for name, semantic in zip(NAMES, TYPES)
        ],
        'relationships': [],
    })
    write(args.output / 'suggestions.json', {
        'contract': 'oo.selfbuild-control-api/suggestions/v1',
        'certified': False, 'merged': False, 'ready': False,
    })
    write(args.output / 'generation.json', {
        'llm_calls': 0, 'outcome': 'structural_draft', 'proxy_call': False,
    })
    write(args.output / 'roles.json', {
        'contract': 'oo.selfbuild-control-api/sealed-roles/v1',
        'roles': {'oo_r_reader': [f'oo_dl01w40.readings.{name}' for name in NAMES]},
    })
    write(args.output / 'native-model.json', {
        'dataset': 'oo_dl01w40.readings',
        'source_contract': {
            'schema': 'oo.pg-builder.source-contract/v1',
            'family': 'clickhouse', 'product': 'clickhouse',
            'version': '26.8.21.10', 'capture': 'sealed_import',
            'grade': 'S0', 'source_kind': 'clickhouse',
            'delete_detection': 'partition_replace',
            'columns': [
                {'dataset': 'oo_dl01w40.readings', 'column': c['logical_name'], 'caveat': c['caveat']}
                for c in table['columns'] if c.get('caveat') is not None
            ],
        },
        'visibility': {
            'protocol': 'oo.builder.role-visibility/v1',
            'project': 'clickhouse',
            'principal': 'fixture-owner',
            'role': 'oo_r_reader',
            'catalog_version': 'pending',
            'ontology_version': 'ont-clickhouse-c4-current',
            'policy': {'id': 'pol-clickhouse-readings', 'version': '1'},
            'knowledge': {'id': 'know-clickhouse-readings', 'version': '1'},
            'issued_for_compiler': 'oo-builder/role-pack/v1',
            'limits': {'max_entries': 64, 'max_file_bytes': 262144, 'max_total_bytes': 1048576},
            'datasets': [{'dataset': 'oo_dl01w40.readings', 'columns': list(NAMES),
                          'global_stats': 'denied', 'row_security': 'none'}],
        },
        'settings': {
            'project': 'clickhouse',
            'catalog_version': 'pending',
            'ontology_version': 'ont-clickhouse-c4-current',
            'policy': {'id': 'pol-clickhouse-readings', 'version': '1'},
            'knowledge': {'id': 'know-clickhouse-readings', 'version': '1'},
            'ontology': {
                'spec_version': 'oos/v1', 'name': 'clickhouse_readings', 'locale': 'en-US',
                'include_schemas': ['oo_dl01w40'],
                'classification': {'auto_accept': 'rule_high_confidence'},
                'compile': {'per_role': True,
                            'budgets': {'readme_tokens': 2000, 'table_file_tokens': 4000},
                            'sample_values': {'default': 'masked', 'max_per_column': 0}},
                'default_tier_preference': ['raw'],
                'publish': {'include_unreviewed': []},
                'lifecycle': {'deprecation_min_releases': 2},
            },
        },
        'declared_price': {'currency': 'USD', 'per_total_token': '0.000001',
                           'status': 'declared_non_billing'},
    })
    if config:
        def rebind(value):
            if isinstance(value,dict):return {rebind(key):(project if key=='project' else rebind(item)) for key,item in value.items()}
            if isinstance(value,list):return [rebind(item) for item in value]
            if isinstance(value,str):
                return value.replace('oo_dl01w40.readings',dataset).replace('oo_dl01w40',database)
            return value
        for file in args.output.glob('*.json'):
            if file.name not in {'catalog.json','profile.json','source_semantics.json'}:write(file,rebind(json.loads(file.read_text())))
    files = {file.name: hashlib.sha256(file.read_bytes()).hexdigest()
             for file in args.output.iterdir() if file.is_file()}
    write(args.output / 'manifest.json', {
        'contract': 'oo.selfbuild-control-api/sealed-bundle/v1', 'files': files,
    })
    print(json.dumps({'columns': len(NAMES), 'bundle': str(args.output), 'manifest_sha256': hashlib.sha256((args.output / 'manifest.json').read_bytes()).hexdigest()}))


if __name__ == '__main__':
    main()
