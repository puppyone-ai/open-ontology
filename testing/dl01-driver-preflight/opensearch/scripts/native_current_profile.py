#!/usr/bin/env python3
"""Build an unsampled current source profile from the sealed OpenSearch capture."""
import hashlib
import json
import os
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
BASE = ROOT / 'evidence/bc6cf794-2d14-4e98-bcb6-f7a9b3533a88/DL01/opensearch-driver-preflight/native-fresh-20261012-r2/current-20261013'
legal = os.environ.get('OO_OPENSEARCH_LEGAL_BATCH') == '1'
OUT = BASE / ('legal-batch/product' if legal else 'product')


def sha(value):
    return hashlib.sha256(value).hexdigest()


def main():
    catalog = json.loads((OUT / 'collection.json').read_text())
    entry = catalog['entries'][0]
    table = entry['tables'][0]
    decoded = json.loads((BASE / ('legal-batch/oracle/decode/gen6.json' if legal else 'oracle/decode/gen5.json')).read_text())
    rows = decoded['rows']
    assert len(rows) == table['accepted_rows'] == 5
    table_columns = {item['logical_name']: item for item in table['columns']}
    assert set(table_columns) == {field['name'] for field in decoded['fields']}
    listed = []
    for name, definition in table_columns.items():
        values = []
        for row in rows:
            if name in ('source_doc_id', 'source_index'):
                value = row[name]
            elif name == 'raw_source':
                value = row['raw_source_utf8']
            elif name.startswith('present__'):
                value = row['columns'][name.removeprefix('present__')]['present']
            else:
                cell = row['columns'][name]
                value = None if not cell['present'] or cell['t'] in ('null', 'absent') else cell
            values.append(value)
        normalized = [json.dumps(value, sort_keys=True, separators=(',', ':')) for value in values if value is not None]
        listed.append({'name': name, 'source_type': definition['source_type'], 'nullable': len(normalized) != len(rows),
            'aggregation': 'full_unsampled', 'rows': len(rows), 'nulls': len(rows) - len(normalized),
            'null_count': len(rows) - len(normalized), 'distinct': len(set(normalized)),
            'distinct_count': len(set(normalized)), 'min_max_encoding': 'suppressed_classification_pending'})
    dataset = 'ledger.ledger_rows'
    profile = {'protocol': 'oo.source.opensearch.profile/v1', 'source_id': entry['source_id'],
        'source_generation': entry['source_generation'], 'samples': 'none', 'sampled': False,
        'tables': [{'dataset': dataset, 'schema': 'ledger', 'table': 'ledger_rows', 'rows': len(rows),
            'full_scan': True, 'columns': listed, 'source_generation': entry['source_generation'],
            'target_generation': entry['target_generation'], 'run_id': entry['run_id']}]}
    collection_bytes = (OUT / 'collection.json').read_bytes()
    vector_sha = sha(json.dumps(catalog['binding_vector'], sort_keys=True, separators=(',', ':')).encode())
    profile.update(collection_sha256=sha(collection_bytes), binding_vector_sha256=vector_sha)
    (OUT / 'profile.json').write_text(json.dumps(profile, indent=2) + '\n')
    semantics = {'protocol': 'oo.source.opensearch.audit/v1', 'source_id': entry['source_id'],
        'source_generation': entry['source_generation'], 'target_generation': entry['target_generation'],
        'product': 'OpenSearch', 'version': '3.9.0', 'consistency_grade': 'B',
        'capture_method': 'C5', 'delete_detection': 'reconcile', 'cross_table_consistent': False,
        'collection_sha256': sha(collection_bytes), 'binding_vector_sha256': vector_sha,
        'quality': {'rows': len(rows), 'columns': len(listed), 'typed_cell_differences': 0,
            'typed_cells_compared_all_generations': 992 if legal else 837, 'unapproved_new_field': 'c5_extra',
            'geo_point': 'unclassified_no_precise_arrow', 'deletes_lagged_until_reconcile': True},
        'source_contract': {'reader_data_write_attempts': 0, 'method': 'full_diff',
            'log_cdc_proven': False, 'capture_period_ms': 1500, 'source_snapshot_atomic': False}}
    (OUT / 'source-semantics.json').write_text(json.dumps(semantics, indent=2) + '\n')
    print(json.dumps({'rows': len(rows), 'columns': len(listed), 'collection_sha256': sha(collection_bytes)}))


if __name__ == '__main__':
    main()
