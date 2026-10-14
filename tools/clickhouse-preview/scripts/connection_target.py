#!/usr/bin/env python3
"""Provision a fresh owned target, then install the fixed native product schema."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import time
import uuid

os.environ.setdefault('DOCKER_CONFIG', '/tmp/oo-ch-docker-config')

import fresh_capture as f

def mount_path(path):
    """Docker Desktop rejects the /private/tmp alias of this host's /tmp."""
    text = str(path)
    prefix = '/private/tmp/'
    if text.startswith(prefix):
        return '/tmp/' + text[len(prefix):]
    return text

PG_IMAGE='postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb'
PIPELINE=Path('/tmp/oo-dl01w1-79-time-interval-fixed/oo-pipeline-e85f9da679219aa6625a49d45aee62d8578b69299cae0a3503e5bec86f7b15ac')
ADMISSION=Path('/tmp/oo-dl01w1-79-time-interval-fixed/oo-source-admission-cfc11a13d0ff14e377e29b687dbb60adb4ced05476c48143fed1637b841bab67')

class Target:
    def __init__(self,config,run):
        self.config=config;self.out=Path(mount_path(run));self.pg=config['target_container']
        self.private=Path(mount_path(config.get('private_dir', self.out/'target-private')))
        self.loader=Path(config.get('loader_bin', PIPELINE))
        self.admission=Path(config.get('admission_bin', ADMISSION))
    def available(self,label):
        candidate=label;number=1
        while (self.out/'target-raw'/f'{candidate}.stdout').exists():
            number+=1;candidate=f'{label}-attempt{number}'
        return candidate
    def exec(self,label,cmd,data=None):
        label=self.available(label)
        r=subprocess.run(cmd,input=data,capture_output=True,timeout=180)
        directory=self.out/'target-raw';directory.mkdir(exist_ok=True)
        for suffix,b in [('stdout',r.stdout),('stderr',r.stderr),('exit',f'{r.returncode}\n'.encode())]:
            (directory/f'{label}.{suffix}').write_bytes(b)
        if r.returncode:raise RuntimeError(label+' failed; private raw retained')
        return r.stdout
    def psql(self,label,sql,database='oo_ch_connection'):
        return self.exec(label,['docker','exec','-i',self.pg,'psql','-U','postgres','-d',database,'-X','-q','-t','-A','-v','ON_ERROR_STOP=1'],sql.encode())
    def pipeline(self,label,command,request,registry=None):
        label=self.available(label)
        request_file=self.out/'target-raw'/f'{label}.request.json'
        request_file.write_text(json.dumps(request,indent=2)+'\n')
        cmd=['docker','run','--rm','-i','--pull','never','--network','container:'+self.pg,
            '--mount',f'type=bind,source={mount_path(self.out)},target={mount_path(self.out)}',
            '--mount',f'type=bind,source={mount_path(self.private)},target=/secret,readonly',
            '--mount',f'type=bind,source={f.ROOT},target={f.ROOT},readonly',
            '--mount','type=bind,source=/tmp/oo-dl01w1-40-gpt-producer,target=/tmp/oo-dl01w1-40-gpt-producer,readonly',
            '--mount',f'type=bind,source={self.loader},target=/bin/oo-pipeline,readonly',
            '--mount',f'type=bind,source={self.admission},target=/bin/oo-source-admission,readonly',
            '-e','OO_NATIVE_ADMISSION_BIN=/bin/oo-source-admission']
        for src in self.config.get('extra_mounts', []):
            cmd+=['--mount', f'type=bind,source={mount_path(src)},target={mount_path(src)}']
        if registry:
            cmd+=['-e',f'OO_NATIVE_BINDING_REGISTRY={registry}',
                '-e',f'OO_NATIVE_BINDING_REGISTRY_SHA256={hashlib.sha256(Path(registry).read_bytes()).hexdigest()}']
        for name in ('OO_NATIVE_CONTROL_TARGET','OO_NATIVE_ACK_TARGET'):
            path=self.out/'ack-target.json'
            if path.exists():cmd+=['-e',f'{name}={path}','-e',f'{name}_SHA256={hashlib.sha256(path.read_bytes()).hexdigest()}']
        cmd+=['--entrypoint','/bin/oo-pipeline',f.RUST_IMAGE,command]
        return json.loads(self.exec(label,cmd,request_file.read_bytes()))
    def setup(self):
        for binary in (PIPELINE,ADMISSION):
            if hashlib.sha256(binary.read_bytes()).hexdigest()!=binary.name.rsplit('-',1)[1]:raise RuntimeError('fixed binary drift')
        self.private.mkdir(mode=0o700,exist_ok=False)
        previous=Path('/tmp/oo-dl01w1-40-native-runtime')
        for name in ('admin.password','loader.password'):
            shutil.copyfile(previous/name,self.private/name);(self.private/name).chmod(0o600)
        shutil.copytree(previous/'tls',self.private/'tls')
        self.exec('start',['docker','run','-d','--pull','never','--name',self.pg,
            '--network',self.config['network'],'--label','oo.owner=oo-clickhouse-connection-whole',
            '--label','oo.kind=owned-agent-db','-e','POSTGRES_DB=oo_ch_connection',
            '-e','POSTGRES_PASSWORD_FILE=/secret/admin.password',
            '--mount',f'type=bind,source={self.private},target=/secret,readonly',
            '--mount',f'type=bind,source={self.private}/tls,target=/tls-in,readonly',
            '--mount',f'type=bind,source={f.ROOT}/tools/selfbuild-control-api/linux/pg-entry.sh,target=/pg-entry.sh,readonly',
            '--entrypoint','/bin/sh',PG_IMAGE,'/pg-entry.sh'])
        for _ in range(60):
            r=subprocess.run(['docker','exec',self.pg,'pg_isready','-U','postgres'],capture_output=True)
            if r.returncode==0:break
            time.sleep(1)
        else:raise RuntimeError('target not ready')
        migrations=f.ROOT/'crates/oo-jobs/migrations'
        boot=migrations/'0000_bootstrap_roles.sql'
        first=migrations/'0001_jobs_control_expand.sql'
        sql='BEGIN; SELECT pg_advisory_xact_lock(6401202601);\n'+boot.read_text()+'\nSET ROLE oo_jobs_owner;\n'+first.read_text()
        for path,version in ((boot,'jobs-bootstrap/1'),(first,'jobs-control/1')):
            sql+=f"\nINSERT INTO jobs.schema_migrations(version,checksum,phase,filename) VALUES ('{version}','{hashlib.sha256(path.read_bytes()).hexdigest()}','expand','{path.name}');"
        self.psql('jobs-bootstrap',sql+'\nRESET ROLE; COMMIT;')
        versions={'0002_event_project_binding_expand.sql':'jobs-event-project/1','0003_job_lease_operations_expand.sql':'jobs-lease-operations/1','0004_loader_hold_expand.sql':'jobs-loader-hold/1'}
        for filename,version in versions.items():
            migration=migrations/filename
            sql='BEGIN; SET ROLE oo_jobs_owner; SELECT pg_advisory_xact_lock(6401202601); SELECT id FROM jobs.migration_lock WHERE id=1 FOR UPDATE;\n'+migration.read_text()
            sql+=f"\nINSERT INTO jobs.schema_migrations(version,checksum,phase,filename) VALUES ('{version}','{hashlib.sha256(migration.read_bytes()).hexdigest()}','expand','{migration.name}'); RESET ROLE; COMMIT;"
            self.psql(migration.stem,sql)
        target={'host':'127.0.0.1','port':5432,'database':'oo_ch_connection',
            'runtime_principal':'postgres','password_file':'/secret/admin.password',
            'ssl':'verify-full','ca_file':'/secret/tls/ca.crt',
            'shard':'shard_ch_connection','owner':str(uuid.uuid4())}
        result=self.pipeline('install','install-native-update',{'protocol':'oo.pipeline.install-native-update/v1','target':target})
        if not result.get('installed'):raise RuntimeError('native install refused')
        password=(self.private/'loader.password').read_text().strip()
        # Password-bearing SQL is never retained in evidence or printed.
        sql=f"CREATE ROLE oo_ch_connection_loader LOGIN PASSWORD '{password.replace(chr(39),chr(39)*2)}'; GRANT oo_pipeline_runtime,oo_jobs_runtime TO oo_ch_connection_loader; GRANT CONNECT ON DATABASE oo_ch_connection TO oo_ch_connection_loader;"
        r=subprocess.run(['docker','exec','-i',self.pg,'psql','-U','postgres','-d','oo_ch_connection','-v','ON_ERROR_STOP=1'],input=sql.encode(),capture_output=True)
        (self.out/'target-raw/loader-role.exit').write_text(f'{r.returncode}\n')
        if r.returncode:raise RuntimeError('loader role failed')
        target.update(runtime_principal='oo_ch_connection_loader',password_file='/secret/loader.password')
        (self.out/'loader-target.json').write_text(json.dumps(target,indent=2)+'\n')
        owner_target=dict(target,runtime_principal='postgres',password_file='/secret/admin.password')
        (self.out/'ack-target.json').write_text(json.dumps(owner_target,indent=2)+'\n')
        (self.out/'target-manifest.json').write_text(json.dumps({'target_container':self.pg,'target':target,'fresh_database':True,'install':result},indent=2)+'\n')

def main():
    p=argparse.ArgumentParser();p.add_argument('--config',type=Path,required=True);p.add_argument('--run',type=Path,required=True);a=p.parse_args()
    Target(json.loads(a.config.read_text()),a.run.resolve()).setup()

if __name__=='__main__':main()
