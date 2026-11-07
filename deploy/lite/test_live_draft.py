#!/usr/bin/env python3
"""Opt-in bounded live model on one table in an owned PostgreSQL fixture."""
import argparse
import json
import os
from pathlib import Path
import sys
import subprocess

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'tools/oo-test'))
sys.path.insert(0,str(ROOT/'crates/oo-cli/src/admin'))
from runner import Run, checksums
from test_oracle_doctor import Fixture as Cleanup
from test_doctor import command, private_file, SECRET_VALUES

class Fixture(Run):
    cleanup = Cleanup.cleanup
    def command(self, argv, **kwargs):
        argv = ['oo.lane=a' if s=='oo.lane=h' else s for s in argv]
        if argv[:2] == ['docker','create']:
            image = next(i for i, value in enumerate(argv) if value.startswith('docker.io/library/postgres:'))
            server_args = argv[image+1:]
            argv = argv[:image+1]
            argv[2:2] = ['-p','127.0.0.1::5432','--entrypoint','bash']
            argv += ['-c', 'cp /run/oo/server.* /tmp/; chown postgres:postgres /tmp/server.*; '
                     'chmod 600 /tmp/server.key; exec docker-entrypoint.sh postgres -c ssl=on '
                     '-c ssl_cert_file=/tmp/server.crt -c ssl_key_file=/tmp/server.key "$@"',
                     'oo-fixture', *server_args]
        return super().command(argv,**kwargs)

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--run-id',required=True)
    parser.add_argument('--live',action='store_true',required=True)
    parser.add_argument('--image',help='owned repository runtime content ID; isolates loopback egress')
    a=parser.parse_args()
    if not a.run_id or any(c not in 'abcdefghijklmnopqrstuvwxyz0123456789-' for c in a.run_id):
        parser.error('invalid run id')
    out=ROOT/'evidence'/a.run_id
    out.mkdir()
    f=Fixture(out,'postgres','17',argparse.Namespace(platform=None,scale=1000,tables=3,health_timeout=180))
    f.name='oo-a-'+f.id
    f.volume=f.name+'-data'
    p=f.private
    key=b''
    runtime=None
    try:
        command(['openssl','req','-x509','-newkey','rsa:2048','-nodes','-keyout',str(p/'ca.key'),
                 '-out',str(p/'ca.crt'),'-days','2','-subj','/CN=OO fixture CA'],out/'ca.log')
        command(['openssl','req','-newkey','rsa:2048','-nodes','-keyout',str(p/'server.key'),
                 '-out',str(p/'server.csr'),'-subj','/CN=localhost'],out/'csr.log')
        (p/'ext').write_text('basicConstraints=critical,CA:FALSE\nsubjectAltName=DNS:localhost,IP:127.0.0.1\nextendedKeyUsage=serverAuth\n')
        command(['openssl','x509','-req','-in',str(p/'server.csr'),'-CA',str(p/'ca.crt'),
                 '-CAkey',str(p/'ca.key'),'-CAcreateserial','-out',str(p/'server.crt'),'-days','2',
                 '-extfile',str(p/'ext')],out/'sign.log')
        f.setup();f.seed();f.verify_shapes();f.gold()
        f.sql("ALTER ROLE fixture_reader PASSWORD '"+f.reader_secret+"'; ALTER ROLE fixture_reader RESET default_transaction_read_only; GRANT pg_read_all_stats TO fixture_reader; ANALYZE;",admin=True)
        home=p/'home';store=home/'.config/open-ontology/secrets'
        store.mkdir(parents=True,mode=0o700)
        # The card explicitly authorizes this one model credential. Never print it.
        from draft import read_private
        key=read_private(Path.home()/'.config/open-ontology/secrets/model-key-9r-sol')
        SECRET_VALUES.extend([key.decode().strip(),f.secret,f.reader_secret])
        for name,value in [('reader',f.reader_secret),('ca',(p/'ca.crt').read_text()),('model-key',key.decode())]:
            private_file(store/name,value)
        _,address=f.command(['docker','port',f.name,'5432/tcp'])
        config={'version':'oo.stack/v1','sources':[{'id':'fixture','engine':'postgres','host':'localhost',
                'port':int(address.strip().rsplit(':',1)[1]),'database':'fixture','user':'fixture_reader',
                'password':{'secret_ref':'reader'},'tls_ca':{'secret_ref':'ca'},'scope':{'include':['public.regions']}}],
                'model':{'provider':'openai-compatible','base_url':read_private(Path.home()/'.config/open-ontology/secrets/model-endpoint-9r').decode().strip(),
                         'model':'gpt-6.1-sol','key':{'secret_ref':'model-key'}}}
        path=out/'stack.json';private_file(path,json.dumps(config))
        argv=[str(ROOT/'target/debug/oo'),'build','--draft-only','--config',str(path),'--json']
        if a.image:
            import re
            if not re.fullmatch(r'sha256:[0-9a-f]{64}',a.image):
                raise ValueError('runtime content digest required')
            config['sources'][0]['port']=5432
            path.write_text(json.dumps(config))
            runtime='oo-a-draft-'+f.id
            argv=['docker','run','--rm','--name',runtime,'--label','oo.lane=a',
                  '--label','oo.run_id='+f.id,'--network','container:'+f.name,
                  '--user',str(os.getuid())+':'+str(os.getgid()),'-e','HOME=/private/home',
                  '--mount','type=bind,src='+str(out)+',dst=/work',
                  '--mount','type=bind,src='+str(p)+',dst=/private,readonly',
                  a.image,'build','--draft-only','--config','/work/stack.json','--json']
        proc=subprocess.run(argv,env=dict(os.environ,HOME=str(home)),capture_output=True,timeout=700)
        raw=proc.stdout+proc.stderr
        assert key.strip() not in raw and f.reader_secret.encode() not in raw
        (out/'build.json').write_bytes(raw);(out/'build.exit').write_text(str(proc.returncode))
        assert proc.returncode==0, 'build did not complete; see stage receipts'
        report=json.loads(proc.stdout)
        assert report['status']=='draft' and report['publication']=='not_published'
        assert report['review']=='unreviewed' and report['agent_db_load']=='pending_oo_load'
        (out/'result.json').write_text(json.dumps({'result':'passed','engine':'PostgreSQL 17',
             'scope':'one-table read-only metadata-to-draft; NOT full Agent DB load or lite stack acceptance',
             'model':'gpt-6.1-sol','max_generation_calls':2,'max_completion_tokens_per_call':8192},indent=2))
        return 0
    finally:
        if runtime:
            checked=subprocess.run(['docker','inspect','--format','{{json .Config.Labels}}',runtime],capture_output=True)
            if checked.returncode==0:
                labels=json.loads(checked.stdout)
                assert labels.get('oo.lane')=='a' and labels.get('oo.run_id')==f.id
                subprocess.run(['docker','rm','-f',runtime],capture_output=True,check=True)
        failures=f.cleanup()
        (out/'cleanup.json').write_text(json.dumps({'failures':failures}))
        checksums(out)

if __name__=='__main__':
    raise SystemExit(main())
