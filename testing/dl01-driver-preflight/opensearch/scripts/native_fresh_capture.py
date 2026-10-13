#!/usr/bin/env python3
"""Run real C5 snapshots and DBA change window; never claim a Loader result."""
import hashlib
import json
from pathlib import Path
import subprocess
import time

ROOT = Path(__file__).resolve().parents[4]
SRC = ROOT / 'testing/dl01-driver-preflight/opensearch'
BASE = ROOT / 'evidence/bc6cf794-2d14-4e98-bcb6-f7a9b3533a88/DL01/opensearch-driver-preflight'
OUT = BASE / 'native-fresh-20261012-r2/capture'
BIN = BASE / 'native-20261012/bin/os-reader'
PRIVATE = Path('/tmp/oo-dl01w1-44-native-fresh-private-r2')
INDEX = 'oo-dl01w1-44-ledger'
NET = 'oo-dl01w1-44-native-fresh-net'
IMAGE = 'oo-a395-selfbuild:local'


def reader(name, args, expect=0):
    dest = OUT / name
    dest.mkdir(parents=True, exist_ok=False)
    cmd = ['docker', 'run', '--rm', '--pull', 'never', '--network', NET,
        '--user', '0:0', '--security-opt', 'no-new-privileges', '--cap-drop', 'ALL',
        '--label', 'owner=oo-dl01w1-44', '--entrypoint', '/bins/os-reader',
        '-v', f'{BIN}:/bins/os-reader:ro', '-v', f'{OUT}:/evidence',
        '-v', f'{PRIVATE}:/private:ro', '-v', f'{SRC}/fixtures:/fixtures:ro', IMAGE,
        *args, '--out', f'/evidence/{name}']
    p = subprocess.run(cmd, capture_output=True)
    (dest / 'stdout').write_bytes(p.stdout)
    (dest / 'stderr').write_bytes(p.stderr)
    (dest / 'exit').write_text(str(p.returncode) + '\n')
    if p.returncode != expect:
        raise SystemExit(f'{name}: exit{p.returncode}, expected{expect}; raw retained')
    result = dest / 'result.json'
    return json.loads(result.read_text()) if result.exists() else {}


def cycle(name, schedule='fast'):
    return reader(name, ['c5-cycle', '--schedule', '/fixtures/c5-schedule.json',
        '--private', '/private', '--index', INDEX, '--shadow', '/evidence/shadow.json',
        '--schedule-name', schedule, '--page-size', '2'])


def dba(name, action, docid='', payload=None):
    args = ['c5-dba', '--schedule', '/fixtures/c5-schedule.json', '--private', '/private',
        '--index', INDEX, '--shadow', '/evidence/shadow.json', '--action', action]
    if docid:
        args += ['--id', docid]
    if payload is not None:
        path = OUT / f'{name}-body.json'
        path.write_text(json.dumps(payload) + '\n')
        args += ['--body-file', f'/evidence/{path.name}']
    return reader(name, args)


def main():
    OUT.mkdir(parents=True, exist_ok=False)
    digest = hashlib.sha256(BIN.read_bytes()).hexdigest()
    if digest != '231fdce4f6da57e8c6f980a718bb72e9cdccda51f1e3fe5e7eb56ea584572b2f':
        raise SystemExit('retained reader identity drift')
    (OUT / 'binary.sha256').write_text(digest + '\n')
    reader('setup', ['setup', '--private', '/private', '--index', INDEX,
        '--expect-version', '3.9.0', '--mapping', '/fixtures/mapping.json',
        '--fixture', '/fixtures/customer-documents.json'])
    results = []
    for n in range(1, 11):
        if n > 1:
            shadow = json.loads((OUT / 'shadow.json').read_text())
            wait = int(shadow['fires_ms']['fast']) + 1500 - int(time.time() * 1000)
            if wait > 0:
                time.sleep(wait / 1000 + 0.05)
        if n == 3:
            source = json.loads((SRC / 'fixtures/customer-documents.json').read_text())['documents'][0]['source'].copy()
            source.update(doc_key='ledger-c5', note='native-insert', long_value=44, seq_key=70)
            dba('dba-insert', 'index', 'led-c5', source)
            dba('dba-insert-refresh', 'refresh')
        if n == 5:
            dba('dba-update', 'update', 'led-c5', {'doc': {'note': 'native-update', 'long_value': 45}})
            dba('dba-add-mapping', 'mapping', payload={'properties': {'c5_extra': {'type': 'keyword'}}})
            dba('dba-add-field', 'update', 'led-02', {'doc': {'c5_extra': 'native-unclassified'}})
            dba('dba-update-refresh', 'refresh')
        if n == 7:
            dba('dba-delete', 'delete', 'led-c5')
            dba('dba-delete-refresh', 'refresh')
        result = cycle(f'fast-{n}')
        if result.get('reader_mutations') != 0 or not result.get('arrow_sha256'):
            raise SystemExit(f'fast-{n}: source mutation or absent Arrow')
        results.append(result['report'])
    assert 'ledger-c5' in results[2]['inserted']
    assert 'ledger-c5' in results[4]['updated']
    assert results[6]['tombstoned'] == [] and results[6]['suspected_deletes'] == ['ledger-c5']
    assert results[7]['tombstoned'] == ['ledger-c5']
    assert all('ledger-c5' not in r['inserted'] for r in results[8:])
    assert json.loads((OUT / 'shadow.json').read_text())['rows']['ledger-c5']['tombstone'] is True
    (OUT / 'summary.json').write_text(json.dumps({'capture_method': 'C5', 'consistency': 'B',
        'cycles': results, 'reader_mutations': 0, 'loader_executed': False,
        'independent_full_target_diff': 'not_run', 'source_acceptance': 'not_claimed'}, indent=2) + '\n')
    print('C5_CAPTURE_OK: Loader, Ontology and dialogue exits remain pending')


if __name__ == '__main__':
    main()
