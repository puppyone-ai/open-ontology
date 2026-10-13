#!/usr/bin/python3
"""MySQL C5 adapter: real source capture/admission; worker alone claims/imports."""
import argparse,fcntl,hashlib,json,os,shutil,subprocess,sys,uuid
from pathlib import Path

def write(p,x):
    Path(p).write_text(json.dumps(x,indent=2)+'\n')
def call(argv,p):
    r=subprocess.run(argv,capture_output=True)
    Path(str(p)+'.stdout').write_bytes(r.stdout);Path(str(p)+'.stderr').write_bytes(r.stderr)
    Path(str(p)+'.exit').write_text(str(r.returncode)+'\n')
    if r.returncode: raise RuntimeError('source stage refused: '+str(p))
def main():
    a=argparse.ArgumentParser();a.add_argument('--config',required=True)
    c=json.loads(Path(a.parse_args().config).read_text());o=json.load(sys.stdin)
    if o['protocol']!='oo.jobs.native-producer/v1' or o['source_id']!='mysql_native' or o['method']!='C5':raise RuntimeError('occurrence mismatch')
    for n in ['driver','admission']:
        if hashlib.sha256(Path(c[n]['path']).read_bytes()).hexdigest()!=c[n]['sha256']:raise RuntimeError(n+' identity changed')
    root=Path(c['state_dir']);root.mkdir(parents=True,exist_ok=True)
    pair=str(uuid.UUID(o['binding']))+'_'+str(int(o['pin']))
    with (root/'producer.lock').open('a') as lock:
        fcntl.flock(lock,fcntl.LOCK_EX)
        out=root/pair;out.mkdir(exist_ok=True);request=out/'native-request.json'
        if request.exists():sys.stdout.write(request.read_text());return
        parent=o.get('acknowledged_source_generation');generation=(parent or 0)+1
        if generation==1:
            registry=Path(c['baseline_registry']);capture=Path(c['baseline_capture'])
        else:
            cfg=json.loads(Path(c['capture_config']).read_text());cfg['generation']=generation;cfg['owner']=str(uuid.uuid4())
            cfg['tables']=[t for t in cfg['tables'] if t['table']==c['table']]
            write(out/'capture-config.json',cfg);capture=out/'capture'
            call([c['driver']['path'],'capture-native','--config',str(out/'capture-config.json'),'--out',str(capture)],out/'capture-call')
            call(['/usr/bin/python3',c['prepare'],'--capture',str(capture),'--out',str(out/'inputs'),'--secret-ref',c['secret_ref']],out/'prepare-call')
            inputs=out/'inputs'/c['table'];sealed=out/'sealed'
            call([c['admission']['path'],'seal-native','--report',str(inputs/'report.json'),'--config',str(inputs/'config.json'),'--out',str(sealed)],out/'seal-call')
            registry=sealed/'registry.json'
        b=json.loads(registry.read_text())['bindings'][0]
        if b['source_generation']!=generation:raise RuntimeError('sealed generation mismatch')
        facts=json.loads((capture/'capturer.json').read_text());t=next(t for t in facts['tables'] if t['name']==c['table'])
        fields=b['fields'];columns=[dict(f,logical_name=f['name']) for f in fields]
        r=dict(protocol='oo.pipeline.native-update/v1',source_engine='mysql',source_id=b['source_id'],source_generation=generation,parent_source_generation=parent,
            logical_schema=t['schema'],logical_table=t['name'],batch_id=pair,run_id=b['run_id'],capture_method='c5_diff',delete_detection='reconcile',consistency_grade='S1',cross_table_consistent=False,
            position_codec='snapshot-zero/v1',position_lineage='mysql_native.'+c['table'],applied_pos='0',full_snapshot=True,columns=columns,key_columns=[f['name'] for f in fields if f.get('key')],tombstones=[],
            arrow_path=t['arrow_path'],max_arrow_bytes=33554432,admission_path=b['admission_receipt_path'],snapshot=b['snapshot'],target=c['target'],mirror=True)
        for n in ['config_digest','schema_digest','type_digest','input_digest']:r[n]=b[n]
        r['audit_digest']=b['capture_receipt_sha256']
        registered=Path(c['registry_template'].format(binding=o['binding'],pin=o['pin']));registered.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(registry,registered)
        write(out/'native-request.tmp',r);os.replace(out/'native-request.tmp',request)
        sys.stdout.write(request.read_text())
if __name__=='__main__':
    try:main()
    except Exception as e:print('mysql producer: '+str(e),file=sys.stderr);sys.exit(1)
