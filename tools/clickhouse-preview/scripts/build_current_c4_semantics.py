#!/usr/bin/env python3
"""Bind truthful ClickHouse C4 semantics to one current native collection."""
import argparse
import hashlib
import json
from pathlib import Path

ROOT = Path('/Users/redux/puppy-one/open-ontology')
PRODUCT = ROOT / 'evidence/4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/DL01/clickhouse-product'
PROFILE = PRODUCT / 'raw/run/native-20261013-c4-profile/source-profile.json'
DIFF = PRODUCT / 'raw/run/native-20261013-c4-demo-v2/demo.json'


def digest(data):
    return hashlib.sha256(data).hexdigest()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--collection', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    raw = args.collection.read_bytes()
    collection = json.loads(raw)
    profile = json.loads(PROFILE.read_text())
    demo = json.loads(DIFF.read_text())
    if collection.get('protocol') != 'oo.pipeline.sealed-import-collection/v1':
        raise RuntimeError('native collection protocol required')
    entries, bindings = collection['entries'], collection['binding_vector']
    if len(entries) != 1 or len(bindings) != 1:
        raise RuntimeError('the approved ClickHouse source has one table')
    entry, binding = entries[0], bindings[0]
    table = entry['tables'][0]
    if (entry['source_id'] != 'clickhouse_268' or entry['source_generation'] != 5
            or entry['target_generation'] != 5 or binding['dataset'] != 'oo_dl01w40.readings'
            or table['accepted_rows'] != 2 or len(table['columns']) != 12):
        raise RuntimeError('collection differs from current fully verified source and target')
    if profile['tables'][0]['rows'] != 2 or not demo['source_typed_cell_diff_zero'] or not demo['typed_cell_diff_zero']:
        raise RuntimeError('profile or final typed proof differs')
    generations = {'oo_dl01w40.readings': {
        'source_generation': 5,
        'target_generation': 5,
        'run_id': entry['run_id'],
    }}
    result = {
        'protocol': 'oo.source.clickhouse.audit/v1',
        'source_id': 'clickhouse_268',
        'source_generation': 5,
        'target_generation': 5,
        'product': 'ClickHouse',
        'version': '26.8.21.10',
        'consistency_grade': 'S0',
        'capture_method': 'C4',
        'delete_detection': 'partition-replace-after-mutation-and-key-reconcile',
        'continuous_capture': {'schedule_periods_tested_seconds': [3, 7], 'status': 'paused_after_catch_up'},
        'generation_semantics': 'high_watermark',
        'table_generations': generations,
        'binding_vector_sha256': digest(json.dumps(bindings, sort_keys=True, separators=(',', ':'), ensure_ascii=False).encode()),
        'collection_sha256': digest(raw),
        'quality': {
            'tables': 1,
            'rows': 2,
            'columns': 12,
            'source_full_typed_diff': 0,
            'target_full_typed_diff': 0,
            'typed_cells_compared_each_side': 24,
            'source_mutations_pending': 0,
            'full_scan': True,
        },
        'source_contract': {
            'reader': 'oo_dl01w1_40_reader',
            'reader_successful_write_rows': 0,
            'source_reader_query_log_proof': str((PRODUCT / 'raw/run/native-20261013-c4-closure/source-reader-audit.tsv').relative_to(ROOT)),
            'dba_write_class': 'owned fixture only',
            'cross_table_consistent': False,
            'c4_not_cdc': True,
            'caveats': [
                'ClickHouse FINAL is per-table S0; no cross-table snapshot token.',
                'Updates and physical deletes become visible after ClickHouse mutations and the next partition refresh.',
                'Non-Nullable source defaults such as zero and empty string may represent omitted input.',
                'DateTime64(9) is preserved as numeric nanoseconds in the target; this is not a SQL timestamp conversion.',
                'Decimal256(76,10) is preserved in numeric(76,10).',
                'Binary bytea fields and JSON array fields are retained physically but are outside the current closed OOS scalar type set; no text coercion is asserted.',
            ],
        },
        'classification': {
            'pii': 'unknown_pending_fixture_review',
            'binary_oos_unsupported': ['note', 'code', 'code_or_null', 'label'],
            'json_oos_unsupported': ['tags', 'labels'],
            'supported_scalar_candidates': ['reading_id', 'version', 'amount', 'ts', 'qty', 'wide_u64'],
        },
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2, ensure_ascii=False) + '\n')
    print(json.dumps({'collection_sha256': result['collection_sha256'], 'binding_vector_sha256': result['binding_vector_sha256']}))


if __name__ == '__main__':
    main()
