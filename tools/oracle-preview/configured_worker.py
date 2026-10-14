#!/usr/bin/env python3
"""Fixed native worker caller for the isolated configured Oracle recipe."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import native_producer

WORKER=Path('/tmp/oo-dl01w1-74-979367e4020d07fd206a00fc1d00dd953a9c8f86-1bca9536-fixed/job-worker')
WORKER_SHA='1bca9536eb7e6904b6b2dce41ed6bedc9536b6476de62dc6862d0eff8f401809'
LOADER=Path('/private/tmp/oo-dl01w1-79-time-interval-fixed/oo-pipeline-e85f9da679219aa6625a49d45aee62d8578b69299cae0a3503e5bec86f7b15ac')
LOADER_SHA='e85f9da679219aa6625a49d45aee62d8578b69299cae0a3503e5bec86f7b15ac'

def run(base,name,args):
    base=base.resolve()
    for binary,digest in [(WORKER,WORKER_SHA),(LOADER,LOADER_SHA),(native_producer.ADMISSION,native_producer.ADMISSION_SHA)]:
        if hashlib.sha256(binary.read_bytes()).hexdigest()!=digest: raise ValueError('fixed ELF drift')
    command=['docker','run','--rm','--pull','never','--platform','linux/arm64','--label','oo.owner=oracle-connection-whole','--label','oo.kind=proof-runner','--network','container:oo-dl01w1-04-p2-pg','--memory','512m','--cpus','0.5','--cap-drop','ALL','--security-opt','no-new-privileges','-v',str(base)+':'+str(base),'-v','/private/tmp/oo-dl01w1-04-native-runtime-20261012:/run/secrets:ro','-v',str(Path(__file__).resolve().parents[2]/'crates/oo-jobs/migrations')+':/migrations:ro','-v',str(WORKER)+':/opt/job-worker:ro','-v',str(LOADER)+':/opt/loader:ro','-v',str(native_producer.ADMISSION)+':/opt/admission:ro','-v','/private/tmp/oo-dl01w1-04-native-source-cli:/private/tmp/oo-dl01w1-04-native-source-cli:ro',native_producer.IMAGE,'/opt/job-worker','--config','/run/secrets/worker.conf',*args]
    try:
        p=subprocess.run(command,capture_output=True,timeout=120)
    except subprocess.TimeoutExpired as error:
        for suffix,raw in [('stdout',error.stdout or b''),('stderr',error.stderr or b'')]: (base/(name+'.timeout.'+suffix)).write_bytes(raw)
        (base/(name+'.timeout.json')).write_text(json.dumps({'deadline_seconds':120,'raw_exit':None,'client_deadline_exceeded':True})+'\n')
        return 124
    for suffix,raw in [('stdout',p.stdout),('stderr',p.stderr)]: (base/(name+'.'+suffix)).write_bytes(raw)
    (base/(name+'.exit')).write_text(str(p.returncode)+'\n')
    return p.returncode

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--base',type=Path,required=True);parser.add_argument('--name',required=True);parser.add_argument('command',nargs=argparse.REMAINDER);a=parser.parse_args()
    try:sys.exit(run(a.base,a.name,a.command))
    except (ValueError,OSError,subprocess.TimeoutExpired):print('oracle fixed worker refused',file=sys.stderr);sys.exit(2)
