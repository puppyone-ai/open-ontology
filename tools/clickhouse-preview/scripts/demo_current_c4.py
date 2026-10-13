#!/usr/bin/env python3
"""Read-only startup demonstration of the live ClickHouse C4 source and target."""
import argparse
from decimal import Decimal
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path('/Users/redux/puppy-one/open-ontology')
GOLD = ROOT / 'testing/dl01-driver-preflight/clickhouse/fixtures/gold-20261013-c4-insert-update-delete.json'
VERIFY = ROOT / 'tools/clickhouse-preview/scripts/verify_current_c4_target.py'
CH = 'oo-dl01w1-40-gpt-fresh-ch'
PG = 'oo-dl01w1-40-gpt-fresh-pg'


def run(command):
    result = subprocess.run(command, capture_output=True, text=True)
    if result.returncode:
        raise RuntimeError(f'{command[0]} exit {result.returncode}: {result.stderr.strip()}')
    return result.stdout.strip()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--generation', type=int, default=5)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    for container in (CH, PG):
        if run(['docker', 'inspect', '-f', '{{.State.Running}}', container]) != 'true':
            raise RuntimeError(f'{container} is not running')
    source = run(['docker', 'exec', CH, 'clickhouse-client', '--query',
        'SELECT reading_id,version,toString(amount) AS amount,note,code,code_or_null,'
        'toString(ts) AS ts,tags,labels,qty,label,wide_u64 '
        'FROM oo_dl01w40.readings FINAL ORDER BY reading_id FORMAT JSONEachRow'])
    source_rows = [json.loads(line) for line in source.splitlines()]
    expected = json.loads(GOLD.read_text())
    numerics = {'reading_id', 'version', 'qty', 'wide_u64'}
    if len(source_rows) != len(expected):
        raise RuntimeError('current source row count differs from independent gold')
    for actual, want in zip(source_rows, expected):
        if set(actual) != set(want):
            raise RuntimeError('current source columns differ from independent gold')
        for name in want:
            observed = str(actual[name]) if name in numerics else actual[name]
            match = Decimal(observed) == Decimal(want[name]) if name == 'amount' else observed == want[name]
            if not match:
                raise RuntimeError(f'current source typed mismatch: {name} at reading {want["reading_id"]}')
    mutations = run(['docker', 'exec', CH, 'clickhouse-client', '--query',
        "SELECT count() FROM system.mutations WHERE database='oo_dl01w40' AND table='readings' AND is_done=0 FORMAT TSV"])
    if mutations != '0':
        raise RuntimeError(f'{mutations} pending source mutations')
    diff_path = args.output / 'typed-diff.json'
    run([sys.executable, str(VERIFY), '--gold', str(GOLD), '--generation', str(args.generation), '--output', str(diff_path)])
    diff = json.loads(diff_path.read_text())
    result = {
        'source_row_ids': [row['reading_id'] for row in source_rows],
        'source_typed_cell_diff_zero': True,
        'pending_source_mutations': int(mutations),
        'target_generation': args.generation,
        'target_rows': diff['row_count'],
        'target_columns': len(diff['columns']),
        'typed_cell_diff_zero': diff['diff_zero'],
        'scope': 'C4 source and unpublished native target only; catalog/model/Gateway are separate gates',
    }
    (args.output / 'demo.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result))


if __name__ == '__main__':
    main()
