#!/usr/bin/env python3
"""Export the actual ACKed current C4 target into a complete approved collection."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path('/Users/redux/puppy-one/open-ontology')
PRIVATE = Path('/tmp/oo-dl01w1-40-native-runtime')
IMAGE = 'rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5'
sys.path.insert(0, str(ROOT / 'tools/pg-builder/scripts'))
from export_native_collection import collection  # noqa: E402


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--pipeline', type=Path, required=True)
    parser.add_argument('--pipeline-sha256', required=True)
    parser.add_argument('--admission', type=Path, required=True)
    parser.add_argument('--admission-sha256', required=True)
    parser.add_argument('--request', type=Path, required=True)
    parser.add_argument('--registry', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if digest(args.pipeline) != args.pipeline_sha256 or digest(args.admission) != args.admission_sha256:
        raise RuntimeError('fixed executable digest changed')
    request = json.loads(args.request.read_text())
    if (request['source_id'] != 'clickhouse_268' or request['source_generation'] != 5
            or request['target_generation'] != 5):
        raise RuntimeError('not the current ClickHouse C4 target')
    registry_hash = digest(args.registry)
    args.output.mkdir(parents=True, exist_ok=False)
    command = ['docker', 'run', '-i', '--rm', '--pull', 'never',
        '--network', 'container:oo-dl01w1-40-gpt-fresh-pg',
        '--mount', f'type=bind,source={PRIVATE},target=/private,readonly',
        '--mount', f'type=bind,source={ROOT},target={ROOT},readonly',
        '--mount', 'type=bind,source=/tmp/oo-dl01w1-40-gpt-producer,target=/tmp/oo-dl01w1-40-gpt-producer,readonly',
        '--mount', f'type=bind,source={args.pipeline.resolve()},target=/bin/oo-pipeline,readonly',
        '--mount', f'type=bind,source={args.admission.resolve()},target=/bin/oo-source-admission,readonly',
        '-e', 'OO_NATIVE_ADMISSION_BIN=/bin/oo-source-admission',
        '-e', f'OO_NATIVE_BINDING_REGISTRY={args.registry.resolve()}',
        '-e', f'OO_NATIVE_BINDING_REGISTRY_SHA256={registry_hash}',
        '--entrypoint', '/bin/oo-pipeline', IMAGE, 'catalog-import-native']
    result = subprocess.run(command, input=args.request.read_bytes(), capture_output=True)
    (args.output / 'catalog.stdout').write_bytes(result.stdout)
    (args.output / 'catalog.stderr').write_bytes(result.stderr)
    (args.output / 'catalog.exit').write_text(str(result.returncode) + '\n')
    if result.returncode:
        raise SystemExit(result.returncode)
    entry = json.loads(result.stdout)
    if entry['source_id'] != request['source_id'] or entry['run_id'] != request['run_id']:
        raise RuntimeError('catalog identity differs from ACKed current request')
    body = collection([entry], [('oo_dl01w40', 'readings')])
    (args.output / 'collection.json').write_text(json.dumps(body, indent=2) + '\n')
    print(json.dumps({'collection_sha256': body['collection_sha256'],
        'entry_run_id': entry['run_id'], 'table_count': len(body['binding_vector'])}))


if __name__ == '__main__':
    main()
