#!/usr/bin/env python3
"""Call the fixed native loader with an exact sealed ClickHouse C4 request."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path('/Users/redux/puppy-one/open-ontology')
ELF = Path('/tmp/oo-dl01w1-79-time-interval-fixed/oo-pipeline-e85f9da679219aa6625a49d45aee62d8578b69299cae0a3503e5bec86f7b15ac')
ADMISSION = Path('/tmp/oo-dl01w1-79-time-interval-fixed/oo-source-admission-cfc11a13d0ff14e377e29b687dbb60adb4ced05476c48143fed1637b841bab67')
PRIVATE = Path('/tmp/oo-dl01w1-40-native-runtime')
IMAGE = 'rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5'

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--request', type=Path, required=True)
    parser.add_argument('--registry', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    request = json.loads(args.request.read_text())
    binding = json.loads(args.registry.read_text())['bindings'][0]
    if request['admission_path'] != binding['admission_receipt_path']:
        raise SystemExit('admission/registry mismatch')
    if hashlib.sha256(Path(request['admission_path']).read_bytes()).hexdigest() != binding['admission_receipt_sha256']:
        raise SystemExit('sealed admission changed')
    if hashlib.sha256(ELF.read_bytes()).hexdigest() != ELF.name.rsplit('-',1)[1]:
        raise SystemExit('fixed loader changed')
    args.output.mkdir(parents=True, exist_ok=False)
    cmd = ['docker','run','-i','--rm','--pull','never',
        '--network','container:oo-dl01w1-40-gpt-fresh-pg',
        '--mount',f'type=bind,source={PRIVATE},target=/private,readonly',
        '--mount',f'type=bind,source={ROOT},target={ROOT},readonly',
        '--mount','type=bind,source=/tmp/oo-dl01w1-40-gpt-producer,target=/tmp/oo-dl01w1-40-gpt-producer,readonly',
        '--mount',f'type=bind,source={ELF},target=/bin/oo-pipeline,readonly',
        '--mount',f'type=bind,source={ADMISSION},target=/bin/oo-source-admission,readonly',
        '-e','OO_NATIVE_ADMISSION_BIN=/bin/oo-source-admission',
        '-e',f'OO_NATIVE_BINDING_REGISTRY={args.registry.resolve()}',
        '-e',f'OO_NATIVE_BINDING_REGISTRY_SHA256={hashlib.sha256(args.registry.read_bytes()).hexdigest()}',
        '--entrypoint','/bin/oo-pipeline',IMAGE,'import-native-update']
    result = subprocess.run(cmd,input=args.request.read_bytes(),capture_output=True)
    (args.output/'stdout.json').write_bytes(result.stdout)
    (args.output/'stderr.txt').write_bytes(result.stderr)
    (args.output/'exit.txt').write_text(str(result.returncode)+'\n')
    print(result.stdout.decode('utf-8','replace').strip())
    raise SystemExit(result.returncode)

if __name__ == '__main__':
    main()
