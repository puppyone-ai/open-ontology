#!/usr/bin/env python3
"""Dictionary-derived capture through genuine admission, job lease, Loader and catalog."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import uuid
import configured_capture
import configured_worker
import native_producer

ROOT=Path(__file__).resolve().parents[2]
RUNTIME=Path('/private/tmp/oo-dl01w1-04-native-runtime-20261012')

def saved_run(base,name,command,body=None):
    p=subprocess.run(command,input=body,capture_output=True,timeout=120)
    for suffix,raw in [('stdout',p.stdout),('stderr',p.stderr)]: (base/(name+'.'+suffix)).write_bytes(raw)
    (base/(name+'.exit')).write_text(str(p.returncode)+'\n')
    if p.returncode: raise ValueError(name+' refused; actual exit retained')
    return p.stdout

def docker(base,registry,command):
    args=['docker','run','--rm','--pull','never','--platform','linux/arm64','--label','oo.owner=oracle-connection-whole','--label','oo.kind=proof-runner','-i','--network','container:oo-dl01w1-04-p2-pg','--memory','512m','--cpus','0.5','--cap-drop','ALL','--security-opt','no-new-privileges','-v',str(base)+':'+str(base),'-v',str(RUNTIME)+':/run/secrets:ro','-v',str(configured_worker.LOADER)+':/opt/loader:ro','-v',str(native_producer.ADMISSION)+':/opt/admission:ro','-v','/private/tmp/oo-dl01w1-04-native-source-cli:/private/tmp/oo-dl01w1-04-native-source-cli:ro']
    if registry:
        args+=['-e','OO_NATIVE_ADMISSION_BIN=/opt/admission','-e','OO_NATIVE_BINDING_REGISTRY='+str(registry),'-e','OO_NATIVE_BINDING_REGISTRY_SHA256='+hashlib.sha256(registry.read_bytes()).hexdigest()]
    return args+[native_producer.IMAGE,*command]

def run(cfg,base,generation=1,previous=None):
    base=base.resolve();base.mkdir(mode=0o700,parents=True,exist_ok=False)
    if len(cfg['selections'])!=1: raise ValueError('one approved object per native binding')
    capture=base/'capture';configured_capture.run(cfg,capture,generation,previous/'capture' if previous else None)
    return load_capture(cfg, base, generation, previous)

def resume_capture(cfg, base, generation, previous):
    """Seal and load a capture directory that already exists. Does not recapture rows."""
    base=base.resolve()
    capture=base/'capture'
    if not (capture/'capture.json').is_file():
        configured_capture.seal_existing(cfg, capture, generation)
    if (base/'loader-result.json').is_file():
        return json.loads((base/'loader-result.json').read_text())
    return load_capture(cfg, base, generation, previous)

def load_capture(cfg, base, generation, previous):
    if len(cfg['selections'])!=1: raise ValueError('one approved object per native binding')
    capture=base/'capture'
    selection=cfg['selections'][0];source=selection['source_id'];owner,table=selection['object'].split('.')
    sealed=base/'sealed'
    for binary,digest in [(configured_worker.LOADER,configured_worker.LOADER_SHA),(native_producer.ADMISSION,native_producer.ADMISSION_SHA)]:
        if hashlib.sha256(binary.read_bytes()).hexdigest()!=digest: raise ValueError('fixed ELF drift')
    saved_run(base,'seal',docker(base,None,['/opt/admission','seal-native','--report',str(capture/'witness'/source/'report.json'),'--config',str(capture/'witness'/source/'config.json'),'--out',str(sealed)]))
    registry=sealed/'registry.json';entry=json.loads(registry.read_text())['bindings'][0]
    target={'host':'oo-dl01w1-04-p2-pg','port':5432,'database':'previewdb','runtime_principal':'oo04_native_runtime','password_file':'/run/secrets/runtime-password','ca_file':'/run/secrets/ca.crt','shard':source,'owner':'c6c5f31b-478b-482a-9986-3d06914fe366'}
    request={'protocol':'oo.pipeline.native-update/v1','source_engine':'oracle','source_id':source,'source_generation':generation,'parent_source_generation':None if generation==1 else generation-1,'full_snapshot':True,'snapshot':entry['snapshot'],'batch_id':source+'-g'+str(generation),'capture_method':'c5_diff','delete_detection':'reconcile','consistency_grade':'S0','cross_table_consistent':False,'position_codec':'snapshot-zero/v1','position_lineage':'configured.oracle','applied_pos':'0','row_identity':'none','logical_schema':owner,'logical_table':table,'columns':[dict(field,logical_name=field['name']) for field in entry['fields']],'arrow_path':str(capture/'run/arrow'/(table.lower()+'-gen-'+str(generation)+'.arrow')),'tombstones':[],'mirror':False,'audit_digest':entry['capture_receipt_sha256'],'admission_path':entry['admission_receipt_path'],'registry_path':str(registry),'admission_expect':{'source_engine':'oracle','source_id':source,'source_generation':generation,'declared_receipt_sha256':entry['admission_receipt_sha256']},'target':target,**{key:entry[key] for key in ('config_digest','schema_digest','type_digest','input_digest')}}
    request_path=base/'request.json';request_path.write_text(json.dumps(request,indent=2)+'\n')
    identity=json.loads((previous/'worker-identity.json').read_text()) if previous else {'binding':str(uuid.uuid4()),'project':str(uuid.uuid4())}
    (base/'worker-identity.json').write_text(json.dumps(identity)+'\n')
    manifest={'protocol':'oo.jobs.native-schedule/v1','binding':identity['binding'],'project_id':identity['project'],'source_id':source,'method':'C5','cycle':'pull','period_ms':sum(cfg['periods'][key] for key in ('detect_period_seconds','pull_period_seconds','reconcile_period_seconds'))*1000,'producer':{'path':'/usr/bin/cat','sha256':'0d5a5dd543ed8d31220320eb513e4680ab303e48cb22a5edbf27a5a91561a71e','args':[str(request_path)]},'loader':{'path':'/opt/loader','sha256':configured_worker.LOADER_SHA,'args':['import-native-update']},'registration':{'admission_binary':'/opt/admission','admission_sha256':native_producer.ADMISSION_SHA,'registry_path':str(registry)}}
    manifest_path=base/'manifest.json';manifest_path.write_text(json.dumps(manifest,indent=2)+'\n')
    for name,args in [('put',['native-put','--manifest',str(manifest_path)]),('fire',['native-fire','--project',identity['project']]),('step',['native-step','--project',identity['project'],'--worker','oracle-connection-whole'])]:
        if configured_worker.run(base,name,args): raise ValueError(name+' failed; raw exit retained')
    if configured_worker.run(base,'disable',['native-enable','--binding',identity['binding'],'--enabled','false']):raise ValueError('disable failed')
    query="SELECT result::text FROM jw_native.watermarks WHERE binding_id='"+identity['binding']+"'::uuid"
    result=json.loads(saved_run(base,'read-result',['docker','exec','-u','postgres','oo-dl01w1-04-p2-pg','psql','-d','previewdb','-At','-c',query]))
    if result.get('source_generation')!=generation or result.get('load_state')!='awaiting_publish':raise ValueError('actual generation not applied')
    (base/'loader-result.json').write_text(json.dumps(result,indent=2)+'\n')
    catalog_request={'protocol':'oo.pipeline.native-sealed-catalog/v1','source_id':source,'run_id':result['run_id'],'owner':target['owner'],'source_generation':generation,'target_generation':result['target_generation'],'snapshot':entry['snapshot'],'target':{key:target[key] for key in ('host','port','database','runtime_principal','password_file','ca_file')},**{key:entry[key] for key in ('config_digest','schema_digest','type_digest','input_digest')}}
    raw=saved_run(base,'catalog',docker(base,registry,['/opt/loader','catalog-import-native']),json.dumps(catalog_request).encode())
    (base/'catalog.json').write_bytes(raw)
    return base

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--config',type=Path,required=True);p.add_argument('--out',type=Path,required=True);p.add_argument('--generation',type=int,default=1);p.add_argument('--previous',type=Path);a=p.parse_args()
    try:run(json.loads(a.config.read_text()),a.out,a.generation,a.previous.resolve() if a.previous else None)
    except (ValueError,KeyError,OSError,subprocess.TimeoutExpired) as e:print('oracle native chain refused: '+str(e),file=sys.stderr);sys.exit(2)
