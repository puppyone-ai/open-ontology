#!/usr/bin/env python3
"""One owned TLS PostgreSQL container at a time. Secrets never enter argv/logs."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import secrets
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[3]
PINS = {13:'13.23',14:'14.24',15:'15.19',16:'16.15',17:'17.11',18:'18.6'}

def run(argv, *, out=None, data=None, env=None):
    p = subprocess.run(argv, input=data, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, env=env)
    if out: out.write_bytes(p.stdout)
    if p.returncode: raise RuntimeError(f'{argv[0]} failed ({p.returncode}); see evidence')
    return p.stdout

def compare_with_lane_h(directory):
    spec = importlib.util.spec_from_file_location('oo_test_comparison', ROOT/'tools/oo-test/comparison.py')
    oracle = importlib.util.module_from_spec(spec)
    previous = sys.dont_write_bytecode
    sys.dont_write_bytecode = True
    try:
        spec.loader.exec_module(oracle)
    finally:
        sys.dont_write_bytecode = previous
    reports = []
    for suite in ['c1', 'c5']:
        checks = json.loads((directory/f'oracle-{suite}.json').read_text())
        if len(checks) != 4:
            raise RuntimeError('incomplete lane-H oracle inputs')
        for check in checks:
            report = oracle.compare(check['source'], check['target'],
                                    key_columns=check['key_columns'], chunk_size=2, consistency='B')
            reports.append(dict(name=check['name'], **report))
    (directory/'H-comparison.json').write_text(json.dumps(reports, indent=2))
    if not all(report['equal'] for report in reports):
        raise RuntimeError('lane-H typed oracle mismatch')

def main():
    ap=argparse.ArgumentParser()
    ap.add_argument('--versions', default='13,14,15,16,17,18')
    ap.add_argument('--run-id',default='d-s0s1-20261104')
    ap.add_argument('--suite', choices=['snapshot', 'changes'], default='snapshot')
    a=ap.parse_args()
    target = 'postgres_changes' if a.suite == 'changes' else 'postgres_engine'
    fixture = 'changes.sql' if a.suite == 'changes' else 'fixture.sql'
    base=ROOT/'evidence'/a.run_id
    base.mkdir(parents=True,exist_ok=True)
    for major in map(int,a.versions.split(',')):
        d=base/f'pg{major}'
        d.mkdir(exist_ok=False)
        inputs = sorted((ROOT/'crates/oo-connectors/src').rglob('*.rs'))
        inputs += [ROOT/f'crates/oo-connectors/tests/{target}.rs', Path(__file__).resolve(), ROOT/'testing/engines/postgres'/fixture,
                   ROOT/'crates/oo-pipeline/src/pg_current_capture.rs', ROOT/'tools/oo-test/comparison.py']
        (d/'source-sha256.json').write_text(json.dumps({str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in inputs}, indent=2))
        secret=d/'private'; secret.mkdir(mode=0o700)
        for name in ['admin','reader']:
            p=secret/name; p.write_text(secrets.token_hex(24)); p.chmod(0o600)
        run(['openssl','req','-x509','-newkey','rsa:2048','-nodes','-keyout',str(secret/'ca.key'),'-out',str(secret/'ca.crt'),'-days','2','-subj','/CN=OO Fixture CA'],out=d/'tls-ca.log')
        run(['openssl','req','-newkey','rsa:2048','-nodes','-keyout',str(secret/'server.key'),'-out',str(secret/'server.csr'),'-subj','/CN=localhost'],out=d/'tls-request.log')
        (secret/'extensions').write_text('basicConstraints=critical,CA:FALSE\nsubjectAltName=DNS:localhost,IP:127.0.0.1\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n')
        run(['openssl','x509','-req','-in',str(secret/'server.csr'),'-CA',str(secret/'ca.crt'),'-CAkey',str(secret/'ca.key'),'-CAcreateserial','-out',str(secret/'server.crt'),'-days','2','-extfile',str(secret/'extensions')],out=d/'tls-sign.log')
        (secret/'server.key').chmod(0o600)
        image=f'postgres:{PINS[major]}-bookworm'
        inspection=json.loads(run(['docker','image','inspect',image]))[0]
        (d/'image.json').write_text(json.dumps({k:inspection[k] for k in ['Id','RepoDigests','Architecture']},indent=2))
        name=f'oo-d-{major}-{secrets.token_hex(4)}'
        created=False
        try:
            # Mount the runtime secret directory; copy/chown inside only this container.
            run(['docker','run','-d','--name',name,'--label','oo.lane=d','--label',f'oo.run_id={a.run_id}',
                 '--memory','1g','--cpus','2','-p','127.0.0.1::5432','--tmpfs',('/var/lib/postgresql:rw' if major >= 18 else '/var/lib/postgresql/data:rw'),
                 '-v',f'{secret}:/run/oo-private:ro','-e','POSTGRES_PASSWORD_FILE=/tmp/admin',
                 '--entrypoint','bash',inspection['Id'],'-c',
                 'cp /run/oo-private/server.* /tmp/; cp /run/oo-private/admin /tmp/admin; chown postgres:postgres /tmp/admin; chmod 600 /tmp/admin; chown postgres:postgres /tmp/server.*; chmod 600 /tmp/server.key; exec docker-entrypoint.sh postgres -c ssl=on -c ssl_cert_file=/tmp/server.crt -c ssl_key_file=/tmp/server.key -c wal_level=logical'])
            created=True
            for _ in range(90):
                p=subprocess.run(['docker','exec',name,'pg_isready','-U','postgres'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
                if p.returncode==0: break
                time.sleep(1)
            else:
                run(['docker','logs',name],out=d/'startup.log')
                (d/'result.json').write_text(json.dumps({'exit':1,'stage':'startup','image':image}))
                raise RuntimeError('server did not start')
            # Secret is on stdin, and setup output deliberately does not echo SQL.
            password=(secret/'reader').read_text()
            setup=f"CREATE ROLE oo_reader LOGIN REPLICATION PASSWORD '{password}';\n".encode()+(ROOT/'testing/engines/postgres'/fixture).read_bytes()
            run(['docker','exec','-i',name,'psql','-U','postgres','-v','ON_ERROR_STOP=1'],data=setup,out=d/'fixture.log')
            port=run(['docker','port',name,'5432/tcp']).decode().strip().rsplit(':',1)[1]
            env=dict(os.environ,OO_PG_TEST_DIR=str(d),OO_PG_TEST_PORT=port,OO_PG_TEST_MAJOR=str(major))
            start=time.monotonic()
            p=subprocess.run(['cargo','test','--config','profile.test.package.oo-connectors.opt-level=2','--config','profile.test.package.sha2.opt-level=3','--config','profile.test.package.oo-core.opt-level=2','-p','oo-connectors','--locked','--test',target,'--','--ignored','--nocapture'],cwd=ROOT,env=env,stdout=(d/'test.log').open('wb'),stderr=subprocess.STDOUT)
            if p.returncode == 0 and a.suite == 'changes':
                try:
                    compare_with_lane_h(d)
                except Exception:
                    (d/'result.json').write_text(json.dumps({'exit':1,'stage':'lane-H-oracle','image':image}))
                    raise
            (d/'result.json').write_text(json.dumps({'exit':p.returncode,'elapsed_seconds':time.monotonic()-start,'image':image}))
            if p.returncode: raise RuntimeError(f'PG {major} tests failed; see {d.name}/test.log')
        finally:
            if created:
                run(['docker','rm','-f','-v',name],out=d/'cleanup.log')
            # Private keys/passwords deliberately excluded from the evidence hash manifest.
            hashes={str(p.relative_to(d)):hashlib.sha256(p.read_bytes()).hexdigest() for p in d.glob('*') if p.is_file()}
            (d/'SHA256SUMS.json').write_text(json.dumps(hashes,indent=2))

if __name__=='__main__': main()
