#!/usr/bin/env python3
"""Assemble the actual OpenSearch catalog and review scope for Selfbuild."""
import hashlib
import json
import os
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
legal = os.environ.get('OO_OPENSEARCH_LEGAL_BATCH') == '1'
BASE = ROOT / ('evidence/bc6cf794-2d14-4e98-bcb6-f7a9b3533a88/DL01/opensearch-driver-preflight/native-fresh-20261012-r2/current-20261013/legal-batch/product' if legal else 'evidence/bc6cf794-2d14-4e98-bcb6-f7a9b3533a88/DL01/opensearch-driver-preflight/native-fresh-20261012-r2/current-20261013/product')
OUT = BASE / 'bundle'
APPROVED = ('bin_value', 'doc_key', 'double_value', 'event_ms', 'flag', 'long_value',
    'note', 'payload_json', 'quantities', 'scaled_value', 'seq_key', 'tags', 'thing')


def sha(data):
    return hashlib.sha256(data).hexdigest()


def save(name, value):
    path = OUT / name
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False) + '\n')
    return sha(path.read_bytes())


def main():
    OUT.mkdir(parents=True, exist_ok=False)
    collection = (BASE / 'collection.json').read_bytes()
    catalog = json.loads(collection)
    entry = catalog['entries'][0]
    table = entry['tables'][0]
    assert len(catalog['entries']) == 1 and table['logical_schema'] == 'ledger' and table['logical_table'] == 'ledger_rows'
    names = {column['logical_name'] for column in table['columns']}
    assert set(APPROVED) <= names and {'c5_extra', 'raw_source', 'source_index'} <= names
    assert table['accepted_rows'] == 5
    profile = json.loads((BASE / 'profile.json').read_text())
    semantics = json.loads((BASE / 'source-semantics.json').read_text())
    assert profile['source_id'] == semantics['source_id'] == entry['source_id']
    assert profile['collection_sha256'] == semantics['collection_sha256'] == sha(collection)
    dataset = 'ledger.ledger_rows'
    visible = ['source_doc_id', *APPROVED]
    role = 'oo_r_ledger'
    visibility = {'protocol': 'oo.builder.role-visibility/v1', 'project': 'opensearch',
        'principal': 'fixture-owner', 'role': role,
        'catalog_version': 'pending', 'ontology_version': 'ont-opensearch-' + sha(collection)[:12],
        'policy': {'id': 'pol-opensearch-ledger', 'version': '1'},
        'knowledge': {'id': 'know-opensearch-ledger', 'version': '1'},
        'issued_for_compiler': 'oo-builder/role-pack/v1',
        'limits': {'max_entries': 64, 'max_file_bytes': 262144, 'max_total_bytes': 1048576},
        'datasets': [{'dataset': dataset, 'columns': visible, 'global_stats': 'denied', 'row_security': 'none'}]}
    settings = {'project': 'opensearch', 'catalog_version': 'pending',
        'ontology_version': visibility['ontology_version'], 'policy': visibility['policy'],
        'knowledge': visibility['knowledge'], 'ontology': {'spec_version': 'oos/v1',
            'name': 'opensearch_ledger', 'locale': 'en-US', 'include_schemas': ['ledger'],
            'classification': {'auto_accept': 'rule_high_confidence'}, 'compile': {'per_role': True,
                'budgets': {'readme_tokens': 2000, 'table_file_tokens': 4000},
                'sample_values': {'default': 'masked', 'max_per_column': 0}},
            'default_tier_preference': ['raw'], 'publish': {'include_unreviewed': []},
            'lifecycle': {'deprecation_min_releases': 2}}}
    (OUT / 'catalog.json').write_bytes(collection)
    files = {'catalog.json': sha(collection)}
    for name, value in {
        'profile.json': profile, 'source_semantics.json': semantics,
        'read_logic.json': {'contract': 'oo.selfbuild-control-api/read-logic/v1',
            'catalog_sha256': sha(collection), 'executed': False,
            'mappings': [{'dataset': dataset, 'stored_physical': table['stored_physical'],
                'scope_namespace': table['stored_namespace'], 'actual_storage_schema': '_oo'}]},
        'rules.json': {'columns': [{'column': dataset + '.' + column, 'semantic_type': 'identifier',
            'confidence': 'high', 'pii': 'unknown'} for column in ('source_doc_id', 'doc_key')],
            'relationships': []},
        'suggestions.json': {'certified': False, 'merged': False, 'ready': False},
        'generation.json': {'llm_calls': 0, 'outcome': 'structural_draft'},
        'roles.json': {'contract': 'oo.selfbuild-control-api/sealed-roles/v1',
            'roles': {role: [dataset + '.' + column for column in visible]}},
        'native-model.json': {'dataset': dataset, 'visibility': visibility, 'settings': settings,
            'declared_price': {'currency': 'USD', 'per_total_token': '0.000001',
                'status': 'declared_non_billing'}},
    }.items():
        files[name] = save(name, value)
    save('manifest.json', {'contract': 'oo.selfbuild-control-api/sealed-bundle/v1', 'files': files})
    print(json.dumps({'files': len(files), 'columns': len(names), 'visible': len(visible),
        'collection_sha256': sha(collection)}))


if __name__ == '__main__':
    main()
