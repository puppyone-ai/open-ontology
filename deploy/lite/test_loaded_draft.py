#!/usr/bin/env python3
"""One bounded live draft through oo build, with at most two owned databases."""
import argparse
import json
import os
from pathlib import Path
import re
import secrets
import subprocess
import sys
import time

ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'crates/oo-cli/src/admin'))
from test_live_draft import Fixture as PostgresFixture
from test_mysql_doctor import Fixture as MysqlFixture
from test_oracle_doctor import Fixture as OracleFixture
from draft import read_private, write_private
from runner import checksums

PG='docker.io/library/postgres@sha256:9b18b78397054fce88a9552e9d5a3ad5bb7fd258c5b3cc1c5028e46373d6ea8f'

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--run-id',required=True)
    parser.add_argument('--engine',choices=['postgres','mysql','oracle'],required=True)
    parser.add_argument('--image',required=True)
    parser.add_argument('--live',action='store_true',required=True)
    args=parser.parse_args()
    if not re.fullmatch('[a-z0-9-]+',args.run_id) or not re.fullmatch('sha256:[0-9a-f]{64}',args.image):
        parser.error('invalid input')
    os.umask(0o077)
    out=ROOT/'evidence'/args.run_id
    out.mkdir(mode=0o700)
    versions={'postgres':'17','mysql':'8.4','oracle':'23.5'}
    classes={'postgres':PostgresFixture,'mysql':MysqlFixture,'oracle':OracleFixture}
    fixture=classes[args.engine](out,args.engine,versions[args.engine],
        argparse.Namespace(platform=None,scale=1000,tables=3,health_timeout=900))
    fixture.name='oo-a-'+fixture.id
    fixture.volume=fixture.name+'-data'
    private=fixture.private
    target_name='oo-a-target-'+fixture.id
    runtime_name='oo-a-build-'+fixture.id
    created=[]
    sensitive=[fixture.secret.encode(),fixture.reader_secret.encode()]

    def run(argv,name,input=None,timeout=180):
        result=subprocess.run(argv,input=input,capture_output=True,timeout=timeout)
        raw=result.stdout+result.stderr
        for value in sensitive:
            raw=raw.replace(value,b'[REDACTED]')
        (out/(name+'.log')).write_bytes(raw)
        if result.returncode:
            if name=='build':
                # One bounded diagnostic preflight; no generation retry.
                diagnostic=argv[:argv.index('build')]+['doctor','--config','/work/stack.json','--json']
                probe=subprocess.run(diagnostic,capture_output=True,timeout=180)
                data=probe.stdout+probe.stderr
                for value in sensitive:
                    data=data.replace(value,b'[REDACTED]')
                (out/'failed-build-doctor.log').write_bytes(data)
            raise RuntimeError(name+' failed')
        return result.stdout

    result={'status':'failed','engine':args.engine,'version_requested':versions[args.engine],
            'publication':'not_published','review':'unreviewed'}
    try:
        # A separate admin fixture connection seeds data; product reads as fixture_reader.
        for argv,name in [
            (['req','-x509','-newkey','rsa:2048','-nodes','-keyout',str(private/'ca.key'),
              '-out',str(private/'ca.crt'),'-days','2','-subj','/CN=OO fixture CA'],'ca'),
            (['req','-newkey','rsa:2048','-nodes','-keyout',str(private/'server.key'),
              '-out',str(private/'server.csr'),'-subj','/CN=localhost'],'csr')]:
            run(['openssl',*argv],name)
        (private/'ext').write_text('basicConstraints=critical,CA:FALSE\n'
            'subjectAltName=DNS:localhost,IP:127.0.0.1\nextendedKeyUsage=serverAuth\n')
        run(['openssl','x509','-req','-in',str(private/'server.csr'),'-CA',str(private/'ca.crt'),
             '-CAkey',str(private/'ca.key'),'-CAcreateserial','-out',str(private/'server.crt'),
             '-days','2','-extfile',str(private/'ext')],'sign')
        fixture.setup()
        fixture.seed()
        fixture.verify_shapes()
        if args.engine=='postgres':
            # The connector validates clean session startup, then opens its own
            # read-only transactions. SELECT-only grants remain unchanged.
            fixture.sql("ALTER ROLE fixture_reader PASSWORD '"+fixture.reader_secret+"'; "
                        "ALTER ROLE fixture_reader RESET default_transaction_read_only; "
                        "GRANT pg_read_all_stats TO fixture_reader; ANALYZE;",admin=True)
        elif args.engine=='mysql':
            fixture.tls()
            fixture.sql("CREATE USER 'fixture_reader'@'%' IDENTIFIED BY '"+fixture.reader_secret+"' REQUIRE SSL; "
                        "GRANT SELECT, SHOW VIEW ON fixture.* TO 'fixture_reader'@'%';",admin=True)
        else:
            fixture.sql('GRANT FLASHBACK ON fixture.regions TO fixture_reader;\n'
                        'GRANT SELECT ON SYS.V_$DATABASE TO fixture_reader;',admin=True)
        home=private/'home'
        store=home/'.config/open-ontology/secrets'
        store.mkdir(parents=True,mode=0o700)
        upstream=Path.home()/'.config/open-ontology/secrets'
        key=read_private(upstream/'model-key-9r-sol')
        endpoint=read_private(upstream/'model-endpoint-9r')
        sensitive.extend([key.strip(),endpoint.strip()])
        target_password=secrets.token_hex(32).encode()
        sensitive.append(target_password)
        values={'reader':fixture.reader_secret.encode(),'model-key':key,'model-endpoint':endpoint,
                'target-password':target_password,'target-ca':(private/'ca.crt').read_bytes()}
        if args.engine!='oracle':
            values['source-ca']=(private/('ca.pem' if args.engine=='mysql' else 'ca.crt')).read_bytes()
        for name,value in values.items():
            write_private(store/name,value)
        write_private(private/'agent-password',target_password)
        # The target and CLI share the fixture's namespace, not any existing service.
        run(['docker','create','--name',target_name,'--label','oo.lane=a',
             '--label','oo.run_id='+fixture.id,'--network','container:'+fixture.name,
             '--memory','512m','--cpus','1','--tmpfs','/var/lib/postgresql/data',
             '-v',str(private)+':/run/fixture:ro','-e','POSTGRES_PASSWORD_FILE=/tmp/agent-password',
             '--entrypoint','bash',PG,'-c',
             'cp /run/fixture/server.* /tmp/; cp /run/fixture/agent-password /tmp/; '
             'chown postgres:postgres /tmp/server.* /tmp/agent-password; '
             'chmod 600 /tmp/server.key /tmp/agent-password; '
             'exec docker-entrypoint.sh postgres -p 55432 -c ssl=on '
             '-c ssl_cert_file=/tmp/server.crt -c ssl_key_file=/tmp/server.key'],'target-create')
        created.append(target_name)
        run(['docker','start',target_name],'target-start')
        for _ in range(90):
            ready=subprocess.run(['docker','exec',target_name,'pg_isready','-p','55432','-U','postgres'],
                                 capture_output=True,timeout=10)
            if ready.returncode==0:
                break
            time.sleep(1)
        else:
            raise RuntimeError('target readiness timeout')
        source={'id':'fixture','engine':args.engine,'host':'localhost',
                'port':{'postgres':5432,'mysql':3306,'oracle':1521}[args.engine],
                'database':'FREEPDB1' if args.engine=='oracle' else 'fixture','user':'fixture_reader',
                'password':{'secret_ref':'reader'},
                'scope':{'include':[{'postgres':'public.regions','mysql':'fixture.regions',
                                    'oracle':'FIXTURE.REGIONS'}[args.engine]]}}
        if args.engine!='oracle':
            source['tls_ca']={'secret_ref':'source-ca'}
        config={'version':'oo.stack/v1','sources':[source],
                'model':{'provider':'openai-compatible','base_url':{'secret_ref':'model-endpoint'},
                         'model':'gpt-6.1-sol','key':{'secret_ref':'model-key'}}}
        write_private(out/'stack.json',json.dumps(config).encode())
        target={'host':'localhost','port':55432,'database':'postgres','user':'postgres',
                'password':{'secret_ref':'target-password'},'tls_ca':{'secret_ref':'target-ca'},
                'run_id':args.run_id}
        write_private(out/'target.json',json.dumps(target).encode())
        created.append(runtime_name)
        raw=run(['docker','run','--rm','--name',runtime_name,'--label','oo.lane=a',
                 '--label','oo.run_id='+fixture.id,'--network','container:'+fixture.name,
                 '--user',str(os.getuid())+':'+str(os.getgid()),'-e','HOME=/private/home',
                 '-v',str(out)+':/work','-v',str(private)+':/private:ro',args.image,
                 'build','--config','/work/stack.json','--agent-target','/work/target.json','--json'],
                'build',timeout=900)
        report=json.loads(raw)
        assert report['status']=='draft' and report['agent_db_load']=='local_table_views'
        assert report['publication']=='not_published' and report['review']=='unreviewed'
        receipts=[json.loads(line) for line in (out/'.oo/load-receipts.jsonl').read_text().splitlines()]
        assert len(receipts)==1 and receipts[0]['gateway_published'] is False
        catalog=json.loads((out/'.oo/catalog.json').read_text())
        assert catalog['tables'] and all(t['schema']=='oo_load_data' for t in catalog['tables'])
        result.update(status='passed',engine_version=receipts[0]['version'],
                      agent_db_load='local_table_views',consistency=report['consistency'],
                      source_time=report['source_time'],max_generation_calls=2,
                      max_completion_tokens_per_call=8192,source_to_draft_command='oo build --agent-target')
    except Exception as error:
        # Exception text can include subprocess argv; retain only its type.
        result['error']=type(error).__name__
        raise
    finally:
        failures=[]
        for name in reversed(created):
            found=subprocess.run(['docker','inspect','--format','{{json .Config.Labels}}',name],capture_output=True)
            if found.returncode==0:
                labels=json.loads(found.stdout)
                assert labels.get('oo.lane')=='a' and labels.get('oo.run_id')==fixture.id
                if subprocess.run(['docker','rm','-f',name],capture_output=True).returncode:
                    failures.append(name)
        failures+=fixture.cleanup()
        result['cleanup_failures']=failures
        (out/'result.json').write_text(json.dumps(result,indent=2)+'\n')
        checksums(out)

if __name__=='__main__':
    main()
