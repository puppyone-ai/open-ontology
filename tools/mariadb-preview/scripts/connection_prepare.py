#!/usr/bin/env python3
"""Provision an isolated MariaDB target using official installers only."""
import argparse, hashlib, json, os, pathlib, secrets, shutil, subprocess, sys, time

def sha(path): return hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest()
def save(path, value):
    pathlib.Path(path).write_text(json.dumps(value,indent=2)+'\n');os.chmod(path,0o600)
def main():
    parser=argparse.ArgumentParser();parser.add_argument('--config',required=True);parser.add_argument('--run',required=True);a=parser.parse_args()
    c=json.loads(pathlib.Path(a.config).read_text());root=pathlib.Path(a.run)
    assert c['owned_fixture'] is True
    assert shutil.disk_usage(root).free >= 1024**3, 'host disk gate'
    for pin in c['prepare_pins']: assert sha(pin['path'])==pin['sha256'], 'program drift'
    private=root/'private';private.mkdir(mode=0o700)
    for name in ['pg-admin-password','runtime-password']:
        p=private/name;p.write_text(secrets.token_hex(32)+'\n');p.chmod(0o600)
    count=0
    def call(argv, name, stdin=None, timeout=900):
        nonlocal count;count+=1;stem=root/f'{count:03d}-{name}'
        with pathlib.Path(str(stem)+'.stdout').open('wb') as out,pathlib.Path(str(stem)+'.stderr').open('wb') as err:
            result=subprocess.run(argv,input=stdin,stdout=out,stderr=err,timeout=timeout)
        pathlib.Path(str(stem)+'.exit').write_text(str(result.returncode)+'\n')
        assert result.returncode==0, 'official preparation refused: '+name
    tag=hashlib.sha256(str(root).encode()).hexdigest()[:10]
    network='oo-maria-connection-'+tag;pg=network+'-pg';runtime=network+'-runtime';volume=network+'-data'
    cert=private/'server.ext';cert.write_text('subjectAltName=DNS:'+pg+'\nextendedKeyUsage=serverAuth\n')
    call([c['openssl'],'req','-x509','-newkey','rsa:2048','-nodes','-keyout',str(private/'ca.key'),'-out',str(private/'ca.crt'),'-days','30','-subj','/CN=Maria connection owned run'], 'tls-ca')
    call([c['openssl'],'req','-newkey','rsa:2048','-nodes','-keyout',str(private/'server.key'),'-out',str(private/'server.csr'),'-subj','/CN='+pg], 'tls-server')
    call([c['openssl'],'x509','-req','-in',str(private/'server.csr'),'-CA',str(private/'ca.crt'),'-CAkey',str(private/'ca.key'),'-CAcreateserial','-out',str(private/'server.crt'),'-days','30','-extfile',str(cert)], 'tls-sign')
    for p in private.iterdir():p.chmod(0o600)
    # Keys are copied into container-private storage before the official entrypoint drops privileges.
    entry=root/'pg-entry.sh';entry.write_text('#!/bin/sh\nset -eu\nmkdir -p /tls\ncp /bootstrap/server.crt /tls/server.crt\ncp /bootstrap/server.key /tls/server.key\nchown postgres:postgres /tls/server.crt /tls/server.key\nchmod 600 /tls/server.key\nexec /usr/local/bin/docker-entrypoint.sh postgres -c ssl=on -c ssl_cert_file=/tls/server.crt -c ssl_key_file=/tls/server.key\n');entry.chmod(0o700)
    docker=c['docker']
    # Default Docker address pools are exhausted. Join the owned fixture network instead of allocating another subnet.
    call([docker,'run','-d','--name',pg,'--network',c['source_network'],'-p','127.0.0.1:'+str(c['api_port'])+':18454','--mount','type=volume,src='+volume+',dst=/var/lib/postgresql/data','--mount','type=bind,src='+str(private)+',dst=/bootstrap,readonly','--mount','type=bind,src='+str(entry)+',dst=/entry.sh,readonly','-e','POSTGRES_PASSWORD_FILE=/bootstrap/pg-admin-password','--entrypoint','/entry.sh',c['pg_image']], 'pg-start')
    call([docker,'network','connect',c['identity_network'],pg],'network-connect')
    for attempt in range(30):
        ready=subprocess.run([docker,'exec',pg,'pg_isready','-U','postgres'],capture_output=True,timeout=10)
        if ready.returncode==0:break
        time.sleep(1)
    assert ready.returncode==0, 'postgres readiness'
    argv=[docker,'run','-d','--name',runtime,'--network','container:'+pg,'--mount','type=bind,src='+str(root)+',dst='+str(root),'--mount','type=bind,src='+c['repo']+',dst='+c['repo']+',readonly']
    paths={p['path'] for p in c['prepare_pins'] if p['path']!=c['openssl']}|{c['reader_user_file'],c['password_file']}
    for path in sorted(paths):argv+=['--mount','type=bind,src='+path+',dst='+path+',readonly']
    call(argv+[c['runtime_image'],'sleep','86400'],'runtime-start')
    conf={'host':pg,'port':5432,'database':'oo_a390_control','runtime_user':'oo_a390_runtime_login','runtime_password_file':str(private/'runtime-password'),'admin_user':'postgres','admin_password_file':str(private/'pg-admin-password'),'ca_file':str(private/'ca.crt'),'migrations':c['repo']+'/crates/oo-jobs/migrations'}
    config=root/'worker.conf';config.write_text(''.join(str(k)+'='+str(v)+'\n' for k,v in conf.items()));config.chmod(0o600)
    worker=[docker,'exec',runtime,c['worker']['path'],'--config',str(config)]
    call(worker+['apply'],'official-worker-apply')
    call(worker+['native-install'],'official-native-install')
    # Worker apply stops at jobs lease operations. The loader locks through jobs.loader_hold.
    call([sys.executable,str(pathlib.Path(c['repo'])/'crates/oo-jobs/tools/deploy.py'),'--container',pg,'--database','oo_a390_control','--admin','postgres','--loader-role','oo_a390_runtime_login'],'official-jobs-deploy')
    call([docker,'exec',pg,'psql','-U','postgres','-d','oo_a390_control','-v','ON_ERROR_STOP=1','-c','GRANT CREATE ON DATABASE oo_a390_control TO oo_a390_runtime_login'], 'target-owner-bootstrap')
    target={'host':pg,'port':5432,'database':'oo_a390_control','runtime_principal':'oo_a390_runtime_login','password_file':str(private/'runtime-password'),'ssl':'verify-full','ca_file':str(private/'ca.crt'),'owner':c['owner'],'shard':'maria_connection'}
    for binary,command in [(c['classic_bootstrap'],'install-native-update'),(c['loader'],'install-sealed-catalog'),(c['loader'],'install-native-sealed-catalog'),(c['loader'],'install-native-update'),(c['loader'],'install-native-claim-selected')]:
        if command in ('install-sealed-catalog','install-native-sealed-catalog'):
            install={'protocol':'oo.pipeline.'+command+'/v1','target':{'host':pg,'port':5432,'database':'oo_a390_control','administrator':'postgres','password_file':str(private/'pg-admin-password'),'ca_file':str(private/'ca.crt')}}
        else:
            administrator=dict(target,runtime_principal='postgres',password_file=str(private/'pg-admin-password'))
            install={'protocol':'oo.pipeline.install-native-update/v1','target':administrator}
        save(root/(command+'.request.json'),install)
        call([docker,'exec','-i',runtime,binary['path'],command],'official-'+command,json.dumps(install).encode())
    call([docker,'exec',pg,'psql','-U','postgres','-d','oo_a390_control','-v','ON_ERROR_STOP=1','-c','GRANT oo_pipeline_runtime TO oo_a390_runtime_login WITH INHERIT TRUE, SET FALSE'], 'target-pipeline-inherit')
    call([docker,'exec',pg,'psql','-U','postgres','-d','oo_a390_control','-v','ON_ERROR_STOP=1','-c','GRANT USAGE,CREATE ON SCHEMA _oo TO oo_a390_runtime_login; GRANT SELECT,INSERT,UPDATE,DELETE ON ALL TABLES IN SCHEMA _oo TO oo_a390_runtime_login; GRANT USAGE,SELECT ON ALL SEQUENCES IN SCHEMA _oo TO oo_a390_runtime_login'], 'target-loader-acl')
    call([docker,'exec',pg,'psql','-U','postgres','-d','oo_a390_control','-At','-c',"SELECT version,checksum,phase,filename FROM jobs.schema_migrations ORDER BY version; SELECT rolname,rolsuper,rolcreatedb,rolcreaterole,rolbypassrls FROM pg_roles WHERE rolname='oo_a390_runtime_login'; SELECT count(*) FROM jobs.tasks; SELECT count(*) FROM _oo.source_watermark;"], 'independent-installer-readiness')
    save(root/'preparation.json',{'protocol':'oo.mariadb.official-preparation/v1','pg':pg,'runtime':runtime,'network':c['source_network'],'volume':volume,'worker_config':str(config),'target':target,'official_installers':True,'manual_ready_markers':False,'source_rows_copied':0,'complete_chain':False})
if __name__=='__main__':main()
