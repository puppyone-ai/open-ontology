#!/usr/bin/env python3
"""Dictionary-derived capture through genuine admission, job lease, Loader and catalog."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import time
import uuid
import configured_capture
import configured_worker
import native_producer
import rust_c5_reader

ROOT=Path(__file__).resolve().parents[2]
RUNTIME=Path('/private/tmp/oo-dl01w1-04-native-runtime-20261012')
NATIVE_C5_READER=rust_c5_reader.READER
NATIVE_C5_HELPER=ROOT/'tools/oracle-preview/rust_c5_native_update.py'

def saved_run(base,name,command,body=None):
    p=subprocess.run(command,input=body,capture_output=True,timeout=120)
    for suffix,raw in [('stdout',p.stdout),('stderr',p.stderr)]: (base/(name+'.'+suffix)).write_bytes(raw)
    (base/(name+'.exit')).write_text(str(p.returncode)+'\n')
    if p.returncode: raise ValueError(name+' refused; actual exit retained')
    return p.stdout

def docker(base,registry,command):
    args=['docker','run','--rm','--pull','never','--platform','linux/arm64','--label','oo.owner=oracle-connection-whole','--label','oo.kind=proof-runner','-i','--network','container:oo-dl01w1-04-p2-pg','--memory','512m','--cpus','0.5','--cap-drop','ALL','--security-opt','no-new-privileges','-v',str(base)+':'+str(base),'-v',str(RUNTIME)+':/run/secrets:ro','-v',str(configured_worker.LOADER)+':/opt/loader:ro','-v',str(native_producer.ADMISSION)+':/opt/admission:ro','-v','/private/tmp/oo-dl01w1-04-native-source-cli:/private/tmp/oo-dl01w1-04-native-source-cli:ro']
    args+=['-v',str(rust_c5_reader.READER)+':/opt/oracle-driver-probe:ro']
    if registry:
        args+=['-e','OO_NATIVE_ADMISSION_BIN=/opt/admission','-e','OO_NATIVE_BINDING_REGISTRY='+str(registry),'-e','OO_NATIVE_BINDING_REGISTRY_SHA256='+hashlib.sha256(registry.read_bytes()).hexdigest()]
    return args+[native_producer.IMAGE,*command]

def run(cfg,base,generation=1,previous=None,keep=False):
    base=base.resolve();base.mkdir(mode=0o700,parents=True,exist_ok=False)
    if len(cfg['selections'])!=1: raise ValueError('one approved object per native binding')
    capture=base/'capture';configured_capture.run(cfg,capture,generation,previous/'capture' if previous else None)
    return load_capture(cfg, base, generation, previous, keep)

def resume_capture(cfg, base, generation, previous, keep=False):
    """Seal and load a capture directory that already exists. Does not recapture rows."""
    base=base.resolve()
    capture=base/'capture'
    if not (capture/'capture.json').is_file():
        configured_capture.seal_existing(cfg, capture, generation)
    if (base/'loader-result.json').is_file():
        return json.loads((base/'loader-result.json').read_text())
    return load_capture(cfg, base, generation, previous, keep)

def load_capture(cfg, base, generation, previous, keep=False):
    if len(cfg['selections'])!=1: raise ValueError('one approved object per native binding')
    capture=base/'capture'
    selection=cfg['selections'][0];source=selection['source_id'];owner,table=selection['object'].split('.')
    sealed=base/'sealed'
    for binary,digest in [(configured_worker.LOADER,configured_worker.LOADER_SHA),(native_producer.ADMISSION,native_producer.ADMISSION_SHA)]:
        if hashlib.sha256(binary.read_bytes()).hexdigest()!=digest: raise ValueError('fixed ELF drift')
    registry=sealed/'registry.json'
    if not registry.is_file():
        if sealed.exists(): shutil.rmtree(sealed)
        saved_run(base,'seal',docker(base,None,['/opt/admission','seal-native','--report',str(capture/'witness'/source/'report.json'),'--config',str(capture/'witness'/source/'config.json'),'--out',str(sealed)]))
    entry=json.loads(registry.read_text())['bindings'][0]
    target={'host':'oo-dl01w1-04-p2-pg','port':5432,'database':'previewdb','runtime_principal':'oo04_native_runtime','password_file':'/run/secrets/runtime-password','ca_file':'/run/secrets/ca.crt','shard':source,'owner':'c6c5f31b-478b-482a-9986-3d06914fe366'}
    request={'protocol':'oo.pipeline.native-update/v1','source_engine':'oracle','source_id':source,'source_generation':generation,'parent_source_generation':None if generation==1 else generation-1,'full_snapshot':True,'snapshot':entry['snapshot'],'batch_id':source+'-g'+str(generation),'capture_method':'c5_diff','delete_detection':'reconcile','consistency_grade':'S0','cross_table_consistent':False,'position_codec':'snapshot-zero/v1','position_lineage':'configured.oracle','applied_pos':'0','row_identity':'none','logical_schema':owner,'logical_table':table,'columns':[dict(field,logical_name=field['name']) for field in entry['fields']],'arrow_path':str(capture/'run/arrow'/(table.lower()+'-gen-'+str(generation)+'.arrow')),'tombstones':[],'mirror':False,'audit_digest':entry['capture_receipt_sha256'],'admission_path':entry['admission_receipt_path'],'registry_path':str(registry),'admission_expect':{'source_engine':'oracle','source_id':source,'source_generation':generation,'declared_receipt_sha256':entry['admission_receipt_sha256']},'target':target,**{key:entry[key] for key in ('config_digest','schema_digest','type_digest','input_digest')}}
    request_path=base/'request.json';request_path.write_text(json.dumps(request,indent=2)+'\n')
    identity=json.loads((previous/'worker-identity.json').read_text()) if previous else {'binding':str(uuid.uuid4()),'project':str(uuid.uuid4())}
    (base/'worker-identity.json').write_text(json.dumps(identity)+'\n')
    manifest={'protocol':'oo.jobs.native-schedule/v1','binding':identity['binding'],'project_id':identity['project'],'source_id':source,'method':'C5','cycle':'pull','period_ms':sum(cfg['periods'][key] for key in ('detect_period_seconds','pull_period_seconds','reconcile_period_seconds'))*1000,'producer':{'path':'/usr/bin/cat','sha256':'0d5a5dd543ed8d31220320eb513e4680ab303e48cb22a5edbf27a5a91561a71e','args':[str(request_path)]},'loader':{'path':'/opt/loader','sha256':configured_worker.LOADER_SHA,'args':['import-native-update']},'registration':{'admission_binary':'/opt/admission','admission_sha256':native_producer.ADMISSION_SHA,'registry_path':str(registry)}}
    manifest_path=base/'manifest.json';manifest_path.write_text(json.dumps(manifest,indent=2)+'\n')
    if configured_worker.run(base,'put',['native-put','--manifest',str(manifest_path)]): raise ValueError('put failed; raw exit retained')
    # put does not move next_run_at. Fire only enqueues a binding that is already due.
    due=float(saved_run(base,'due',['docker','exec','-u','postgres','oo-dl01w1-04-p2-pg','psql','-d','previewdb','-At','-c',"SELECT GREATEST(0, EXTRACT(EPOCH FROM (next_run_at - clock_timestamp()))) FROM jw_native.bindings WHERE id='"+identity['binding']+"'::uuid"]).decode().strip() or 0)
    if due>0: time.sleep(due+0.05)
    for name,args in [('fire',['native-fire','--project',identity['project']]),('step',['native-step','--project',identity['project'],'--worker','oracle-connection-whole'])]:
        if configured_worker.run(base,name,args): raise ValueError(name+' failed; raw exit retained')
    # keep defaults false so an existing caller still pauses. The ordinary entry passes true on its last cycle.
    label, enabled = ('enable', 'true') if keep else ('disable', 'false')
    if configured_worker.run(base, label, ['native-enable', '--binding', identity['binding'], '--enabled', enabled]): raise ValueError(label+' failed')
    query="SELECT result::text FROM jw_native.watermarks WHERE binding_id='"+identity['binding']+"'::uuid"
    result=json.loads(saved_run(base,'read-result',['docker','exec','-u','postgres','oo-dl01w1-04-p2-pg','psql','-d','previewdb','-At','-c',query]))
    if result.get('source_generation')!=generation or result.get('load_state')!='awaiting_publish':raise ValueError('actual generation not applied')
    (base/'loader-result.json').write_text(json.dumps(result,indent=2)+'\n')
    catalog_request={'protocol':'oo.pipeline.native-sealed-catalog/v1','source_id':source,'run_id':result['run_id'],'owner':target['owner'],'source_generation':generation,'target_generation':result['target_generation'],'snapshot':entry['snapshot'],'target':{key:target[key] for key in ('host','port','database','runtime_principal','password_file','ca_file')},**{key:entry[key] for key in ('config_digest','schema_digest','type_digest','input_digest')}}
    raw=saved_run(base,'catalog',docker(base,registry,['/opt/loader','catalog-import-native']),json.dumps(catalog_request).encode())
    (base/'catalog.json').write_bytes(raw)
    return base

def configure_current_nochange(cfg,base,binding,project,generation,parent_request,parent_result):
    """Pin a Rust-only quiet-tick producer to the acknowledged Oracle C5 input."""
    base=base.resolve();base.mkdir(mode=0o700,parents=True,exist_ok=False)
    if hashlib.sha256(NATIVE_C5_READER.read_bytes()).hexdigest()!=rust_c5_reader.READER_SHA256:
        raise ValueError('fixed Rust producer drift')
    if len(cfg['selections'])!=1: raise ValueError('one approved Oracle object required')
    selection=cfg['selections'][0];source=selection['source_id'];owner,table=selection['object'].split('.')
    raw=configured_worker.run(base,'native-state-before',['native-state','--binding',binding])
    if raw: raise ValueError('native-state refused')
    state=json.loads((base/'native-state-before.stdout').read_text())
    if state.get('source_id')!=source or state.get('method')!='C5' or state.get('source_generation')!=generation or not state.get('applied'):
        raise ValueError('actual acknowledged Oracle binding state mismatch')
    query="SELECT json_build_object('manifest',b.manifest,'pin',w.pin,'source_generation',w.source_generation,'request',o.request) FROM jw_native.bindings b JOIN jw_native.watermarks w ON w.binding_id=b.id JOIN jw_native.occurrences o ON o.task_id=w.task_id WHERE b.id='"+binding+"'::uuid AND b.project_id='"+project+"'::uuid AND b.source_id='"+source+"' AND b.method='C5'"
    actual=json.loads(saved_run(base,'read-current-occurrence',['docker','exec','-u','postgres','oo-dl01w1-04-p2-pg','psql','-d','previewdb','-At','-c',query]))
    request=actual.get('request') or {}
    if json.loads(Path(parent_request).read_text())!=request:
        raise ValueError('local request differs from immutable scheduler occurrence')
    if actual.get('pin')!=state.get('pin') or actual.get('source_generation')!=generation or request.get('source_generation')!=generation or request.get('source_id')!=source:
        raise ValueError('immutable current Oracle occurrence mismatch')
    parent_arrow=Path(request.get('arrow_path',''))
    if not parent_arrow.is_file() or hashlib.sha256(parent_arrow.read_bytes()).hexdigest()!=request.get('input_digest'):
        raise ValueError('acknowledged Oracle Arrow/input digest mismatch')
    parent_result=Path(parent_result).resolve(strict=True)
    parent_result_value=json.loads(parent_result.read_text())
    if parent_result_value.get('source_id')!=source or parent_result_value.get('content_digest') is None:
        raise ValueError('Rust parent content result mismatch')
    shutil.copyfile(parent_arrow,base/'parent.arrow');shutil.copyfile(parent_result,base/'parent-result.json')
    for path in (base/'parent.arrow',base/'parent-result.json'): path.chmod(0o600)
    parent_capture=parent_arrow.parents[2]
    baseline_capture=base/'baseline'/'capture'
    shutil.copytree(parent_capture,baseline_capture)
    baseline_arrow=baseline_capture/'run/arrow'/(table.lower()+'-gen-'+str(generation)+'.arrow')
    shutil.copyfile(parent_result,Path(str(baseline_arrow)+'.source-result.json'))
    baseline_request=base/'parent-request.json'
    baseline_request_body=dict(request)
    baseline_request_body['arrow_path']=str(baseline_capture/'run/arrow'/(table.lower()+'-gen-'+str(generation)+'.arrow'))
    baseline_request.write_text(json.dumps(baseline_request_body,indent=2)+'\n');baseline_request.chmod(0o600)
    output_root=base/'source-output';output_root.mkdir(mode=0o700)
    (base/'ca-ref').write_text('absent\n');(base/'ca-ref').chmod(0o600)
    producer={'source_id':source,'host':cfg['host'],'owner':owner,'table':table,'key':selection['key'],
        'password_file':'/oracle-source/reader-password',
        'ca_ref_file':str(base/'ca-ref'),'max_rows':cfg['hard_limit'],
        'baseline_generation':generation,'baseline_capture':str(baseline_capture),
        'baseline_input_digest':request['input_digest'],'baseline_request':str(baseline_request),
        'output_root':str(output_root),'worker_helper':'/workspace/tools/oracle-preview/rust_c5_native_update.py',
        'helper_sha256':hashlib.sha256(NATIVE_C5_HELPER.read_bytes()).hexdigest(),
        'producer_sha256':hashlib.sha256(NATIVE_C5_READER.read_bytes()).hexdigest(),
        'target_id':cfg['target_id'],'secret_ref':cfg['secret_ref']}
    (base/'producer.json').write_text(json.dumps(producer,indent=2)+'\n');(base/'producer.json').chmod(0o600)
    manifest=actual['manifest']
    manifest['producer']={'path':'/opt/oracle-driver-probe','sha256':hashlib.sha256(NATIVE_C5_READER.read_bytes()).hexdigest(),
        'args':['native-c5-producer','--config',str(base/'producer.json'),'--binding',binding,
            '--helper-sha256',producer['helper_sha256']]}
    manifest['registration']['registry_path']=str(output_root/'produced'/'{binding}'/'{pin}'/'sealed'/'registry.json')
    manifest_path=base/'manifest.json';manifest_path.write_text(json.dumps(manifest,indent=2)+'\n');manifest_path.chmod(0o600)
    (base/'expected.json').write_text(json.dumps({'binding':binding,'project':project,'source_id':source,'source_generation':generation,
        'parent_pin':actual['pin'],'parent_input_digest':request['input_digest'],'parent_content_digest':parent_result_value['content_digest']},indent=2)+'\n')
    if configured_worker.run(base,'native-put',['native-put','--manifest',str(manifest_path)]):
        raise ValueError('native-put refused Rust C5 producer; raw exit retained')
    return {'state':state,'source_id':source,'source_generation':generation,'parent_pin':actual['pin'],
        'parent_input_digest':request['input_digest'],'manifest_sha256':hashlib.sha256(manifest_path.read_bytes()).hexdigest()}

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--config',type=Path,required=True);p.add_argument('--out',type=Path,required=True);p.add_argument('--generation',type=int,default=1);p.add_argument('--previous',type=Path);a=p.parse_args()
    try:run(json.loads(a.config.read_text()),a.out,a.generation,a.previous.resolve() if a.previous else None)
    except (ValueError,KeyError,OSError,subprocess.TimeoutExpired) as e:print('oracle native chain refused: '+str(e),file=sys.stderr);sys.exit(2)
