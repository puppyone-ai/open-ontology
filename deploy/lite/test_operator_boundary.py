#!/usr/bin/env python3
"""Verify D14 TLS/SCRAM and real token issuance on an owned lite DB."""
import argparse
import json
import os
from pathlib import Path
import re
import secrets
import shutil
import subprocess
import time

import lifecycle

ROOT=Path(__file__).resolve().parents[2]

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--run-id',required=True)
    parser.add_argument('--gateway-boundary',action='store_true')
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
            '-keyout','ca-key','-out','lite-ca-cert',
            '-addext','basicConstraints=critical,CA:TRUE',
            '-addext','keyUsage=critical,keyCertSign,cRLSign')
    for name,usage in [('control-db','serverAuth'),('admin','clientAuth')]:
        stem='lite-'+name
        san='DNS:localhost,DNS:control-db' if name=='control-db' else 'URI:urn:oo:workload:lite-operator'
        openssl('req','-newkey','rsa:2048','-nodes','-subj','/CN='+name,
                '-keyout',stem+'-key','-out',stem+'.csr')
        (pki/'extensions').write_text('basicConstraints=critical,CA:FALSE\n'
                                    'keyUsage=critical,digitalSignature,keyEncipherment\n'
                                    'subjectKeyIdentifier=hash\nauthorityKeyIdentifier=keyid,issuer\n'
                                    'subjectAltName='+san+'\n'
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
        # Provision only fixture roles over the local admin socket. Passwords
        # stay in files/stdin, never argv or database statement logs.
        sql="""SET log_statement='none';
SET log_min_error_statement='panic';
SELECT format('CREATE ROLE oo_token_admin LOGIN PASSWORD %L',
              pg_read_file('/tmp/admin-password')) \\gexec
SELECT format('CREATE ROLE oo_token_runtime LOGIN PASSWORD %L',
              pg_read_file('/tmp/admin-password')) \\gexec
GRANT CREATE ON DATABASE postgres TO oo_token_admin;
"""
        provision=subprocess.run(['docker',*argv,'exec','-T','control-db','psql',
                                  '-X','-q','-v','ON_ERROR_STOP=1','-U','postgres'],
                                 input=sql.encode(),capture_output=True,timeout=20)
        assert provision.returncode==0, 'fixture role provisioning failed'
        issue=subprocess.run([str(ROOT/'target/debug/issue-token'),'install',
                              str(out/'endpoint.json'),str(pki/'lite-admin-password'),
                              str(pki/'lite-ca-cert')],capture_output=True,timeout=30)
        (out/'issue-token.log').write_bytes(issue.stdout+issue.stderr)
        logs=lifecycle.docker(argv+['logs','--no-color','control-db'],env)
        (out/'database.log').write_bytes(logs.replace(password.encode(),b'[REDACTED]'))
        assert issue.returncode==0, 'TLS/SCRAM token store install failed'
        grant={'kind':'client','project_id':'lite-boundary','principal_id':'operator',
               'role':'lite_operator','claims':[],'policy_version':'bootstrap',
               'knowledge_version':'unpublished','audience_url':'https://localhost:8443/',
               'purpose':'owned D14 regression'}
        lifecycle.write(out/'grant.json',json.dumps(grant).encode())
        issued=subprocess.run([str(ROOT/'target/debug/issue-token'),'issue',
                               str(out/'endpoint.json'),str(pki/'lite-admin-password'),
                               str(pki/'lite-ca-cert'),str(out/'grant.json'),
                               str(pki/'operator-token')],capture_output=True,timeout=30)
        (out/'issued.log').write_bytes(issued.stdout+issued.stderr)
        assert issued.returncode==0, 'real token issuance failed'
        token=lifecycle.read_private(pki/'operator-token').strip()
        assert token.startswith(b'ooct_')
        openssl('req','-x509','-newkey','rsa:2048','-nodes','-days','1',
                '-subj','/CN=untrusted-ca','-keyout','untrusted-key','-out','untrusted-cert')
        untrusted=subprocess.run([str(ROOT/'target/debug/issue-token'),'install',
                                  str(out/'endpoint.json'),str(pki/'lite-admin-password'),
                                  str(pki/'untrusted-cert')],capture_output=True,timeout=30)
        (out/'untrusted-ca.log').write_bytes(untrusted.stdout+untrusted.stderr)
        assert untrusted.returncode!=0, 'token client accepted an untrusted server'
        # A certificate-less verify-full connection succeeds, but neither
        # plaintext nor a mismatched server identity is admitted.
        for name,mode,host,expected in [
                ('tls-scram','verify-full','localhost',0),
                ('plaintext','disable','localhost',2),
                ('hostname-mismatch','verify-full','wrong.invalid',2)]:
            check='''export PGPASSWORD="$(cat /pki/lite-admin-password)"
unset PGSSLCERT PGSSLKEY
export PGSSLROOTCERT=/pki/lite-ca-cert
exec psql -X -qAt "host=%s hostaddr=127.0.0.1 user=postgres dbname=postgres sslmode=%s" -c 'SELECT 1'
'''%(host,mode)
            checked=subprocess.run(['docker',*argv,'exec','-T','control-db','sh','-c',check],
                                   capture_output=True,timeout=20)
            (out/(name+'.log')).write_bytes(checked.stdout+checked.stderr)
            assert checked.returncode==expected, name
        logs=lifecycle.docker(argv+['logs','--no-color','control-db'],env)
        assert token not in logs and password.encode() not in logs
        (out/'database.log').write_bytes(logs)
        version=lifecycle.docker(argv+['exec','-T','control-db','psql','-X','-qAt',
                                      '-U','postgres','-c','SHOW server_version'],env).decode().strip()
        result={'status':'passed','positive_mtls_connection':'passed',
                'engine':'PostgreSQL','version':version,
                'issue_token_install_exit':issue.returncode,
                'issue_token_exit':issued.returncode,
                'server_identity_verified':True,'plaintext_refused':True,
                'untrusted_ca_refused':True,
                'client_certificate_required':False,'token_issued':True,
                'health_meta_role_enforcement_tested':False}
        if args.gateway_boundary:
            result['gateway_boundary']=gateway_boundary(out,pki,grant)
            result['health_meta_role_enforcement_tested']=True
        (out/'result.json').write_text(json.dumps(result,indent=2))
    finally:
        for cid in lifecycle.docker(argv+['ps','-aq'],env).decode().split():
            labels=json.loads(lifecycle.docker(['inspect','--format','{{json .Config.Labels}}',cid],env))
            assert labels.get('oo.lane')=='a' and labels.get('com.docker.compose.project')==project
        lifecycle.docker(argv+['down'],env)
        shutil.rmtree(pki)

def gateway_boundary(out,pki,grant):
    """Exercise the real pre-publication API; never substitute a fake Pack."""
    lifecycle.write(pki/'endpoint.json',(out/'endpoint.json').read_bytes())
    (pki/'packs').mkdir(mode=0o700)
    config={'listen':'127.0.0.1:0','audience':grant['audience_url'],
            'runtime_user':'oo_token_runtime','password_file':'lite-admin-password',
            'ca_file':'lite-ca-cert','endpoint_file':'endpoint.json','pack_dir':'packs',
            'body_limit':65536,'request_timeout_ms':10000,'global_concurrency':2,
            'transport':'tls','tls_key_file':'lite-control-db-key',
            'tls_chain_file':'lite-control-db-cert','client_ca_file':'lite-ca-cert',
            'handshake_timeout_ms':3000}
    lifecycle.write(pki/'config.json',json.dumps(config).encode())
    lifecycle.write(out/'health-client.json',json.dumps({
        'token':'operator-token','ca':'lite-ca-cert','cert':'lite-admin-cert',
        'key':'lite-admin-key'}).encode())
    with (out/'gateway.log').open('wb') as log:
        process=subprocess.Popen([str(ROOT/'target/debug/oo-gateway'),str(pki)],
                                 stdout=log,stderr=log)
        try:
            deadline=time.monotonic()+15
            while not (pki/'listen.addr').exists():
                assert process.poll() is None, 'Gateway failed; see gateway.log'
                assert time.monotonic()<deadline, 'Gateway startup timed out'
                time.sleep(.05)
            port=int((pki/'listen.addr').read_text().strip().rsplit(':',1)[1])
            health=lifecycle.health(out,{'gateway':{'public_url':f'https://localhost:{port}'}})
            assert health['status']=='measured' and health['nearest_credential_expiry_ms']
            assert health['token_expiry']['expires_at_ms']
            assert health['internal_mtls_certificate_expiry']['expires_at_ms']
            lifecycle.write(out/'health.json',json.dumps(health).encode())
            context=lifecycle.ssl.create_default_context(cafile=pki/'lite-ca-cert')
            context.load_cert_chain(pki/'lite-admin-cert',pki/'lite-admin-key')
            token=lifecycle.read_private(pki/'operator-token').decode().strip()
            statuses={}
            for route in ('/v1/health','/v1/meta','/v1/session/current','/v1/tables'):
                conn=lifecycle.http.client.HTTPSConnection('localhost',port,context=context,timeout=10)
                try:
                    conn.request('GET',route,headers={'Authorization':'Bearer '+token})
                    response=conn.getresponse()
                    body=response.read(65537)
                    assert token.encode() not in body
                    statuses[route]=response.status
                    lifecycle.write(out/(route.rsplit('/',1)[1]+'-response.json'),body)
                finally:
                    conn.close()
            assert statuses['/v1/health']==200
            assert statuses['/v1/session/current']==200
            assert statuses['/v1/meta']!=200, 'operator meta contract changed; recheck integration'
            assert statuses['/v1/tables']!=200
            return {'status':'blocked','http_statuses':statuses,
                    'reason':'authenticated meta requires a publication; role is not health/meta-only',
                    'gateway_internal_mtls':True,'fake_publication_created':False}
        finally:
            process.terminate()
            try:process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()

if __name__=='__main__':
    main()
