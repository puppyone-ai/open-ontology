#!/usr/bin/env python3
"""Commit the native partition ACK only after jobs.complete_success CAS."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path('/Users/redux/puppy-one/open-ontology')
ELF = Path('/tmp/oo-dl01w1-79-time-interval-fixed/oo-pipeline-e85f9da679219aa6625a49d45aee62d8578b69299cae0a3503e5bec86f7b15ac')
IMAGE = 'rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5'

def main():
    p=argparse.ArgumentParser()
    p.add_argument('--ack',type=Path,required=True)
    p.add_argument('--target',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True)
    a=p.parse_args()
    ack=json.loads(a.ack.read_text())
    if ack.get('protocol')!='oo.pipeline.native-partition-ack/v1':
        raise SystemExit('wrong ACK protocol')
    if hashlib.sha256(ELF.read_bytes()).hexdigest()!=ELF.name.rsplit('-',1)[1]:
        raise SystemExit('fixed ELF changed')
    digest=hashlib.sha256(a.target.read_bytes()).hexdigest()
    a.output.mkdir(parents=True,exist_ok=False)
    cmd=['docker','run','-i','--rm','--pull','never',
        '--network','container:oo-dl01w1-40-gpt-fresh-pg',
        '--mount','type=bind,source=/tmp/oo-dl01w1-40-native-runtime,target=/private,readonly',
        '--mount',f'type=bind,source={ROOT},target={ROOT},readonly',
        '--mount',f'type=bind,source={ELF},target=/bin/oo-pipeline,readonly',
        '-e',f'OO_NATIVE_CONTROL_TARGET={a.target.resolve()}',
        '-e',f'OO_NATIVE_CONTROL_TARGET_SHA256={digest}',
        '-e',f'OO_NATIVE_ACK_TARGET={a.target.resolve()}',
        '-e',f'OO_NATIVE_ACK_TARGET_SHA256={digest}',
        '--entrypoint','/bin/oo-pipeline',IMAGE,'ack-native-partition']
    r=subprocess.run(cmd,input=a.ack.read_bytes(),capture_output=True)
    (a.output/'stdout.json').write_bytes(r.stdout)
    (a.output/'stderr.txt').write_bytes(r.stderr)
    (a.output/'exit.txt').write_text(str(r.returncode)+'\n')
    print(r.stdout.decode('utf-8','replace').strip())
    raise SystemExit(r.returncode)

if __name__=='__main__':
    main()
