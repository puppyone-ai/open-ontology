#!/usr/bin/env python3
"""Check the Oracle three-binding Selfbuild input against the actual collection."""
import argparse
import hashlib
import json
from pathlib import Path


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('bundle', type=Path)
    parser.add_argument('--generation', type=int, required=True)
    args = parser.parse_args()
    root = args.bundle
    files = {name: json.loads((root / name).read_text()) for name in
             ('catalog.json', 'profile.json', 'source_semantics.json', 'roles.json',
              'native-model.json', 'manifest.json')}
    catalog = files['catalog.json']
    profile = files['profile.json']
    semantics = files['source_semantics.json']
    vector = catalog['binding_vector']
    outer_sha = digest(root / 'catalog.json')
    vector_sha = hashlib.sha256(json.dumps(vector, sort_keys=True, separators=(',', ':'),
                                           ensure_ascii=False).encode()).hexdigest()
    ids = sorted(entry['source_id'] for entry in catalog['entries'])
    assert len(ids) == 3 and len(set(ids)) == 3
    assert len(vector) == 3
    for document in (profile, semantics):
        assert document['source_ids'] == ids
        assert 'source_id' not in document
        assert document['collection_sha256'] == outer_sha
        assert document['binding_vector_sha256'] == vector_sha
        assert document['cross_table_consistent'] is (
            len({item['source_scn'] for item in document['per_table']}) == 1)
        assert document['source_generation'] == document['target_generation'] == args.generation
        assert {entry['dataset']: {key: entry[key] for key in
                ('source_id', 'source_generation', 'target_generation', 'run_id')}
                for entry in vector} == document['table_generations']
    by_dataset = {entry['dataset']: entry for entry in vector}
    expected_fields = set()
    for entry in catalog['entries']:
        table = entry['tables'][0]
        dataset = table['logical_schema'] + '.' + table['logical_table']
        assert by_dataset[dataset]['source_id'] == entry['source_id']
        expected_fields.update(dataset + '.' + column['logical_name'] for column in table['columns'])
    assert {entry['dataset'] for entry in profile['tables']} == set(by_dataset)
    for table in profile['tables']:
        assert table['source_id'] == by_dataset[table['dataset']]['source_id']
        assert table['full_scan'] is True
        assert {table['dataset'] + '.' + column['name'] for column in table['columns']} == {
            field for field in expected_fields if field.startswith(table['dataset'] + '.')}
    assert set(files['roles.json']['roles']['oo_r_oracle_full']) == expected_fields
    assert len(expected_fields) == 13
    visibility = files['native-model.json']['visibility']
    settings = files['native-model.json']['settings']
    assert set(settings) == {'project', 'ontology_version', 'catalog_version',
                             'knowledge', 'policy', 'ontology'}
    assert {'spec_version', 'name', 'locale', 'include_schemas',
            'default_tier_preference'} <= set(settings['ontology'])
    assert visibility['project'] == 'oracle' and visibility['role'] == 'oo_r_oracle_full'
    assert {item['dataset'] + '.' + column for item in visibility['datasets']
            for column in item['columns']} == expected_fields
    for name, sha in files['manifest.json']['files'].items():
        assert digest(root / name) == sha, name
    assert profile['rows_compared'] == 11 and profile['compared_cells'] == 54
    assert profile['difference_count'] == semantics['source_full_typed_cell_diff'] == 0
    print(json.dumps({'status': 'ok', 'sources': ids, 'fields': len(expected_fields),
                      'rows': 11, 'cells': 54, 'catalog_sha256': outer_sha,
                      'binding_vector_sha256': vector_sha}, sort_keys=True))


if __name__ == '__main__':
    main()
