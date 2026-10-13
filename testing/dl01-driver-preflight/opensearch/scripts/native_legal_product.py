#!/usr/bin/env python3
"""Export a genuine new sealed catalog from the legal-batch target run."""
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
BASE = ROOT / 'evidence/bc6cf794-2d14-4e98-bcb6-f7a9b3533a88/DL01/opensearch-driver-preflight/native-fresh-20261012-r2'
PIN = BASE / 'scheduled-source/f7bae27e-e2b5-489f-9326-64801ec5aec4/7'
OUT = BASE / 'current-20261013/legal-batch/product'
PRIVATE = Path('/tmp/oo-dl01w1-44-native-fresh-private-r2')
LOADER = Path('/tmp/oo-native-loader-minimal-20261012-linux194')


def sha(data):
    return hashlib.sha256(data).hexdigest()


def pg(query):
    return subprocess.run(['docker', 'exec', 'oo-dl01w1-44-native-fresh-pg', 'psql', '-X', '-U',
        'postgres', '-d', 'oo_dl01w1_44_native_fresh', '-At', '-c', query],
        check=True, capture_output=True, text=True).stdout.strip()


def main():
    if (OUT / 'collection.json').exists():
        raise SystemExit('sealed legal-batch collection exists; refusing replacement')
    OUT.mkdir(parents=True, exist_ok=True)
    loader = json.loads((PIN / 'loader-input.json').read_text())
    assert loader['batch_id'] == 't6.b1' and loader['source_generation'] == 6
    run = json.loads(pg("select row_to_json(r)::text from (select run_id,source_id,source_generation,"
        "target_generation,config_digest,schema_digest,type_digest,input_digest from _oo.full_import_run "
        "where source_id='oo_dl01w1_44' and target_generation=6) r"))
    assert run['source_generation'] == 6 and run['schema_digest'] == loader['schema_digest']
    request = dict(run, protocol='oo.pipeline.native-sealed-catalog/v1',
        owner='5d3f04f6-15de-4cf5-af60-62f24974cc76', snapshot=loader['snapshot'],
        target={k: loader['target'][k] for k in ('host','port','database','runtime_principal','password_file','ca_file')})
    (OUT / 'catalog-request.json').write_text(json.dumps(request, indent=2) + '\n')
    assert sha(LOADER.read_bytes()) == '26855c12b869388b1a9a4590fe6f9521ae7f62685817428078bf51be40e67c18'
    registry = PIN / 'admission/registry.json'
    admission = Path('/tmp/oo-dl01w1-02-native-admission-minimal')
    assert sha(admission.read_bytes()) == '0b584453877a4c5ec3e7c311c199ade8af723229ea2ab56745858e37f7a1e274'
    proc = subprocess.run(['docker','run','--rm','-i','--pull','never',
        '--network','container:oo-dl01w1-44-native-fresh-pg','--entrypoint','/opt/oo-pipeline',
        '-v',str(LOADER)+':/opt/oo-pipeline:ro','-v',str(PRIVATE)+':/private:ro',
        '-v',str(ROOT)+':'+str(ROOT)+':ro','-v',str(admission)+':/opt/oo-source-admission:ro',
        '-e','OO_NATIVE_ADMISSION_BIN=/opt/oo-source-admission',
        '-e','OO_NATIVE_BINDING_REGISTRY='+str(registry),
        '-e','OO_NATIVE_BINDING_REGISTRY_SHA256='+sha(registry.read_bytes()),
        'oo-a395-selfbuild:local','catalog-import-native'], input=json.dumps(request).encode(),
        capture_output=True)
    (OUT / 'catalog.stdout').write_bytes(proc.stdout)
    (OUT / 'catalog.stderr').write_bytes(proc.stderr)
    (OUT / 'catalog.exit').write_text(str(proc.returncode) + '\n')
    if proc.returncode:
        raise RuntimeError('actual sealed catalog export failed')
    entry = json.loads(proc.stdout)
    assert entry['quality']['stored_rows'][0]['batch_id'] == 't6.b1'
    spec = importlib.util.spec_from_file_location('native_collection', ROOT / 'tools/pg-builder/scripts/export_native_collection.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    collection = module.collection([entry], [('ledger','ledger_rows')])
    (OUT / 'collection.json').write_text(json.dumps(collection, indent=2) + '\n')
    print(json.dumps({'run_id':run['run_id'],'source_generation':6,'target_generation':6,
        'batch_id':'t6.b1','catalog_file_sha256':sha((OUT/'collection.json').read_bytes())}))


if __name__ == '__main__':
    main()
