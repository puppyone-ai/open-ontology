#!/usr/bin/env python3
"""Reproduce the shared token client's mTLS blocker on an owned lite DB."""
import argparse
import json
import os
from pathlib import Path
import re
import secrets
import shutil
import subprocess

import lifecycle

ROOT=Path(__file__).resolve().parents[2]

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--run-id',required=True)
    args=parser.parse_args()
    if not re.fullmatch('[a-z0-9-]+',args.run_id):
        parser.error('invalid run ID')
    os.umask(0o077)
    out=ROOT/'evidence'/args.run_id
    out.mkdir(mode=0o700)
    pki=out/'pki'
    pki.mkdir(mode=0o700)
    password=secrets.token_hex(32)
    lifecycle.write(pki/'lite-admin-password',password.encode())

    def openssl(*arguments):
        subprocess.run(['openssl',*arguments],check=True,capture_output=True,cwd=pki)

    openssl('req','-x509','-newkey','rsa:2048','-nodes','-days','1','-subj','/CN=lite-test-ca',
            '-keyout','ca-key','-out','lite-ca-cert')
    for name,usage in [('control-db','serverAuth'),('admin','clientAuth')]:
        stem='lite-'+name
        openssl('req','-newkey','rsa:2048','-nodes','-subj','/CN='+name,
                '-keyout',stem+'-key','-out',stem+'.csr')
        (pki/'extensions').write_text('subjectAltName=DNS:localhost,DNS:control-db\n'
                                    'extendedKeyUsage='+usage+'\n')
        openssl('x509','-req','-in',stem+'.csr','-CA','lite-ca-cert','-CAkey','ca-key',
                '-CAcreateserial','-days','1','-extfile','extensions','-out',stem+'-cert')
    env=os.environ.copy()
    project='oo-a-'+args.run_id
    compose={'services':{'control-db':{
        'image':'docker.io/library/postgres@sha256:9b18b78397054fce88a9552e9d5a3ad5bb7fd258c5b3cc1c5028e46373d6ea8f',
        'labels':{'oo.lane':'a'},'entrypoint':['/bin/sh','/ops/postgres.sh'],
        'environment':{'OO_DB_IDENTITY':'control-db','POSTGRES_PASSWORD_FILE':'/tmp/admin-password'},
        'volumes':[f'{pki}:/pki:ro',f'{ROOT}/deploy/lite/postgres.sh:/ops/postgres.sh:ro'],
        'ports':['127.0.0.1::5432'],'tmpfs':['/var/lib/postgresql/data','/tmp'],
        'mem_limit':'512m',
        'healthcheck':{'test':['CMD','pg_isready','-U','postgres'],'interval':'1s','retries':60}}},
        'networks':{'default':{'labels':{'oo.lane':'a'},
                              'ipam':{'config':[{'subnet':lifecycle.subnet(project,env)}]}}}}
    lifecycle.write(out/'compose.json',json.dumps(compose).encode())
    argv=['compose','-p',project,'-f',str(out/'compose.json')]
    try:
        lifecycle.docker(argv+['up','-d','--wait','--wait-timeout','90'],env)
        port=int(lifecycle.docker(argv+['port','control-db','5432'],env).decode().strip().rsplit(':',1)[1])
        lifecycle.write(out/'endpoint.json',json.dumps({
            'host':'localhost','port':port,'database':'postgres'}).encode())
        # Positive control: the same DB admits a CA-verified client certificate.
        shell='''export PGPASSWORD="$(cat /pki/lite-admin-password)"
export PGSSLMODE=verify-full PGSSLROOTCERT=/pki/lite-ca-cert
export PGSSLCERT=/pki/lite-admin-cert PGSSLKEY=/pki/lite-admin-key
exec psql -X -qAt -h localhost -U postgres -d postgres -c 'SELECT version();'
'''
        positive=subprocess.run(['docker',*argv,'exec','-T','control-db','sh','-c',shell],
                                capture_output=True,timeout=20)
        (out/'positive.log').write_bytes(positive.stdout+positive.stderr)
        assert positive.returncode==0
        issue=subprocess.run([str(ROOT/'target/debug/issue-token'),'install',
                              str(out/'endpoint.json'),str(pki/'lite-admin-password'),
                              str(pki/'lite-ca-cert')],capture_output=True,timeout=30)
        (out/'issue-token.log').write_bytes(issue.stdout+issue.stderr)
        logs=lifecycle.docker(argv+['logs','--no-color','control-db'],env)
        (out/'database.log').write_bytes(logs.replace(password.encode(),b'[REDACTED]'))
        assert issue.returncode!=0, 'shared client behavior changed; recheck integration'
        assert b'valid client certificate' in logs, 'failure was not the expected mTLS refusal'
        result={'status':'blocked','positive_mtls_connection':'passed',
                'issue_token_install_exit':issue.returncode,
                'reason':'token-store client provides no client certificate',
                'tls_policy_weakened':False,'operator_token_issued':False}
        (out/'result.json').write_text(json.dumps(result,indent=2))
    finally:
        for cid in lifecycle.docker(argv+['ps','-aq'],env).decode().split():
            labels=json.loads(lifecycle.docker(['inspect','--format','{{json .Config.Labels}}',cid],env))
            assert labels.get('oo.lane')=='a' and labels.get('com.docker.compose.project')==project
        lifecycle.docker(argv+['down'],env)
        shutil.rmtree(pki)

if __name__=='__main__':
    main()
