#!/usr/bin/env python3
"""Compare the public OO query with independently captured ClickHouse C4 gold."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path


def expected_cell(name, value):
    if name in {'note', 'code', 'code_or_null', 'label'}:
        return value.encode().hex()
    if name == 'ts':
        whole, fraction = value.rsplit('.', 1)
        seconds = int(datetime.strptime(whole, '%Y-%m-%d %H:%M:%S').replace(tzinfo=timezone.utc).timestamp())
        return str(seconds * 1_000_000_000 + int(fraction))
    if name in {'tags', 'labels'}:
        return json.dumps(value, separators=(',', ':'))
    return str(value)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--query', type=Path, required=True)
    parser.add_argument('--gold', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    query = json.loads(args.query.read_text())
    gold = json.loads(args.gold.read_text())
    names = [column['name'] for column in query['columns']]
    assert len(names) == 12 and len(set(names)) == 12
    assert len(query['rows']) == len(gold) == 2 and query['returned_rows'] == 2
    assert all(set(row) == set(names) for row in gold)
    for actual, want in zip(query['rows'], gold):
        assert len(actual) == len(names)
        for name, value in zip(names, actual):
            expected = expected_cell(name, want[name])
            assert value == expected, (name, value, expected)
    assert query['query_id'] and any(w['code'] == 'STALE_DATA' for w in query['warnings'])
    result = {'outcome': 'typed_diff_zero', 'rows': 2, 'columns': 12, 'cells': 24,
              'query_id': query['query_id'],
              'query_sha256': hashlib.sha256(args.query.read_bytes()).hexdigest(),
              'gold_sha256': hashlib.sha256(args.gold.read_bytes()).hexdigest()}
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result))


if __name__ == '__main__':
    main()
