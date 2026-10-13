#!/usr/bin/env python3
"""Build the unsampled approved-table profile from verified final C4 captures."""
import argparse
import hashlib
import json
from pathlib import Path
from decimal import Decimal

ROOT = Path('/Users/redux/puppy-one/open-ontology')
PRODUCT = ROOT / 'evidence/4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/DL01/clickhouse-product'
RUN = PRODUCT / 'raw/run'
GOLD = ROOT / 'testing/dl01-driver-preflight/clickhouse/fixtures/gold-20261013-c4-insert-update-delete.json'
CAPTURE = RUN / 'native-20261013-c4-catchup'
SOURCE = RUN / 'native-20261013-c4-current-capture-v2'
DIFF = RUN / 'native-20261013-c4-zero-tail/full-typed-diff.json'


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if not json.loads(DIFF.read_text())['diff_zero']:
        raise RuntimeError('final target typed diff is nonzero')
    columns = [json.loads(line) for line in (SOURCE / 'columns.body').read_text().splitlines()]
    gold = json.loads(GOLD.read_text())
    names = [item['name'] for item in columns]
    if len(names) != 12 or any(set(row) != set(names) for row in gold):
        raise RuntimeError('profile scope differs from verified final capture')
    for partition in ('202401', '202402'):
        compare = json.loads((CAPTURE / f'{partition}-allcells.json').read_text())
        if not compare['passed']:
            raise RuntimeError(f'{partition} source Arrow differs from independent gold')
        expected = [row for row in gold if row['ts'].replace('-', '')[:6] == partition]
        actual = json.loads((CAPTURE / f'{partition}.json').read_text())
        if actual['rows'] != len(expected):
            raise RuntimeError(f'{partition} capture row count differs')
    profiles = []
    for column in columns:
        name, source_type = column['name'], column['type']
        values = [row[name] for row in gold]
        nonnull = [v for v in values if v is not None]
        encoded = [json.dumps(v, ensure_ascii=False, sort_keys=True) if isinstance(v, list) else str(v) for v in nonnull]
        if source_type.startswith(('UInt', 'Int', 'Decimal')):
            ordered = sorted(encoded, key=Decimal)
        else:
            ordered = sorted(encoded)
        profile = {
            'name': name,
            'source_type': source_type,
            'nullable': source_type.startswith('Nullable(') or source_type.startswith('LowCardinality(Nullable('),
            'aggregation': 'full_unsampled',
            'min_max_encoding': 'source_text',
            'rows': len(values),
            'nulls': len(values) - len(nonnull),
            'distinct': len(set(encoded)),
            'null_count': len(values) - len(nonnull),
            'distinct_count': len(set(encoded)),
            'min': ordered[0] if ordered else None,
            'max': ordered[-1] if ordered else None,
        }
        profiles.append(profile)
    result = {
        'protocol': 'oo.source.clickhouse.profile/v1',
        'source_id': 'clickhouse_268',
        'source_generation': 5,
        'samples': 'none',
        'sampled': False,
        'generation_semantics': 'C4 partition-vector high-water mark',
        'tables': [{
            'dataset': 'oo_dl01w40.readings',
            'schema': 'oo_dl01w40',
            'table': 'readings',
            'rows': len(gold),
            'full_scan': True,
            'columns': profiles,
            'source_generation': 5,
            'target_generation': 5,
        }],
        'provenance': {
            'final_gold_sha256': hashlib.sha256(GOLD.read_bytes()).hexdigest(),
            'current_columns_sha256': hashlib.sha256((SOURCE / 'columns.body').read_bytes()).hexdigest(),
            'full_source_arrow_partitions': ['202401', '202402'],
            'empty_partitions_verified': ['202403', '209912'],
            'target_typed_diff': str(DIFF.relative_to(ROOT)),
        },
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2, ensure_ascii=False) + '\n')
    print(json.dumps({'table_count': 1, 'column_count': len(names), 'row_count': len(gold), 'profile': str(args.output)}))


if __name__ == '__main__':
    main()
