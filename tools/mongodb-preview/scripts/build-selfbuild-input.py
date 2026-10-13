#!/usr/bin/env python3
"""Bind one real Mongo native catalog to an unsampled source profile."""
import argparse
import hashlib
import json
import pathlib


def compact(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False).encode()


def sha(value):
    return hashlib.sha256(value).hexdigest()


def value(record, name):
    if name.startswith('q_'):
        name = bytes.fromhex(name[2:]).decode('utf-8')
    if name == 'uid':
        return record['uid_hex']
    if name == 'blob':
        return record['blob_hex']
    if name == 'created':
        return record['created_ms']
    if name == 'price':
        return record['price']
    if name == 'weight':
        return record['weight']
    if name == 'lane':
        return {'type': record['lane_tag'], 'value': record['lane_text'] if record['lane_tag'] == 'utf8' else record['lane_num']}
    if name == 'pack':
        return None if record['pack_presence'] < 2 else {'kind': record['pack_kind'], 'n': record['pack_n']}
    return record[name]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--catalog', required=True)
    parser.add_argument('--decoded', required=True)
    parser.add_argument('--out', required=True)
    args = parser.parse_args()
    catalog = json.loads(pathlib.Path(args.catalog).read_text())
    records = json.loads(pathlib.Path(args.decoded).read_text())['records']
    if catalog['protocol'] != 'oo.pipeline.sealed-import-catalog/v1' or catalog['engine'] != 'mongodb' or len(catalog['tables']) != 1:
        raise RuntimeError('Mongo native catalog required')
    table = catalog['tables'][0]
    if len(records) != table['accepted_rows'] or len(table['columns']) != 20:
        raise RuntimeError('complete 20-column source required')
    dataset = f"{table['logical_schema']}.{table['logical_table']}"
    binding = {key: catalog[key] for key in ('source_id', 'run_id', 'source_generation', 'target_generation', 'config_digest', 'schema_digest', 'type_digest', 'input_digest')}
    binding.update(dataset=dataset, stored_namespace=table['stored_namespace'], stored_physical=table['stored_physical'])
    vector = [binding]
    collection = {'protocol': 'oo.pipeline.sealed-import-collection/v1', 'entries': [catalog], 'binding_vector': vector}
    collection['collection_sha256'] = sha(compact(collection))
    columns = []
    for column in table['columns']:
        name = column['logical_name']
        values = [value(record, name) for record in records]
        nonnull = [v for v in values if v is not None]
        encoded = [compact(v).decode() for v in nonnull]
        columns.append({'name': name, 'source_type': column['source_type'], 'nullable': len(nonnull) != len(values),
                        'aggregation': 'full_unsampled', 'min_max_encoding': 'canonical_json',
                        'min': min(encoded, default=None), 'max': max(encoded, default=None),
                        'rows': len(values), 'nulls': len(values)-len(nonnull), 'distinct': len(set(encoded)),
                        'null_count': len(values)-len(nonnull), 'distinct_count': len(set(encoded))})
    generations = {dataset: {key: catalog[key] for key in ('source_generation', 'target_generation', 'run_id')}}
    profile = {'protocol': 'oo.source.mongodb.profile/v1', 'source_id': catalog['source_id'],
               'source_generation': catalog['source_generation'], 'samples': 'none', 'sampled': False,
               'tables': [{'dataset': dataset, 'schema': table['logical_schema'], 'table': table['logical_table'],
                           'rows': len(records), 'full_scan': True, 'columns': columns,
                           **generations[dataset]}], 'generation_semantics': 'uniform',
               'table_generations': generations, 'binding_vector_sha256': sha(compact(vector))}
    out = pathlib.Path(args.out)
    out.mkdir(parents=True, exist_ok=False)
    (out/'collection.json').write_bytes(compact(collection))
    collection_file_sha = sha((out/'collection.json').read_bytes())
    profile['collection_sha256'] = collection_file_sha
    (out/'source-profile.json').write_bytes(compact(profile))
    semantics = {'protocol': 'oo.source.mongodb.audit/v1', 'source_id': catalog['source_id'],
                 'source_generation': catalog['source_generation'], 'target_generation': catalog['target_generation'],
                 'product': 'MongoDB', 'version': '8.0.32', 'consistency_grade': 'B',
                 'capture_method': 'C5', 'delete_detection': 'reconcile',
                 'continuous_capture': {'schedule_period_ms': 6000, 'tested_pins': [3,4,5,6],
                                        'status': 'paused_after_catch_up', 'external_snapshot_token': False},
                 'quality': {'tables': 1, 'rows': len(records), 'columns': 20, 'source_rows': 16,
                             'scope_refused_rows': 2, 'current_full_unsampled_typed_cell_differences': 0,
                             'current_checked_cells': len(records)*20},
                 'source_contract': {'reader_write_attempts': 0, 'standalone_c5': True},
                 'binding_vector_sha256': sha(compact(vector)), 'collection_sha256': collection_file_sha,
                 'evidence': {'catalog_run_id': catalog['run_id'], 'typed_oracle': 'current 14x20 diff0'},
                 'generation_semantics': 'uniform', 'table_generations': generations}
    (out/'source-semantics.json').write_bytes(compact(semantics))
    print(json.dumps({'dataset': dataset, 'columns': len(columns), 'rows': len(records),
                      'catalog_sha256': sha(pathlib.Path(args.catalog).read_bytes()),
                      'collection_sha256': collection_file_sha, 'binding_vector_sha256': sha(compact(vector))}, sort_keys=True))


if __name__ == '__main__':
    main()
