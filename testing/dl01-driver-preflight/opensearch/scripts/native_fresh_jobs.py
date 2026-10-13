#!/usr/bin/env python3
"""Run the frozen real worker against the source-owned live C5 producer."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import uuid

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('target', HERE / 'native_fresh_target.py')
t = importlib.util.module_from_spec(spec)
spec.loader.exec_module(t)
ELF = Path('/tmp/oo-native-capture-worker-20261012-r6')
SHA = 'f58498f695b859e015b3b17d5a1f4e7a3c28af823e7c45f3f391d918e7d77b05'
PIPE = Path('/tmp/oo-native-loader-20261012-linux194')
PIPE_SHA = 'daca92824e5dde50d58f1bc81fadddbe49b303f0847f016e24301d2f06fb6903'
ADM = Path('/tmp/oo-dl01w1-02-native-admission-final2')
ADM_SHA = 'f3353f1d13e68990693818660cb9eec180961dde66dd39f84d94ef7d80366081'
PROJECT = '5d3f04f6-15de-4cf5-af60-62f24974cc76'
OUT = t.BASE / 'jobs'


def worker(name, args):
    t.phase('worker-' + name, ['docker', 'run', '--rm', '-i', '--pull', 'never',
        '--network', 'container:' + t.NAME, '--user', '0:0', '--security-opt', 'no-new-privileges',
        '--cap-drop', 'ALL', '--entrypoint', '/opt/job-worker',
        '-v', str(ELF) + ':/opt/job-worker:ro', '-v', str(PIPE) + ':/opt/oo-pipeline:ro',
        '-v', str(ADM) + ':/opt/oo-source-admission:ro',
        '-v', str(t.ROOT) + ':' + str(t.ROOT) + ':ro', '-v', str(t.BASE) + ':' + str(t.BASE),
        '-v', str(t.PRIVATE) + ':/private:ro', 'oo-a395-selfbuild:local',
        '--config', '/private/native-worker.conf', *args])


def main():
    for path, digest in ((ELF, SHA), (PIPE, PIPE_SHA), (ADM, ADM_SHA)):
        if hashlib.sha256(path.read_bytes()).hexdigest() != digest:
            raise SystemExit('shared executable digest drift')
    OUT.mkdir(exist_ok=False)
    config = '\n'.join(['host=127.0.0.1','port=5432','database=' + t.DB,
        'runtime_user=oo_dl01w1_44_runtime','runtime_password_file=/private/native-runtime.pw',
        'admin_user=postgres','admin_password_file=/private/native-admin.pw',
        'ca_file=/private/certs/ca.pem','migrations=/unused-not-read-by-native-commands',''])
    private_worker = t.PRIVATE / 'native-worker.conf'
    private_worker.write_text(config)
    os.chmod(private_worker, 0o600)
    producer = HERE / 'native_c5_product_producer.py'
    witness = HERE / 'emit_capture_witness.py'
    capture_root = t.BASE / 'scheduled-source'
    producer_config = {'reader_bin': str(t.BASE.parent / 'native-20261012/bin/os-reader'),
        'reader_sha256': '231fdce4f6da57e8c6f980a718bb72e9cdccda51f1e3fe5e7eb56ea584572b2f',
        'admission_bin': '/opt/oo-source-admission', 'admission_sha256': ADM_SHA,
        'worker_bin': '/opt/job-worker', 'worker_sha256': SHA, 'worker_config': '/private/native-worker.conf',
        'witness_module': str(witness), 'witness_sha256': hashlib.sha256(witness.read_bytes()).hexdigest(),
        'schedule_path': str(t.ROOT / 'testing/dl01-driver-preflight/opensearch/fixtures/c5-schedule.json'),
        'approved_registry_path': str(t.ROOT / 'testing/dl01-driver-preflight/opensearch/fixtures/customer-request.json'),
        'private_path': '/private', 'capture_root': str(capture_root),
        'target': {'host':'127.0.0.1','port':5432,'database':t.DB,
            'runtime_principal':'oo_dl01w1_44_runtime','password_file':'/private/native-runtime.pw',
            'ca_file':'/private/certs/ca.pem','ssl':'verify-full',
            'owner':PROJECT,'shard':'shard_oo_dl01w1_44'}}
    raw = (json.dumps(producer_config) + '\n').encode()
    private_producer = t.PRIVATE / 'native-producer.json'
    private_producer.write_bytes(raw)
    os.chmod(private_producer,0o600)
    binding = str(uuid.uuid4())
    manifest = {'protocol':'oo.jobs.native-schedule/v1','binding':binding,'project_id':PROJECT,
        'source_id':'oo_dl01w1_44','method':'C5','cycle':'pull','period_ms':1500,
        'producer': {'path':str(producer),'sha256':hashlib.sha256(producer.read_bytes()).hexdigest(),
            'args':['--config','/private/native-producer.json','--config-sha256',hashlib.sha256(raw).hexdigest()]},
        'loader': {'path':'/opt/oo-pipeline','sha256':PIPE_SHA,'args':['import-native-update']},
        'registration':{'admission_binary':'/opt/oo-source-admission','admission_sha256':ADM_SHA,
            'registry_path':str(capture_root / '{binding}/{pin}/admission/registry.json')}}
    (OUT / 'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
    worker('install', ['native-install'])
    worker('put', ['native-put','--manifest',str(OUT/'manifest.json')])
    worker('fire', ['native-fire','--project',PROJECT])
    worker('first-step', ['native-step','--project',PROJECT,'--worker','opensearch-native-owner44'])
    worker('state-first', ['native-state','--binding',binding])
    print('ACTUAL_NATIVE_JOB_FIRST_STEP_DONE: inspect state and independent full cells before claiming import')


if __name__ == '__main__':
    main()
