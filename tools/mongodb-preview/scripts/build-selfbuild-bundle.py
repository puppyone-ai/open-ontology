#!/usr/bin/env python3
"""Assemble a Mongo-specific, hash-bound Selfbuild bundle from current proofs."""
import argparse
import hashlib
import json
import pathlib


def encoded(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False).encode()


def sha(data):
    return hashlib.sha256(data).hexdigest()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--input', required=True)
    parser.add_argument('--out', required=True)
    parser.add_argument('--project', required=True)
    parser.add_argument('--role', required=True)
    args = parser.parse_args()
    source = pathlib.Path(args.input)
    collection = (source/'collection.json').read_bytes()
    profile = (source/'source-profile.json').read_bytes()
    semantics = (source/'source-semantics.json').read_bytes()
    catalog = json.loads(collection)
    entries = catalog['entries']
    if len(entries) != 1 or entries[0]['engine'] != 'mongodb' or len(entries[0]['tables']) != 1:
        raise RuntimeError('one current Mongo catalog required')
    table = entries[0]['tables'][0]
    dataset = f"{table['logical_schema']}.{table['logical_table']}"
    columns = [item['logical_name'] for item in table['columns']]
    if len(columns) != 20 or len(set(columns)) != 20:
        raise RuntimeError('complete 20-column catalog required')
    out = pathlib.Path(args.out)
    out.mkdir(parents=True, exist_ok=False)
    files = {
        'catalog.json': collection,
        'profile.json': profile,
        'source_semantics.json': semantics,
        'read_logic.json': encoded({'contract': 'oo.selfbuild-control-api/read-logic/v1',
                                    'catalog_sha256': sha(collection), 'executed': False,
                                    'mappings': [{'dataset': dataset, 'actual_storage_schema': '_oo',
                                                  'scope_namespace': table['stored_namespace'],
                                                  'stored_physical': table['stored_physical']}]}),
        'rules.json': encoded({'columns': [{'column': dataset+'.'+name, 'semantic_type': meaning,
                                             'confidence': 'high', 'pii': 'unknown',
                                             **({'unit': 'unspecified'} if meaning == 'money' else {})}
                                            for name, meaning in [('_id','identifier'),('uid','identifier'),
                                                                  ('code','code'),('created','event_time'),
                                                                  ('units','quantity'),('price','money'),
                                                                  ('weight','quantity')]], 'relationships': []}),
        'suggestions.json': encoded({'certified': False, 'merged': False, 'ready': False}),
        'generation.json': encoded({'outcome': 'structural_draft', 'llm_calls': 0}),
        'roles.json': encoded({'contract': 'oo.selfbuild-control-api/sealed-roles/v1',
                               'roles': {args.role: [dataset+'.'+name for name in columns]}}),
    }
    ontology = 'ont-mongodb-native-'+catalog['collection_sha256'][:16]
    knowledge = {'id': 'know-mongodb-shipments', 'version': '1'}
    policy = {'id': 'pol-mongodb-shipments', 'version': '1'}
    settings = {'catalog_version': 'pending', 'knowledge': knowledge, 'ontology': {
        'classification': {'auto_accept': 'rule_high_confidence'},
        'compile': {'budgets': {'readme_tokens': 2000, 'table_file_tokens': 4000}, 'per_role': True,
                    'sample_values': {'default': 'masked', 'max_per_column': 0}},
        'default_tier_preference': ['raw'], 'include_schemas': [table['logical_schema']],
        'lifecycle': {'deprecation_min_releases': 2}, 'locale': 'en-US', 'name': 'mongodb_shipments',
        'publish': {'include_unreviewed': []}, 'spec_version': 'oos/v1'},
        'ontology_version': ontology, 'policy': policy, 'project': args.project}
    visibility = {'catalog_version': 'pending', 'datasets': [{'dataset': dataset, 'columns': columns,
                    'global_stats': 'denied', 'row_security': 'none'}],
                  'issued_for_compiler': 'oo-builder/role-pack/v1', 'knowledge': knowledge,
                  'limits': {'max_entries': 128, 'max_file_bytes': 262144, 'max_total_bytes': 1048576},
                  'ontology_version': ontology, 'policy': policy, 'principal': 'fixture-admin',
                  'project': args.project, 'protocol': 'oo.builder.role-visibility/v1', 'role': args.role}
    files['native-model.json'] = encoded({'dataset': dataset, 'settings': settings, 'visibility': visibility,
                                          'declared_price': {'currency': 'USD', 'per_total_token': '0.00000100',
                                                             'status': 'declared_non_billing'}})
    for name, data in files.items():
        (out/name).write_bytes(data)
    manifest = {'contract': 'oo.selfbuild-control-api/sealed-bundle/v1',
                'files': {name: sha(data) for name, data in files.items()}}
    (out/'manifest.json').write_bytes(encoded(manifest))
    print(json.dumps({'dataset': dataset, 'columns': len(columns), 'bundle_files': len(files),
                      'manifest_sha256': sha((out/'manifest.json').read_bytes())}, sort_keys=True))


if __name__ == '__main__':
    main()
