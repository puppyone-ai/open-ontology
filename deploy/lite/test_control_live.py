#!/usr/bin/env python3
"""Owned PostgreSQL status regression, not seven-service acceptance."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import subprocess

import lifecycle

ROOT=Path(__file__).resolve().parents[2]
PROJECT='00000000-0000-0000-0000-000000000001'

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--run-id',required=True)
    args=parser.parse_args()
    if not re.fullmatch('[a-z0-9-]+',args.run_id):
        parser.error('invalid run ID')
    os.umask(0o077)
    out=ROOT/'evidence'/args.run_id
    out.mkdir(mode=0o700)
    root=out/'lite'
    (root/'work/operator').mkdir(parents=True,mode=0o700)
    lifecycle.write(root/'work/operator/schedule.json',json.dumps({'project':PROJECT}).encode())
    lifecycle.write(out/'password',secrets.token_hex(32).encode())
    project='oo-a-'+args.run_id
    env=os.environ.copy()
    subnet=lifecycle.subnet(project,env)
    compose={'services':{'control-db':{
        'image':'docker.io/library/postgres@sha256:9b18b78397054fce88a9552e9d5a3ad5bb7fd258c5b3cc1c5028e46373d6ea8f',
        'labels':{'oo.lane':'a'},
        'environment':{'POSTGRES_PASSWORD_FILE':'/run/password'},
        'volumes':[f'{out}/password:/run/password:ro'],
        'tmpfs':['/var/lib/postgresql/data'],
        'mem_limit':'512m',
        'healthcheck':{'test':['CMD','pg_isready','-U','postgres'],'interval':'1s','retries':60}}},
        'networks':{'default':{'labels':{'oo.lane':'a'},
                              'ipam':{'config':[{'subnet':subnet}]}}}}
    lifecycle.write(out/'compose.json',json.dumps(compose).encode())
    argv=['compose','-p',project,'-f',str(out/'compose.json')]

    def sql(text):
        proc=subprocess.run(['docker',*argv,'exec','-T','control-db','psql','-X','-qAt',
                             '-v','ON_ERROR_STOP=1','-U','postgres','-d','postgres'],
                            input=text.encode(),capture_output=True,timeout=30)
        with (out/'sql.log').open('ab') as log:
            log.write(proc.stdout+proc.stderr)
        if proc.returncode:
            raise RuntimeError('fixture SQL failed; see sql.log')
        return proc.stdout.decode().strip()

    def status(name):
        data=lifecycle.control_status(root,argv,env)
        (out/(name+'.json')).write_text(json.dumps(data,indent=2))
        return data

    try:
        lifecycle.docker(argv+['up','-d','--wait','--wait-timeout','90'],env)
        assert status('missing-schema')['schedule_status']=='unavailable'
        migrations=ROOT/'crates/oo-jobs/migrations'
        install=['BEGIN; SELECT pg_advisory_xact_lock(6401202601);']
        versions=['jobs-bootstrap/1','jobs-control/1','jobs-event-project/1','jobs-lease-operations/1']
        files=['0000_bootstrap_roles.sql','0001_jobs_control_expand.sql',
               '0002_event_project_binding_expand.sql','0003_job_lease_operations_expand.sql']
        for index,name in enumerate(files):
            if index==1:
                install.append('SET LOCAL ROLE oo_jobs_owner;')
            install.append((migrations/name).read_text())
        for version,name in zip(versions,files):
            digest=hashlib.sha256((migrations/name).read_bytes()).hexdigest()
            install.append("INSERT INTO jobs.schema_migrations(version,checksum,phase,filename)"
                           f" VALUES ('{version}','{digest}','expand','{name}');")
        install.append('COMMIT;')
        sql('\n'.join(install))
        initial=status('empty')
        assert initial['schedule_status']=='measured'
        assert all(initial[key] is None for key in ('last_success','last_failure','next_due'))
        # The fixture administers a schedule; no source connection exists.
        schedule=sql(f"""SET ROLE oo_jobs_runtime;
INSERT INTO jobs.schedules(project_id,kind,config_version,artifact_digest,dedupe_prefix,next_run_at)
VALUES ('{PROJECT}','lite_noop','v1',repeat('a',64),'lite',clock_timestamp())
RETURNING id;""")
        before=status('scheduled')
        assert before['next_due'] is not None and before['last_success'] is None
        for state in ('success','failure'):
            sql(f"""SET ROLE oo_jobs_runtime;
BEGIN;
SELECT * FROM jobs.enqueue('{PROJECT}','lite_noop',1,'{{}}','v1',repeat('a',64),
                          '{state}',clock_timestamp(),1) \\gset task_
INSERT INTO jobs.schedule_occurrences(schedule_id,scheduled_at,project_id,task_id)
SELECT id,next_run_at,project_id,:'task_task_id'::uuid FROM jobs.schedules WHERE id='{schedule}';
UPDATE jobs.schedules SET next_run_at=clock_timestamp()+interval '30 seconds' WHERE id='{schedule}';
COMMIT;
SELECT * FROM jobs.claim('{PROJECT}',ARRAY['lite_noop'],'fixture') \\gset lease_
SELECT status FROM jobs.complete_{state}('{PROJECT}',:'lease_task_id','fixture',:lease_generation
{", 'dependency_unavailable'" if state=='failure' else ''});
""")
            observed=status(state)
            assert observed['last_'+state] is not None
        # Neither a disabled nor another project's schedule is a next due.
        sql(f"""SET ROLE oo_jobs_runtime;
UPDATE jobs.schedules SET enabled=false WHERE id='{schedule}';
INSERT INTO jobs.schedules(project_id,kind,config_version,artifact_digest,dedupe_prefix,next_run_at)
VALUES ('00000000-0000-0000-0000-000000000002','lite_noop','v1',repeat('a',64),'other',clock_timestamp());""")
        disabled=status('disabled')
        assert disabled['next_due'] is None
        assert disabled['last_success']==observed['last_success']
        assert disabled['last_failure']==observed['last_failure']
        version=sql('SHOW server_version;')
        (out/'result.json').write_text(json.dumps({
            'result':'passed','engine':'PostgreSQL','version':version,
            'scope':'real control DB queries and fixture-driven scheduled no-op job transitions',
            'seven_service_acceptance':False,'production_scheduler_tested':False},indent=2))
    finally:
        ids=lifecycle.docker(argv+['ps','-aq'],env).decode().split()
        for cid in ids:
            labels=json.loads(lifecycle.docker(['inspect','--format','{{json .Config.Labels}}',cid],env))
            assert labels.get('oo.lane')=='a' and labels.get('com.docker.compose.project')==project
        lifecycle.docker(argv+['down'],env)
        (out/'password').unlink()

if __name__=='__main__':
    main()
