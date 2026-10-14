#!/usr/bin/env python3
"""Capture a newly supplied private MySQL connection; never reuse baseline bytes."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import uuid


def main():
    p = argparse.ArgumentParser()
    p.add_argument('--config', required=True)
    p.add_argument('--run', required=True)
    p.add_argument('--secret-ref', required=True)
    a = p.parse_args()
    root = Path(a.run).resolve()
    c = json.loads(Path(a.config).read_text())
    c['owner'] = str(uuid.uuid4())
    c['generation'] = 1
    for table in c['tables']:
        table['schema'] = c['mysql']['database']
    config = root/'capture-config.json'
    config.write_text(json.dumps(c, indent=2)+'\n'); os.chmod(config, 0o600)
    driver = '/tmp/oo-dl01w1-06-native-20261012/bin/time-official/mysql-preview'
    admission = '/tmp/oo-dl01w1-06-native-20261012/bin/time-official/oo-source-admission'
    image = 'docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5'
    tools = Path(__file__).resolve().parent
    password_parent = Path(c["mysql"]["password_file"]).resolve().parent
    common = ['docker','run','--rm','--network','oo-dl01w1-06-mysql-net','--cpus','0.5','--memory','512m',
              '-v',f'{root}:{root}', '-v',f'{password_parent}:{password_parent}:ro', '-v', '/tmp/oo-dl01w1-06-native-20261012:/tmp/oo-dl01w1-06-native-20261012:ro',
              '-v','/tmp/oo-dl01w1-06/scale-secrets:/secrets:ro', '-v',f'{tools}:/mysql-tools:ro',image]
    def call(name, argv):
        with (root/(name+'.stdout')).open('xb') as out, (root/(name+'.stderr')).open('xb') as err:
            r = subprocess.run(common+argv,stdout=out,stderr=err,timeout=360)
        (root/(name+'.exit')).write_text(str(r.returncode)+'\n')
        if r.returncode: raise SystemExit('MySQL '+name+' refused; private logs retained')
    call('capture',[driver,'capture-native','--config',str(config),'--out',str(root/'capture')])
    call('prepare',['/usr/bin/python3','/mysql-tools/prepare-native.py','--capture',str(root/'capture'),
                    '--out',str(root/'inputs'),'--secret-ref',a.secret_ref])
    receipts=[]
    for table in c['tables']:
        name=table['table']; dest=root/'sealed'/name; dest.parent.mkdir(exist_ok=True)
        call('seal-'+name,[admission,'seal-native','--report',str(root/'inputs'/name/'report.json'),
                          '--config',str(root/'inputs'/name/'config.json'),'--out',str(dest)])
        registry=dest/'registry.json'
        receipts.append({'table':name,'path':str(registry),'sha256':hashlib.sha256(registry.read_bytes()).hexdigest()})
    (root/'capture-receipt.json').write_text(json.dumps({'protocol':'oo.mysql.connection-capture/v1',
        'fresh_source_capture':True,'state':'admitted','registries':receipts,
        'full_product_chain_complete':False},indent=2)+'\n')

if __name__ == '__main__': main()
