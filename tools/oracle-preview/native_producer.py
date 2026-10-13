#!/usr/bin/env python3
"""Fixture-only authenticated producer; reuse readonly capture and real admission.

The observer publishes SCN separately. This process never reads its DBA secret,
imports target data, creates leases, or edits acknowledgments. Retries return the
durable occurrence bytes. Source driver orchestration runs on the operator host;
the queue worker receives only the returned admitted request.
"""
import argparse
import hashlib
import hmac
import json
import os
from pathlib import Path
import subprocess
from http.server import BaseHTTPRequestHandler, HTTPServer
import sys
import current_capture

IMAGE = 'docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5'
ADMISSION = Path('/tmp/oo-dl01w1-02-native-admission-43bde81c')
ADMISSION_SHA = '43bde81c78726c9c3aed6bbb9e2d5eaf0a34f3b45d00fb861886f7841d161523'
ROOT = Path('/Users/redux/puppy-one/open-ontology')

def durable(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix('.tmp')
    with temporary.open('wb') as stream:
        stream.write(value)
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(temporary, path)

def produce(config, envelope):
    if envelope.get('protocol') != 'oo.jobs.native-producer/v1':
        raise ValueError('producer protocol')
    binding = envelope['binding']
    item = config['bindings'][binding]
    if envelope['source_id'] != item['source_id'] or envelope['method'] != 'C5':
        raise ValueError('binding identity')
    pin = envelope['pin']
    if not isinstance(pin, int) or pin < 2:
        raise ValueError('existing acknowledged binding required')
    occurrence = Path(config['occurrences']) / binding / str(pin)
    saved = occurrence / 'request.json'
    if saved.exists():
        if json.loads((occurrence/'envelope.json').read_text()) != envelope:
            raise ValueError('immutable occurrence identity')
        return saved.read_bytes()
    parent = envelope.get('acknowledged_source_generation')
    if not isinstance(parent, int) or parent < 1:
        raise ValueError('real acknowledgment required')
    previous = Path(item['baseline']) if parent == 1 else Path(config['occurrences']) / binding / str(envelope['acknowledged_pin']) / 'capture'
    if json.loads((previous/'capture.json').read_text())['generation'] != parent:
        raise ValueError('acknowledged capture mismatch')
    occurrence.mkdir(parents=True, exist_ok=True)
    durable(occurrence/'envelope.json', json.dumps(envelope, sort_keys=True).encode())
    capture = occurrence/'capture'
    current_capture.run(Path(config['scn_file']).read_text().strip(), capture, parent+1, previous)
    source = item['source_id']
    witness = capture/'witness'/source
    sealed = occurrence/'sealed'
    if hashlib.sha256(ADMISSION.read_bytes()).hexdigest() != ADMISSION_SHA:
        raise ValueError('admission pin')
    command = ['docker','run','--rm','--network','none','-v',str(ROOT)+':'+str(ROOT),
               '-v',str(ADMISSION)+':/opt/admission:ro',
               '-v','/private/tmp/oo-dl01w1-04-native-source-cli:/private/tmp/oo-dl01w1-04-native-source-cli:ro',
               IMAGE,'/opt/admission','seal-native','--report',str(witness/'report.json'),
               '--config',str(witness/'config.json'),'--out',str(sealed)]
    result = subprocess.run(command, capture_output=True, timeout=60)
    durable(occurrence/'seal.stdout', result.stdout)
    durable(occurrence/'seal.stderr', result.stderr)
    durable(occurrence/'seal.exit', str(result.returncode).encode())
    if result.returncode:
        raise ValueError('real admission refused')
    registry = json.loads((sealed/'registry.json').read_text())
    entry = registry['bindings'][0]
    request = json.loads(Path(item['baseline_request']).read_text())
    request.update({key:entry[key] for key in ('config_digest','schema_digest','type_digest','input_digest','snapshot')})
    request.update(source_generation=parent+1,parent_source_generation=parent,batch_id='t'+str(parent+1)+'.b0',
                   arrow_path=str(capture/'run/arrow'/('t_'+source.removeprefix('oo04_native_')+'-gen-'+str(parent+1)+'.arrow')),
                   audit_digest=entry['capture_receipt_sha256'],admission_path=entry['admission_receipt_path'],
                   registry_path=str(occurrence/'registry.json'),
                   admission_expect={'source_engine':'oracle','source_id':source,'source_generation':parent+1,
                                     'declared_receipt_sha256':entry['admission_receipt_sha256']})
    request['columns']=[dict(field,logical_name=field['name']) for field in entry['fields']]
    # A full replacement reconciles physical deletes; keyless tombstones are source evidence only.
    request['tombstones'] = []
    durable(occurrence/'registry.json', (sealed/'registry.json').read_bytes())
    durable(saved, json.dumps(request,indent=2).encode()+b'\n')
    return saved.read_bytes()

def main():
    parser=argparse.ArgumentParser();parser.add_argument('--config',type=Path,required=True)
    args=parser.parse_args();config=json.loads(args.config.read_text())
    token=Path(config['token_file']).read_text().strip()
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_): pass
        def do_POST(self):
            if self.path != '/capture' or not hmac.compare_digest(self.headers.get('Authorization',''), 'Bearer '+token):
                self.send_error(403);return
            try:
                length=int(self.headers.get('Content-Length','0'))
                if not 0 < length <= 4096: raise ValueError('envelope budget')
                response=produce(config,json.loads(self.rfile.read(length)))
            except Exception as error:
                print('producer_refused='+type(error).__name__,file=sys.stderr,flush=True)
                self.send_error(422,'producer refused');return
            self.send_response(200);self.send_header('Content-Type','application/json')
            self.send_header('Content-Length',str(len(response)));self.end_headers();self.wfile.write(response)
    HTTPServer((config['listen_host'],config['port']),Handler).serve_forever()

if __name__=='__main__': main()
