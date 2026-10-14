#!/usr/bin/env python3
"""Fresh owned-fixture connection capture; does not claim downstream acceptance."""
import argparse, hashlib, json, os, pathlib, subprocess, uuid

def sha(p): return hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest()
def save(p, x):
    pathlib.Path(p).write_text(json.dumps(x, indent=2)+'\n')
    os.chmod(p, 0o600)
def call(argv, stem):
    with open(str(stem)+'.stdout','wb') as out, open(str(stem)+'.stderr','wb') as err:
        r=subprocess.run(argv,stdout=out,stderr=err,timeout=240)
    pathlib.Path(str(stem)+'.exit').write_text(str(r.returncode)+'\n')
    if r.returncode: raise RuntimeError('stage refused: '+str(stem))
def main():
    p=argparse.ArgumentParser();p.add_argument('--config',required=True);p.add_argument('--run',required=True);a=p.parse_args()
    c=json.loads(pathlib.Path(a.config).read_text());root=pathlib.Path(a.run)
    assert c['owned_fixture'] is True
    for binary in ('driver','admission'):
        assert sha(c[binary]['path'])==c[binary]['sha256']
    assert pathlib.Path(c['reader_user_file']).read_text().strip()==c['user']
    assert pathlib.Path(c['password_file']).read_bytes()==pathlib.Path(c['fixture_password_file']).read_bytes()
    image=subprocess.check_output([c['docker'],'image','inspect',c['image'],'--format','{{.Id}}'],text=True).strip()
    assert image==c['image']
    # The client reads the exact private fixture credential; connection fields override defaults.
    call([c['docker'],'exec',c['fixture_container'],'mariadb','--defaults-extra-file=/run/secrets/reader.cnf','--host='+c['host'],'--port='+str(c['port']),'--user='+c['user'],'--database='+c['database'],'--batch','--skip-column-names','-e','SELECT DATABASE(), CURRENT_USER(), VERSION(); SELECT PRIVILEGE_TYPE, TABLE_SCHEMA FROM information_schema.SCHEMA_PRIVILEGES;'],root/'connection-preflight')
    rows=[]
    for cycle in (1,2):
        for item in c['objects']:
            name=item['schema']+'__'+item['table'];out=root/f'cycle-{cycle}'/name;out.mkdir(parents=True)
            req=dict(schema=item['schema'],table=item['table'],maria_host=c['host'],maria_port=c['port'],maria_user_file=c['reader_user_file'],maria_password_file=c['password_file'],out_dir=str(out/'capture'),driver_path=c['driver']['path'],source_id=name,run_id=str(uuid.uuid4())+'_'+str(cycle),label='fresh-connection',secret_ref=c['secret_ref'],source_generation=cycle)
            save(out/'request.json',req)
            base=[c['docker'],'run','--rm','--network',c['network'],'--mount','type=bind,src='+str(root)+',dst='+str(root)]
            for src in (c['reader_user_file'],c['password_file'],c['driver']['path'],c['admission']['path']):
                base+=['--mount','type=bind,src='+src+',dst='+src+',readonly']
            call(base+[c['image'],c['driver']['path'],'product-witness',str(out/'request.json')],out/'capture-call')
            call(base+[c['image'],c['admission']['path'],'seal-native','--report',str(out/'capture'/'report.json'),'--config',str(out/'capture'/'config.json'),'--out',str(out/'admission')],out/'seal-call')
            rows.append({'cycle':cycle,'object':name,'report_sha256':sha(out/'capture'/'report.json'),'admission_sha256':sha(out/'admission'/'admission-receipt.json')})
    save(root/'capture-manifest.json',{'protocol':'oo.mariadb.connection-capture/v1','fresh_capture':True,'cycles':rows,'downstream_complete':False,'missing':['Loader/CAS/ACK','fixture insert/update/delete window','fresh S0-S10','maintained Ontology/Pack','current Gateway/CLI/dialogue']})
if __name__=='__main__': main()
