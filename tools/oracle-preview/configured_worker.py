#!/usr/bin/env python3
"""Fixed native worker caller for the isolated configured Oracle recipe."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import uuid
import native_producer
import rust_c5_reader

PROJECT_ROOT=Path(__file__).resolve().parents[2]
WORKER=Path('/tmp/oo-mysql-connection-whole/sol-20261015/bin/f5f6a7857038fffee5cbfe2a78d21f1067cdda3fdc5d1e357735ee37c4fdd346/job-worker')
WORKER_SHA='f5f6a7857038fffee5cbfe2a78d21f1067cdda3fdc5d1e357735ee37c4fdd346'
LOADER=Path('/private/tmp/oo-dl01w1-79-time-interval-fixed/oo-pipeline-e85f9da679219aa6625a49d45aee62d8578b69299cae0a3503e5bec86f7b15ac')
LOADER_SHA='e85f9da679219aa6625a49d45aee62d8578b69299cae0a3503e5bec86f7b15ac'

def run(base,name,args):
    base=base.resolve()
    for binary,digest in [(WORKER,WORKER_SHA),(LOADER,LOADER_SHA),(native_producer.ADMISSION,native_producer.ADMISSION_SHA),(rust_c5_reader.READER,rust_c5_reader.READER_SHA256)]:
        if hashlib.sha256(binary.read_bytes()).hexdigest()!=digest: raise ValueError('fixed ELF drift')
    command=['docker','run','--rm','--pull','never','--platform','linux/arm64','--label','oo.owner=oracle-connection-whole','--label','oo.kind=proof-runner','--network','container:oo-dl01w1-04-p2-pg','--memory','512m','--cap-drop','ALL','--security-opt','no-new-privileges','-v',str(base)+':'+str(base),'-v','/private/tmp/oo-dl01w1-04-native-runtime-20261012:/run/secrets:ro','-v','/Users/redux/.oo-dl01w1-04-p2/reader-window:/oracle-source/reader-password:ro','-v',str(Path(__file__).resolve().parents[2]/'crates/oo-jobs/migrations')+':/migrations:ro','-v',str(WORKER)+':/opt/job-worker:ro','-v',str(LOADER)+':/opt/loader:ro','-v',str(native_producer.ADMISSION)+':/opt/admission:ro','-v',str(rust_c5_reader.READER)+':/opt/oracle-driver-probe:ro','-v','/private/tmp/oo-dl01w1-04-native-source-cli:/private/tmp/oo-dl01w1-04-native-source-cli:ro',native_producer.IMAGE,'/opt/job-worker','--config','/run/secrets/worker.conf',*args]
    try:
        p=subprocess.run(command,capture_output=True,timeout=120)
    except subprocess.TimeoutExpired as error:
        for suffix,raw in [('stdout',error.stdout or b''),('stderr',error.stderr or b'')]: (base/(name+'.timeout.'+suffix)).write_bytes(raw)
        (base/(name+'.timeout.json')).write_text(json.dumps({'deadline_seconds':120,'raw_exit':None,'client_deadline_exceeded':True})+'\n')
        return 124
    for suffix,raw in [('stdout',p.stdout),('stderr',p.stderr)]: (base/(name+'.'+suffix)).write_bytes(raw)
    (base/(name+'.exit')).write_text(str(p.returncode)+'\n')
    return p.returncode

def serve_with_oracle_source_network(base,name,seconds):
    """Run the pinned scheduler on its PG network plus the owned Oracle source network."""
    base=base.resolve()
    for binary,digest in [(WORKER,WORKER_SHA),(LOADER,LOADER_SHA),(native_producer.ADMISSION,native_producer.ADMISSION_SHA),(rust_c5_reader.READER,rust_c5_reader.READER_SHA256)]:
        if hashlib.sha256(binary.read_bytes()).hexdigest()!=digest: raise ValueError('fixed ELF drift')
    container='oo-oracle-source-worker-'+uuid.uuid4().hex[:12]
    create=['docker','create','--pull','never','--platform','linux/arm64','--name',container,'--label','oo.owner=oracle-connection-whole','--label','oo.kind=native-serve','--network','oo-dl01w1-04-net','--memory','512m','--cap-drop','ALL','--security-opt','no-new-privileges','-v',str(base)+':'+str(base),'-v',str(PROJECT_ROOT)+':/workspace:ro','-v','oo-dl01w1-04-c5-target:/t:ro','-v','/private/tmp/oo-dl01w1-04-native-runtime-20261012:/run/secrets:ro','-v','/Users/redux/.oo-dl01w1-04-p2/reader-window:/oracle-source/reader-password:ro','-v',str(PROJECT_ROOT/'crates/oo-jobs/migrations')+':/migrations:ro','-v',str(WORKER)+':/opt/job-worker:ro','-v',str(LOADER)+':/opt/loader:ro','-v',str(native_producer.ADMISSION)+':/opt/admission:ro','-v',str(rust_c5_reader.READER)+':/opt/oracle-driver-probe:ro','-v','/private/tmp/oo-dl01w1-04-native-source-cli:/private/tmp/oo-dl01w1-04-native-source-cli:ro',native_producer.IMAGE,'/bin/sleep','180']
    proc=subprocess.run(create,capture_output=True,timeout=30)
    (base/(name+'.create.stdout')).write_bytes(proc.stdout);(base/(name+'.create.stderr')).write_bytes(proc.stderr);(base/(name+'.create.exit')).write_text(str(proc.returncode)+'\n')
    if proc.returncode: return proc.returncode
    try:
        connect=subprocess.run(['docker','network','connect','oo-dl01w1-04-p2-src',container],capture_output=True,timeout=30)
        (base/(name+'.network.stdout')).write_bytes(connect.stdout);(base/(name+'.network.stderr')).write_bytes(connect.stderr);(base/(name+'.network.exit')).write_text(str(connect.returncode)+'\n')
        if connect.returncode: return connect.returncode
        start=subprocess.run(['docker','start',container],capture_output=True,timeout=30)
        (base/(name+'.start.stdout')).write_bytes(start.stdout);(base/(name+'.start.stderr')).write_bytes(start.stderr);(base/(name+'.start.exit')).write_text(str(start.returncode)+'\n')
        if start.returncode: return start.returncode
        command=['docker','exec',container,'/opt/job-worker','--config','/run/secrets/worker.conf','native-serve','--project',_project_from_base(base),'--worker','oracle-connection-whole','--seconds',str(seconds)]
        try: result=subprocess.run(command,capture_output=True,timeout=max(120,seconds+30))
        except subprocess.TimeoutExpired as error:
            (base/(name+'.stdout')).write_bytes(error.stdout or b'');(base/(name+'.stderr')).write_bytes(error.stderr or b'');(base/(name+'.exit')).write_text('124\n');return 124
        (base/(name+'.stdout')).write_bytes(result.stdout);(base/(name+'.stderr')).write_bytes(result.stderr);(base/(name+'.exit')).write_text(str(result.returncode)+'\n')
        return result.returncode
    finally:
        removed=subprocess.run(['docker','rm','-f',container],capture_output=True,timeout=30)
        (base/(name+'.remove.stdout')).write_bytes(removed.stdout);(base/(name+'.remove.stderr')).write_bytes(removed.stderr);(base/(name+'.remove.exit')).write_text(str(removed.returncode)+'\n')

def _project_from_base(base):
    manifest=base/'manifest.json'
    if not manifest.is_file(): raise ValueError('native serve manifest missing')
    return json.loads(manifest.read_text())['project_id']

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--base',type=Path,required=True);parser.add_argument('--name',required=True);parser.add_argument('command',nargs=argparse.REMAINDER);a=parser.parse_args()
    try:sys.exit(run(a.base,a.name,a.command))
    except (ValueError,OSError,subprocess.TimeoutExpired):print('oracle fixed worker refused',file=sys.stderr);sys.exit(2)
