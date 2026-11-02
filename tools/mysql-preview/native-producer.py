#!/usr/bin/python3
"""MySQL C5 adapter: real source capture/admission; worker alone claims/imports."""
import argparse,fcntl,hashlib,json,os,shutil,subprocess,sys,uuid,time
from pathlib import Path

def write(p,x):
    Path(p).write_text(json.dumps(x,indent=2)+'\n')
def call(argv,p):
    started=time.monotonic()
    r=subprocess.run(argv,capture_output=True)
    write(Path(str(p)+'.timing.json'),dict(elapsed_ms=int((time.monotonic()-started)*1000)))
    Path(str(p)+'.stdout').write_bytes(r.stdout);Path(str(p)+'.stderr').write_bytes(r.stderr)
    Path(str(p)+'.exit').write_text(str(r.returncode)+'\n')
    if r.returncode: raise RuntimeError('source stage refused: '+str(p))
def installed_policy(operator,path):
    if 'source_audit_policy' not in operator:return
    if operator['source_audit_policy']!='grant_enforced':raise RuntimeError('installed audit policy invalid')
    config=json.loads(path.read_text());config['source_audit_policy']=operator['source_audit_policy'];write(path,config)
def main():
    os.umask(0o077)
    a=argparse.ArgumentParser();a.add_argument('--config',required=True)
    c=json.loads(Path(a.parse_args().config).read_text());o=json.load(sys.stdin)
    cfg=json.loads(Path(c['capture_config']).read_text())
    if o['protocol']!='oo.jobs.native-producer/v1' or o['source_id']!=cfg['source_id'] or o['method']!='C5':raise RuntimeError('occurrence mismatch')
    for n in ['driver','admission']:
        if hashlib.sha256(Path(c[n]['path']).read_bytes()).hexdigest()!=c[n]['sha256']:raise RuntimeError(n+' identity changed')
    root=Path(c['state_dir']);root.mkdir(parents=True,exist_ok=True)
    pair=str(uuid.UUID(o['binding']))+'_'+str(int(o['pin']))
    with (root/'producer.lock').open('a') as lock:
        fcntl.flock(lock,fcntl.LOCK_EX)
        out=root/pair;out.mkdir(exist_ok=True);request=out/'native-request.json'
        if request.exists():sys.stdout.write(request.read_text());return
        parent=o.get('acknowledged_source_generation');generation=(parent or 0)+1
        cfg['generation']=generation;cfg['owner']=str(uuid.uuid4())
        full = cfg.get('protocol') == 'oo.mysql-full/v1'
        if full:
            if cfg['table']['table'] != c['table'] or cfg['secret_ref'] != c['secret_ref']:raise RuntimeError('selected table/secret mismatch')
        else:
            cfg['tables']=[t for t in cfg['tables'] if t['table']==c['table']]
            if len(cfg['tables']) != 1:raise RuntimeError('selected table missing')
        checkpoints=[path.parent.parent for path in out.glob('*/capture/chunk-binding.json')]
        if len(checkpoints)>1:raise RuntimeError('ambiguous capture checkpoints')
        if checkpoints:
            attempt=checkpoints[0]
            previous=json.loads((attempt/'capture-config.json').read_text())
            cfg['owner']=previous['owner']
            if previous!=cfg:raise RuntimeError('capture configuration changed for occurrence checkpoint')
        else:
            attempt=out/str(uuid.uuid4());attempt.mkdir()
            write(attempt/'capture-config.json',cfg)
        capture=attempt/'capture'
        if not (capture/'capturer.json').exists():
            try:
                call([c['driver']['path'],'capture-full' if full else 'capture-native','--config',str(attempt/'capture-config.json'),'--out',str(capture)],attempt/'capture-call')
            except RuntimeError:
                if not full or not (capture/'rejects.json').exists():raise
                rejects=(capture/'rejects.json').read_bytes()
                r=dict(protocol='oo.jobs.native-c5-rejected/v1',source_id=cfg['source_id'],source_generation=generation,capture_method='c5_diff',logical_schema=cfg['table']['schema'],logical_table=cfg['table']['table'],target=c['target'],rejects=json.loads(rejects),observation_sha256=hashlib.sha256((pair+'\n'+cfg['owner']+'\n').encode()+rejects).hexdigest())
                write(out/'native-request.tmp',r);os.replace(out/'native-request.tmp',request);sys.stdout.write(request.read_text());return
        facts=json.loads((capture/'capturer.json').read_text())
        prepare_args=['/usr/bin/python3',c['prepare'],'--capture',str(capture),'--out',str(attempt/'inputs'),'--secret-ref',c['secret_ref']]
        if c.get('source_audit_policy') is not None:
            if c['source_audit_policy'] != 'grant_enforced':raise RuntimeError('unsupported installed source audit policy')
            prepare_args+=['--source-audit-policy',c['source_audit_policy']]
        if full and 'multipart_scope' in facts:prepare_args+=['--multipart']
        if not (attempt/'inputs'/c['table']/'report.json').exists():
            if (attempt/'inputs').exists():raise RuntimeError('incomplete preparer output requires operator recovery; capture checkpoint retained')
            call(prepare_args,attempt/'prepare-call')
        inputs=attempt/'inputs'/c['table'];sealed=attempt/'sealed'
        installed_policy(c,inputs/'config.json')
        if not (sealed/'registry.json').exists():call([c['admission']['path'],'seal-native','--report',str(inputs/'report.json'),'--config',str(inputs/'config.json'),'--out',str(sealed)],out/'seal-call')
        registry=sealed/'registry.json'
        b=json.loads(registry.read_text())['bindings'][0]
        if b['source_generation']!=generation:raise RuntimeError('sealed generation mismatch')
        facts=json.loads((capture/'capturer.json').read_text());t=next(t for t in facts['tables'] if t['name']==c['table'])
        fields=b['fields'];columns=[dict(f,logical_name=f['name']) for f in fields]
        r=dict(protocol='oo.pipeline.native-update/v1',source_engine='mysql',source_id=b['source_id'],source_generation=generation,parent_source_generation=parent,
            logical_schema=t['schema'],logical_table=t['name'],batch_id=pair,run_id=b['run_id'],capture_method='c5_diff',delete_detection='reconcile',consistency_grade=t.get('consistency','S1'),cross_table_consistent=False,
            position_codec='snapshot-zero/v1',position_lineage=cfg['source_id']+'.'+c['table'],applied_pos='0',full_snapshot=True,columns=columns,key_columns=[f['name'] for f in fields if f.get('key')],tombstones=[],
            arrow_path=str(capture/'0.arrow') if b.get('multipart_anchor') else t['arrow_path'],max_arrow_bytes=cfg['budgets']['max_bytes'] if full else 33554432,admission_path=b['admission_receipt_path'],snapshot=b['snapshot'],target=c['target'],mirror=True)
        if full and (capture/'rejects.json').exists():
            policy=cfg.get('type_policy',{})
            if not any(policy.get(k) is True for k in ['zero_date_to_null','strip_text_nul']):raise RuntimeError('normalization policy missing')
            notes=capture/'rejects.json'
            if notes.stat().st_size>33554432:raise RuntimeError('normalization notes budget')
            r['type_policy']=policy
            r['normalization_rejects']=dict(path=str(notes),sha256=hashlib.sha256(notes.read_bytes()).hexdigest())
        for n in ['config_digest','schema_digest','type_digest','input_digest']:r[n]=b[n]
        r['audit_digest']=b['capture_receipt_sha256']
        if b.get('multipart_scope') is not None:r['multipart_scope']=b['multipart_scope']
        registered=Path(c['registry_template'].format(binding=o['binding'],pin=o['pin']));registered.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(registry,registered)
        write(out/'native-request.tmp',r);os.replace(out/'native-request.tmp',request)
        sys.stdout.write(request.read_text())
if __name__=='__main__':
    try:main()
    except Exception as e:print('mysql producer: '+str(e),file=sys.stderr);sys.exit(1)
