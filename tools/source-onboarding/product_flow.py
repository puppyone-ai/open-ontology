#!/usr/bin/env python3
"""Shared real fixture S0–S10/Pack caller; identity remains the actual OIDC service."""
import argparse
import hashlib
import json
import os
import re
import stat
from pathlib import Path
import ssl
import subprocess
import sys
import time
import urllib.request
import uuid
from onboard import private, load, save, require, digest, Refused, maintenance_consumption, pairs, S5_MODELS, hex64

ROOT = Path(__file__).resolve().parents[2]
OPERATOR = ROOT/'tools/selfbuild-control-api/recipes/operator.py'
# Immutable S5 proof only. Maintenance files are produced by this invocation.
RETAINED = ('build.json','product-receipt.json','role-pack.json','catalog.json','effective.json','oos/ontology.yaml','oos/effective-oos.yaml','oos/source.json')

def _write_once(path, data, uid, token):
    """Create one private artifact without replacing evidence from an earlier attempt."""
    path=Path(path)
    parent=path.parent
    parent.mkdir(mode=0o700,parents=True,exist_ok=True)
    mode=parent.lstat()
    require(stat.S_ISDIR(mode.st_mode) and not stat.S_ISLNK(mode.st_mode) and mode.st_uid==uid and mode.st_mode & 0o777==0o700, token)
    try:
        fd=os.open(path,os.O_WRONLY|os.O_CREAT|os.O_EXCL|getattr(os,'O_NOFOLLOW',0),0o600)
    except FileExistsError:
        existing=path.lstat()
        require(stat.S_ISREG(existing.st_mode) and not stat.S_ISLNK(existing.st_mode) and existing.st_uid==uid and existing.st_mode & 0o777==0o600 and path.read_bytes()==data, token)
        return
    with os.fdopen(fd,'wb') as target:
        target.write(data);target.flush();os.fsync(target.fileno())
    parent_fd=os.open(parent,os.O_RDONLY|getattr(os,'O_DIRECTORY',0))
    try: os.fsync(parent_fd)
    finally: os.close(parent_fd)

def _private_json_receipt(path, descriptor, bundle_root, evidence_id, uid):
    require(isinstance(descriptor,dict) and set(descriptor)=={'path','sha256','size','uid','mode'},'maintenance_source_semantics')
    relative=descriptor.get('path')
    require(isinstance(relative,str),'maintenance_source_semantics')
    rel=Path(relative)
    parts=rel.parts
    require(not rel.is_absolute() and len(parts)==3 and parts[0]=='source-evidence' and parts[1]==evidence_id and all(part not in {'','.','..'} for part in parts),'maintenance_source_semantics')
    require(parts[2]==path,'maintenance_source_semantics')
    require(isinstance(descriptor.get('sha256'),str) and re.fullmatch(r'[a-f0-9]{64}',descriptor['sha256']) is not None,'maintenance_source_semantics')
    require(type(descriptor.get('size')) is int and 0 < descriptor['size'] <= 1_048_576 and descriptor.get('uid')==uid and descriptor.get('mode')==0o600,'maintenance_source_semantics')
    for directory in (Path(bundle_root)/'source-evidence',Path(bundle_root)/'source-evidence'/evidence_id):
        directory_stat=directory.lstat()
        require(stat.S_ISDIR(directory_stat.st_mode) and not stat.S_ISLNK(directory_stat.st_mode) and directory_stat.st_uid==uid and directory_stat.st_mode & 0o777==0o700,'maintenance_source_semantics')
    source=Path(bundle_root)/rel
    source_stat=source.lstat()
    require(stat.S_ISREG(source_stat.st_mode) and not stat.S_ISLNK(source_stat.st_mode) and source_stat.st_uid==uid and source_stat.st_mode & 0o777==0o600 and source_stat.st_size==descriptor['size'],'maintenance_source_semantics')
    raw=source.read_bytes()
    require(hashlib.sha256(raw).hexdigest()==descriptor['sha256'],'maintenance_source_semantics')
    try: parsed=json.loads(raw.decode(),object_pairs_hook=pairs)
    except (UnicodeError,json.JSONDecodeError): raise Refused('maintenance_source_semantics')
    return raw,parsed

def _doris_source_semantics(raw_bytes, bundle_root, build_root, catalog_sha, source_path, archive_root, receipt_root):
    """Normalize only the legacy Doris sidecar; preserve and bind every source byte."""
    try: raw=json.loads(raw_bytes.decode(),object_pairs_hook=pairs)
    except (UnicodeError,json.JSONDecodeError): raise Refused('maintenance_source_semantics')
    require(isinstance(raw,dict) and raw.get('protocol')=='oo.source.doris.audit/v1','maintenance_source_semantics')
    uid=os.getuid()
    for directory,private_mode in ((bundle_root,True),(build_root,False)):
        st=Path(directory).lstat()
        mode_ok=st.st_mode & 0o777==0o700 if private_mode else st.st_mode & 0o022==0
        require(stat.S_ISDIR(st.st_mode) and not stat.S_ISLNK(st.st_mode) and st.st_uid==uid and mode_ok,'maintenance_source_semantics')
    manifest_path=Path(bundle_root)/'manifest.json'
    catalog_path=Path(bundle_root)/'catalog.json'
    for file in (manifest_path,catalog_path):
        st=file.lstat()
        require(stat.S_ISREG(st.st_mode) and not stat.S_ISLNK(st.st_mode) and st.st_uid==uid and st.st_mode & 0o777==0o600,'maintenance_source_semantics')
    try:
        manifest=json.loads(manifest_path.read_text(),object_pairs_hook=pairs)
        catalog=json.loads(catalog_path.read_text(),object_pairs_hook=pairs)
    except (UnicodeError,json.JSONDecodeError): raise Refused('maintenance_source_semantics')
    require(manifest.get('files',{}).get('catalog.json')==catalog_sha==hashlib.sha256(catalog_path.read_bytes()).hexdigest(),'maintenance_source_semantics')
    require(catalog.get('protocol')=='oo.pipeline.sealed-import-collection/v1' and isinstance(catalog.get('entries'),list) and catalog['entries'],'maintenance_source_semantics')
    source_id=raw.get('source_id')
    require(isinstance(source_id,str) and source_id and source_id=='dorispreview','maintenance_source_semantics')
    require(raw.get('collection_sha256')==catalog_sha and type(raw.get('source_generation')) is int and raw['source_generation']>0 and type(raw.get('target_generation')) is int and raw['target_generation']>0,'maintenance_source_semantics')
    require(raw.get('consistency_grade')=='per_table' and raw.get('cross_table_consistent') is False and raw.get('reader_write_attempts')==0 and raw.get('generation_semantics')=='high_watermark','maintenance_source_semantics')
    entries={}
    alias_to_dataset={}
    for entry in catalog['entries']:
        require(isinstance(entry,dict) and entry.get('engine')=='doris' and entry.get('source_id')==source_id,'maintenance_source_semantics')
        tables=entry.get('tables')
        require(isinstance(tables,list) and len(tables)==1,'maintenance_source_semantics')
        table=tables[0]
        schema=table.get('logical_schema');name=table.get('logical_table')
        require(isinstance(schema,str) and schema and '.' not in schema and '/' not in schema and isinstance(name,str) and name and '.' not in name and '/' not in name,'maintenance_source_semantics')
        dataset=f'{schema}.{name}'
        source_generation=entry.get('source_generation');target_generation=entry.get('target_generation');run_id=entry.get('run_id')
        require(type(source_generation) is int and source_generation>0 and type(target_generation) is int and target_generation>0 and isinstance(run_id,str) and run_id,'maintenance_source_semantics')
        require(dataset not in entries and name not in alias_to_dataset,'maintenance_source_semantics')
        entries[dataset]={'schema':schema,'table':name,'source_generation':source_generation,'target_generation':target_generation,'run_id':run_id,'catalog_entry':entry}
        alias_to_dataset[name]=dataset
    expected_datasets=set(entries)
    require(len(entries)==7,'maintenance_source_semantics')
    generations=raw.get('table_generations')
    methods=raw.get('capture_methods')
    evidence=raw.get('evidence')
    evidence_id=raw.get('evidence_set_id')
    require(isinstance(evidence_id,str) and re.fullmatch(r'[a-f0-9]{64}',evidence_id) is not None,'maintenance_source_semantics')
    require(isinstance(generations,dict) and set(generations)==expected_datasets,'maintenance_source_semantics')
    require(isinstance(methods,dict) and set(methods)==set(alias_to_dataset),'maintenance_source_semantics')
    require(isinstance(evidence,dict) and set(evidence)==expected_datasets,'maintenance_source_semantics')
    quality=raw.get('quality')
    require(isinstance(quality,dict) and isinstance(quality.get('warnings'),dict),'maintenance_source_semantics')
    warning_aliases=quality['warnings']
    warnings={}
    for key,value in warning_aliases.items():
        dataset=key if key in expected_datasets else alias_to_dataset.get(key)
        require(dataset is not None and isinstance(value,str) and value in {'DELETES_LAGGED','DELETES_UNDETECTED','POSITION_EXPIRED','STALE_RISK','PRECISION_LOSS','SOURCE_BUDGET_EXHAUSTED'} and dataset not in warnings,'maintenance_source_semantics')
        warnings[dataset]=value
    require(set(warnings)==expected_datasets,'maintenance_source_semantics')
    normalized_methods={}
    source_receipts={}
    staged_receipts=[]
    for dataset in sorted(expected_datasets):
        catalog_row=entries[dataset]
        generation=generations.get(dataset)
        require(isinstance(generation,dict) and set(generation)=={'run_id','source_generation','target_generation'},'maintenance_source_semantics')
        require(generation.get('run_id')==catalog_row['run_id'] and type(generation.get('source_generation')) is int and generation['source_generation']==catalog_row['source_generation'] and type(generation.get('target_generation')) is int and generation['target_generation']==catalog_row['target_generation'],'maintenance_source_semantics')
        alias=catalog_row['table']
        method=methods.get(alias)
        require(isinstance(method,dict) and {'capture_method','consistency','delete_detection'} <= set(method),'maintenance_source_semantics')
        allowed_method_fields={'capture_method','consistency','delete_detection','quality','current_update','logical_table','source_generation','target_generation'}
        require(set(method) <= allowed_method_fields,'maintenance_source_semantics')
        raw_capture=method.get('capture_method')
        require(raw_capture in {'C4','C5'},'maintenance_source_semantics')
        capture={'C4':'c4_partition','C5':'c5_diff'}[raw_capture]
        deletion=method.get('delete_detection')
        expected_deletion='partition_replace' if capture=='c4_partition' else 'reconcile'
        require(method.get('consistency')=='S0' and deletion==expected_deletion,'maintenance_source_semantics')
        if 'logical_table' in method: require(method['logical_table']==dataset,'maintenance_source_semantics')
        if 'source_generation' in method: require(type(method['source_generation']) is int and method['source_generation']==catalog_row['source_generation'],'maintenance_source_semantics')
        if 'target_generation' in method: require(type(method['target_generation']) is int and method['target_generation']==catalog_row['target_generation'],'maintenance_source_semantics')
        if 'quality' in method: require(method['quality']==warnings[dataset],'maintenance_source_semantics')
        if 'current_update' in method: require(capture=='c5_diff' and method['current_update'] in {'observed','none_observed'},'maintenance_source_semantics')
        pair=evidence.get(dataset)
        require(isinstance(pair,dict) and set(pair)=={'request','load'},'maintenance_source_semantics')
        normalized_methods[dataset]={key:value for key,value in method.items() if key in {'quality','current_update'}}
        normalized_methods[dataset].update({'logical_table':dataset,'capture_method':capture,'consistency':'S0','delete_detection':expected_deletion,'source_generation':catalog_row['source_generation'],'target_generation':catalog_row['target_generation']})
        source_receipts[dataset]={}
        for kind in ('request','load'):
            descriptor=pair[kind]
            expected_name=f'{dataset}.{kind}.json'
            raw_receipt,parsed=_private_json_receipt(expected_name,descriptor,bundle_root,evidence_id,uid)
            require(Path(descriptor['path']).name==expected_name,'maintenance_source_semantics')
            common={'source_id':source_id,'source_engine':'doris','logical_schema':catalog_row['schema'],'logical_table':catalog_row['table'],'source_generation':catalog_row['source_generation']}
            require(all(parsed.get(key)==value for key,value in common.items()) and type(parsed.get('source_generation')) is int,'maintenance_source_semantics')
            if kind=='request':
                protocol='oo.pipeline.native-partition/v1' if capture=='c4_partition' else 'oo.pipeline.native-update/v1'
                require(parsed.get('protocol')==protocol and parsed.get('capture_method')==capture and parsed.get('delete_detection')==expected_deletion and parsed.get('consistency_grade')=='S0' and type(parsed.get('full_snapshot')) is bool and parsed.get('full_snapshot')==(capture=='c5_diff'),'maintenance_source_semantics')
            else:
                require(parsed.get('protocol')=='oo.pipeline.native-update-result/v1' and type(parsed.get('target_generation')) is int and parsed.get('target_generation')==catalog_row['target_generation'] and parsed.get('load_state')=='awaiting_publish' and parsed.get('run_id')==catalog_row['run_id'],'maintenance_source_semantics')
            source_receipts[dataset][kind]=descriptor['sha256']
            staged_receipts.append((Path(descriptor['path']),raw_receipt,descriptor))
    preimage_evidence=raw.get('source_preimage_evidence')
    preimages=raw.get('source_preimages')
    require(isinstance(preimage_evidence,dict) and set(preimage_evidence)=={'schema','type'} and isinstance(preimages,dict) and set(preimages)=={'schema_sha256','type_sha256'},'maintenance_source_semantics')
    preimage_files={'schema':'source-schema-preimage.json','type':'source-type-preimage.json'}
    for kind,filename in preimage_files.items():
        descriptor=preimage_evidence[kind]
        raw_receipt,parsed=_private_json_receipt(filename,descriptor,bundle_root,evidence_id,uid)
        require(Path(descriptor['path']).name==filename and preimages.get(kind+'_sha256')==descriptor['sha256'],'maintenance_source_semantics')
        require(isinstance(parsed,(dict,list)),'maintenance_source_semantics')
        staged_receipts.append((Path(descriptor['path']),raw_receipt,descriptor))
    normalized=dict(raw)
    normalized['capture_methods']=normalized_methods
    quality_copy=dict(quality);quality_copy['warnings']=dict(sorted(warnings.items()));normalized['quality']=quality_copy
    normalized_bytes=(json.dumps(normalized,indent=2,ensure_ascii=False)+'\n').encode()
    normalized_sha=hashlib.sha256(normalized_bytes).hexdigest()
    raw_sha=hashlib.sha256(raw_bytes).hexdigest()
    evidence_root=Path(build_root)/'source-evidence'
    evidence_dir=evidence_root/evidence_id
    for directory in (evidence_root,evidence_dir):
        directory.mkdir(mode=0o700,exist_ok=True)
        st=directory.lstat()
        require(stat.S_ISDIR(st.st_mode) and not stat.S_ISLNK(st.st_mode) and st.st_uid==uid and st.st_mode & 0o777==0o700,'maintenance_source_semantics')
    for rel,raw_receipt,descriptor in staged_receipts:
        target=Path(build_root)/rel
        _write_once(target,raw_receipt,uid,'maintenance_source_semantics')
        staged_stat=target.lstat()
        require(staged_stat.st_size==descriptor['size'] and staged_stat.st_uid==descriptor['uid'] and staged_stat.st_mode & 0o777==descriptor['mode'] and hashlib.sha256(target.read_bytes()).hexdigest()==descriptor['sha256'],'maintenance_source_semantics')
    semantics_dir=Path(build_root)/'source-semantics'
    semantics_dir.mkdir(mode=0o700,exist_ok=True)
    semantics_stat=semantics_dir.lstat()
    require(stat.S_ISDIR(semantics_stat.st_mode) and not stat.S_ISLNK(semantics_stat.st_mode) and semantics_stat.st_uid==uid and semantics_stat.st_mode & 0o777==0o700,'maintenance_source_semantics')
    _write_once(semantics_dir/(normalized_sha+'.json'),normalized_bytes,uid,'maintenance_source_semantics')
    _write_once(Path(archive_root)/(raw_sha+'.json'),raw_bytes,uid,'maintenance_source_semantics')
    receipt={'protocol':'oo.source-onboarding.doris-semantics-normalization/v1','source_file':Path(source_path).name,'raw_source_semantics_sha256':raw_sha,'normalized_source_semantics_sha256':normalized_sha,'catalog_sha256':catalog_sha,'evidence_set_id':evidence_id,'tables':{dataset:{'run_id':entries[dataset]['run_id'],'source_generation':entries[dataset]['source_generation'],'target_generation':entries[dataset]['target_generation'],'capture_method':normalized_methods[dataset]['capture_method'],'consistency':normalized_methods[dataset]['consistency'],'delete_detection':normalized_methods[dataset]['delete_detection'],'request_sha256':source_receipts[dataset]['request'],'load_sha256':source_receipts[dataset]['load']} for dataset in sorted(expected_datasets)},'preimage_sha256':{kind+'_sha256':preimage_evidence[kind]['sha256'] for kind in sorted(preimage_evidence)}}
    receipt_bytes=(json.dumps(receipt,indent=2,ensure_ascii=False)+'\n').encode()
    receipt_sha=hashlib.sha256(receipt_bytes).hexdigest()
    _write_once(Path(receipt_root)/(raw_sha+'-'+normalized_sha+'.json'),receipt_bytes,uid,'maintenance_source_semantics')
    return normalized_bytes,receipt_bytes,receipt_sha

class Flow:
    def __init__(self, config, out):
        self.c=config;self.out=out;out.mkdir(mode=0o700,parents=True,exist_ok=True)
        self.current_source_semantics_file=None
        require(not out.is_symlink() and not out.stat().st_mode & 0o077, 'output_not_private')
        self.counter=max([int(p.name.split('-')[0]) for p in out.glob('[0-9][0-9][0-9][0-9]-*.response.json')]+[0])
    def current(self, role):
        cookie=private(self.c['cookies'][role]).read_text().strip()
        req=urllib.request.Request(self.c['identity_origin'].rstrip('/')+'/control/v1/identity/current',headers={'Cookie':'__Host-oo-session='+cookie})
        with urllib.request.urlopen(req,context=ssl.create_default_context(cafile=self.c['identity_ca']),timeout=30) as response:
            current=json.load(response)
        require(current.get('install_state') == 'initialized', 'identity_not_initialized')
        return current['revision']
    def call(self, role, path, body=None, name='read'):
        self.counter+=1
        prefix=self.out/f'{self.counter:04d}-{name}'
        self.last_prefix=prefix
        argv=[sys.executable,str(OPERATOR),'--endpoint',self.c['endpoint'],'--origin',self.c['origin'],'--ca',self.c['ca'],'--cookie-file',self.c['cookies'][role],'--identity-origin',self.c['identity_origin'],'--identity-ca',self.c['identity_ca'],'--path',path,'--out',str(prefix.with_suffix('.response.json'))]
        if body is not None:
            body=dict(body,identity_revision=self.current(role),idempotency_key=uuid.uuid4().hex)
            bp=prefix.with_suffix('.body.json');save(bp,body);argv+=['--body-file',str(bp)]
        result=subprocess.run(argv,capture_output=True,timeout=65)
        for suffix,data in [('stdout',result.stdout),('stderr',result.stderr)]:
            p=prefix.with_suffix('.'+suffix);p.write_bytes(data);p.chmod(0o600)
        save(prefix.with_suffix('.exit.json'),{'exit':result.returncode})
        rp=prefix.with_suffix('.response.json')
        if rp.exists():rp.chmod(0o600)
        require(result.returncode == 0, 'actual_api_refused')
        response=load(rp);require(response['status'] < 400, 'actual_api_refused')
        return response['response']
    def run(self, prepare_only=False, decisions_path=None, reuse_product=None, reuse_receipt=None, current_source_semantics=None):
        c=self.c
        self.current_source_semantics_file=Path(current_source_semantics) if current_source_semantics else None
        require(c.get('fixture_authorized') is True, 'fixture_authority_required')
        require(digest(OPERATOR) == c['operator_sha256'], 'operator_drift')
        if reuse_product or reuse_receipt:
            require(isinstance(reuse_product, str) and reuse_product and hex64(reuse_receipt), 'product_resume_drift')
            self.bind_retained(reuse_product, reuse_receipt)
            reused=self.completed()
            require(reused is not None, 'product_resume_drift')
            return self.bind_current(reused)
        reused=self.completed()
        if reused is not None: return self.bind_current(reused)
        build_file=self.out/'build.json'
        if build_file.exists():
            saved=load(build_file);require(saved['config_sha256'] == c['_sha256'], 'product_resume_drift');build=saved['build_id']
        else:
            require(not (self.out/'create-started.json').exists(), 'create_resume_uncertain')
            save(self.out/'create-started.json',{'config_sha256':c['_sha256'],'model_build_may_have_started':True})
            created=self.call('admin','/control/v1/builds',c['create'],'create')
            build=created['build_id'];save(build_file,{'build_id':build,'config_sha256':c['_sha256']})
        path='/control/v1/builds/'+build
        deadline=time.monotonic()+c.get('model_deadline_seconds',1800)
        while True:
            state=self.call('admin',path,name='poll')
            stages={x['stage']:x for x in state['stages']}
            require(not any(x['status'] in {'failed','canceled','resume_uncertain'} for x in stages.values()), 'stage_failed')
            if all(stages[f'S{i}']['status'] == 'succeeded' for i in range(9)):break
            require(time.monotonic()<deadline,'model_deadline');time.sleep(5)
        actual_build=Path(c['work_dir_host'])/'builds'/build
        catalog_file=actual_build/'catalog.json'
        require(digest(catalog_file) == c['catalog_sha256'], 'actual_catalog_not_fresh_input')
        copied=self.out/'catalog.json';copied.write_bytes(catalog_file.read_bytes());copied.chmod(0o600)
        s5=load(actual_build/'S5.response.json')
        require(s5.get('outcome') == 'validated' and s5.get('builder_spawned') is True and s5.get('structural_draft') is False, 'not_real_s5')
        require(type(s5.get('llm_calls')) is int and s5['llm_calls'] >= 0 and isinstance(s5.get('cache_hit'),bool), 'model_accounting')
        generation_path=actual_build/'generation.json';generation=load(generation_path)
        require(digest(generation_path) == s5['generation_sha256'] and generation.get('outcome')=='packed','model_generation_binding')
        require(generation.get('evidence_hash') == c['catalog_sha256'],'model_source_binding')
        require(re.fullmatch(r'[a-f0-9]{64}',generation.get('input_sha256','')) is not None,'model_input_hash')
        identity=generation.get('response_identity',{})
        require(identity.get('model') in {'gpt-6.1-sol','grok-4.7','grok-4.7-build'} and identity.get('id') and len(identity.get('envelope_sha256',''))==64,'model_identity')
        usage=identity.get('usage',{})
        require(all(type(usage.get(k)) is int and usage[k]>=0 for k in ('prompt_tokens','completion_tokens','total_tokens')) and usage['prompt_tokens']+usage['completion_tokens']==usage['total_tokens'],'model_usage')
        cache=load(actual_build/'cache'/'pg-builder'/(generation['input_sha256']+'.json'))
        require(cache.get('input_sha256') == generation['input_sha256'] and cache.get('evidence_hash') == c['catalog_sha256'] and cache.get('response_identity') == identity and cache.get('outcome') == 'validated','actual_validated_cache')
        if not s5['cache_hit']:
            envelope=actual_build/'native-model-output/model-envelope.private.json'
            require(digest(envelope)==identity['envelope_sha256'],'provider_envelope_binding')
        save(self.out/'actual-generation.json',generation)
        save(self.out/'actual-cache-proof.json',{'input_sha256':cache['input_sha256'],'evidence_hash':cache['evidence_hash'],'response_identity':identity,'cache_file_sha256':digest(actual_build/'cache'/'pg-builder'/(generation['input_sha256']+'.json')),'cache_hit':s5['cache_hit'],'llm_calls':s5['llm_calls']})
        save(self.out/'actual-s5.json',s5)
        prepared_overlay=self.call('admin',path+'/overlay',name='prepared-overlay')
        save(self.out/'prepared-overlay.json',prepared_overlay)
        if prepare_only:
            save(self.out/'prepared.json',{'build_id':build,'s0_s8':'succeeded','overlay_sha256':digest(self.out/'prepared-overlay.json'),'awaiting_fixture_review':True})
            return {'build_id':build,'s0_s8':'succeeded','awaiting_fixture_review':True,'published':False}
        decisions=c
        if decisions_path:
            dp=private(decisions_path);decisions=load(dp)
            require(set(decisions) == {'reviews','confirm_columns','role','fixture_authorized'},'decisions_shape')
            require(decisions['fixture_authorized'] is True,'fixture_authority_required')
            binding=self.out/'decisions-binding.json'
            if binding.exists():require(load(binding)['sha256'] == digest(dp),'decisions_drift')
            else:save(binding,{'sha256':digest(dp),'build_id':build})
        # Explicit source-owner decisions, authenticated with actual fixture roles.
        for decision in decisions['reviews']:
            require(set(decision) == {'name','binds_fact','decision'}, 'review_shape')
            self.call('reviewer',path+'/review',decision,'review')
        confirmed=self.call('owner',path+'/confirm',{'columns':decisions['confirm_columns']},'confirm')
        require(confirmed.get('business_owner_signed') is False and confirmed.get('signer_class') == 'control_plane_test','business_signature_not_fixture')
        pack=self.call('admin',path+'/role-pack',{'policy_version':'oos/v1','role':decisions['role']},'role-pack')
        fetched=self.call('admin',path+'/role-pack?role='+decisions['role'],name='pack')
        final=self.call('admin',path,name='final-stages')
        stages={x['stage']:x for x in final['stages']}
        require(all(stages[f'S{i}']['status'] == 'succeeded' for i in range(11)), 'incomplete_s0_s10')
        save(self.out/'pack.json',fetched)
        overlay=self.call('admin',path+'/overlay',name='overlay');save(self.out/'overlay.json',overlay)
        native = pack.get('native_maintenance')
        require(isinstance(native,dict), 'native_compiler_receipt_missing')
        remote = Path(native['artifact_directory'])
        relative = remote.relative_to(Path(c['work_dir_container']))
        artifact_dir = Path(c['work_dir_host'])/relative
        require(artifact_dir.resolve().is_relative_to(Path(c['work_dir_host']).resolve()), 'artifact_root')
        for name in ('effective.json','reviewed.json','role-pack.json','receipt.json','visibility.json'):
            original=artifact_dir/name;require(original.is_file() and not original.is_symlink(),'native_artifact_missing')
            target=self.out/name;target.write_bytes(original.read_bytes());target.chmod(0o600)
        effective=load(self.out/'effective.json')
        require(effective.get('protocol') == 'oo.builder.effective-oos/v1','effective_protocol')
        exporter=ROOT/'tools/selfbuild-control-api/recipes/export_oos.py'
        require(digest(exporter) == c['export_oos_sha256'],'exporter_drift')
        exported=subprocess.run([sys.executable,str(exporter),'--effective',str(self.out/'effective.json'),'--out',str(self.out/'oos')],capture_output=True,timeout=30)
        require(exported.returncode==0,'oos_export_failed')
        oos_names=('ontology.yaml','effective-oos.yaml','source.json')
        (self.out/'oos').chmod(0o700)
        for file in (self.out/'oos').rglob('*'): file.chmod(0o700 if file.is_dir() else 0o600)
        standard={name:digest(self.out/'oos'/name) for name in oos_names}
        require(load(self.out/'oos'/'source.json').get('actual_core_effective_sha256')==digest(self.out/'effective.json'),'oos_source_binding')
        s5_proof={'outcome':s5['outcome'],'builder_spawned':s5['builder_spawned'],'structural_draft':s5['structural_draft'],'cache_hit':s5['cache_hit'],'llm_calls':s5['llm_calls'],'model':identity['model'],'input_sha256':generation['input_sha256'],'evidence_hash':generation['evidence_hash']}
        receipt=self.out/'product-receipt.json'
        save(receipt,{'protocol':'oo.source-onboarding.product/v1','build_id':build,'stages':list(stages.values()),'pack':pack,'pack_artifact_sha256':digest(self.out/'pack.json'),'effective_oos_sha256':digest(self.out/'effective.json'),'native_role_pack_sha256':digest(self.out/'role-pack.json'),'native_artifact_revision':native['artifact_revision'],'overlay_sha256':digest(self.out/'overlay.json'),'catalog_sha256':c['catalog_sha256'],'source_fresh':True,'s5':s5_proof,'standard_oos':standard,'producer':'tools/source-onboarding/product_flow.py','producer_sha256':digest(Path(__file__).resolve()),'fixture_review':True,'business_owner_signed':False,'published':False,'gateway_dialogue_complete':False})
        return {'build_id':build,'s0_s10':'succeeded','fixture_review':True,'published':False}
    def bind_retained(self, src, receipt_sha):
        """Bind one owner-private S5 product into this empty run. Do not create, and do not copy its maintenance call."""
        src=self.owned(src, stat.S_ISDIR)
        require(src.resolve()!=self.out.resolve() and not self.out.resolve().is_relative_to(src.resolve()) and not src.resolve().is_relative_to(self.out.resolve()), 'product_resume_drift')
        receipt_path=self.owned(src/'product-receipt.json', stat.S_ISREG)
        raw=receipt_path.read_bytes()
        require(hashlib.sha256(raw).hexdigest()==receipt_sha, 'product_resume_drift')
        try:
            receipt=json.loads(raw.decode(), object_pairs_hook=pairs)
            saved=json.loads(self.owned(src/'build.json', stat.S_ISREG).read_text(), object_pairs_hook=pairs)
        except (UnicodeError, json.JSONDecodeError):
            raise Refused('product_resume_drift')
        require(receipt.get('protocol')=='oo.source-onboarding.product/v1' and receipt.get('producer')=='tools/source-onboarding/product_flow.py' and hex64(receipt.get('producer_sha256')) and receipt.get('source_fresh') is True and receipt.get('fixture_review') is True, 'source_not_fresh')
        s5=receipt.get('s5')
        require(isinstance(s5, dict) and s5.get('outcome')=='validated' and s5.get('builder_spawned') is True and s5.get('structural_draft') is False and s5.get('model') in S5_MODELS and hex64(s5.get('input_sha256')), 'not_real_s5')
        require(isinstance(receipt.get('stages'), list) and {x.get('stage') for x in receipt['stages'] if isinstance(x, dict) and x.get('status')=='succeeded'} >= {f'S{i}' for i in range(11)}, 'not_real_s5')
        require(saved.get('config_sha256')==self.c['_sha256'] and saved.get('build_id') and saved.get('build_id')==receipt.get('build_id'), 'product_resume_drift')
        pack_doc=receipt.get('pack')
        require(isinstance(pack_doc, dict) and pack_doc.get('build_id')==receipt['build_id'], 'product_resume_drift')
        source_ref=self.c.get('create',{}).get('source_ref') if isinstance(self.c.get('create'), dict) else None
        require(isinstance(source_ref, str) and source_ref, 'maintenance_source')
        require(receipt.get('catalog_sha256')==self.c.get('catalog_sha256')==s5.get('evidence_hash') and digest(self.owned(src/'catalog.json', stat.S_ISREG))==receipt['catalog_sha256'], 'product_resume_drift')
        require(digest(self.owned(src/'role-pack.json', stat.S_ISREG))==receipt.get('native_role_pack_sha256'), 'role_pack_not_current')
        require(digest(self.owned(src/'effective.json', stat.S_ISREG))==receipt.get('effective_oos_sha256'), 'product_resume_drift')
        standard=receipt.get('standard_oos')
        require(isinstance(standard, dict) and set(standard)=={'ontology.yaml','effective-oos.yaml','source.json'}, 'product_resume_drift')
        self.owned(src/'oos', stat.S_ISDIR)
        for name, sha in standard.items():
            require(digest(self.owned(src/'oos'/name, stat.S_ISREG))==sha, 'product_resume_drift')
        for rel in RETAINED:
            data=(src/rel).read_bytes(); dest=self.out/rel
            dest.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
            if dest.exists():
                require(dest.is_file() and not dest.is_symlink() and dest.read_bytes()==data, 'product_resume_drift')
            else:
                dest.write_bytes(data)
            dest.chmod(0o600)
    def owned(self, path, kind):
        p=Path(path); st=p.lstat()
        require(kind(st.st_mode) and not stat.S_ISLNK(st.st_mode) and st.st_uid==os.getuid() and not st.st_mode & 0o077, 'product_resume_drift')
        return p
    def completed(self):
        receipt_path=self.out/'product-receipt.json'
        if not receipt_path.exists(): return None
        build_file=self.out/'build.json'
        require(build_file.is_file(), 'product_resume_drift')
        saved=load(build_file)
        require(saved.get('config_sha256') == self.c['_sha256'], 'product_resume_drift')
        receipt=load(receipt_path)
        require(receipt.get('protocol') == 'oo.source-onboarding.product/v1' and receipt.get('build_id') == saved.get('build_id') and receipt.get('catalog_sha256') == self.c['catalog_sha256'], 'product_resume_drift')
        return {'build_id':saved['build_id'],'s0_s10':'succeeded','fixture_review':receipt.get('fixture_review') is True,'published':False,'reused_completed':True}
    def bind_current(self, reused):
        receipt_path=self.out/'product-receipt.json'
        before=receipt_path.read_bytes()
        s5_path=self.out/'role-pack.json'
        s5_before=s5_path.read_bytes() if s5_path.is_file() and not s5_path.is_symlink() else None
        try:
            self.maintain()
        finally:
            require(receipt_path.read_bytes()==before, 'product_resume_drift')
        require(s5_before is not None and s5_path.read_bytes()==s5_before, 'role_pack_not_current')
        done=dict(reused)
        done['maintenance_consumption_sha256']=digest(self.out/'maintenance-consumption.json')
        return done
    def caller_ready(self):
        c=self.c
        for key in ('endpoint','origin','ca','identity_origin','identity_ca','work_dir_host','work_dir_container'):
            require(isinstance(c.get(key), str) and c[key], 'maintenance_not_invoked')
        cookies=c.get('cookies')
        require(isinstance(cookies, dict) and isinstance(cookies.get('admin'), str) and cookies['admin'], 'maintenance_not_invoked')
        private(cookies['admin'])
    def sealed_catalog_sha(self):
        root=Path(self.c['work_dir_host']).parent
        bundle=Path(self.c.get('sealed_bundle_host',str(root/'bundle')))
        require(bundle.is_dir() and not bundle.is_symlink() and bundle.resolve().is_relative_to(root.resolve()), 'maintenance_not_invoked')
        catalog=bundle/'catalog.json'
        manifest_path=bundle/'manifest.json'
        require(catalog.is_file() and not catalog.is_symlink() and manifest_path.is_file() and not manifest_path.is_symlink(), 'maintenance_not_invoked')
        require(catalog.resolve().is_relative_to(root.resolve()), 'maintenance_not_invoked')
        sha=digest(catalog)
        manifest=load(manifest_path)
        files=manifest.get('files') if isinstance(manifest, dict) else None
        require(isinstance(files, dict) and files.get('catalog.json')==sha, 'maintenance_not_invoked')
        return sha
    def maintain(self):
        """Same build: official maintenance and compileRolePack. The S5 receipt bytes stay."""
        self.caller_ready()
        receipt=load(self.out/'product-receipt.json')
        s5_path=self.out/'role-pack.json'
        require(s5_path.is_file() and not s5_path.is_symlink() and digest(s5_path)==receipt.get('native_role_pack_sha256'), 'role_pack_not_current')
        pack_doc=receipt.get('pack')
        role_name=pack_doc.get('role') if isinstance(pack_doc, dict) else None
        require(isinstance(role_name, str) and role_name, 'role_pack_not_current')
        catalog_sha=self.sealed_catalog_sha()
        build=receipt['build_id']
        path='/control/v1/builds/'+build
        body={'catalog_sha256':catalog_sha,'trigger':'source_generation_advanced'}
        semantics_sha=None
        semantics_original_bytes=None
        semantics_normalized_bytes=None
        semantics_normalization_receipt=None
        if self.current_source_semantics_file is not None:
            source=self.owned(self.current_source_semantics_file, stat.S_ISREG)
            semantics_original_bytes=source.read_bytes()
            build_root=Path(self.c['work_dir_host'])/'builds'/build
            require(build_root.is_dir() and not build_root.is_symlink(), 'maintenance_not_invoked')
            try: raw_document=json.loads(semantics_original_bytes.decode(),object_pairs_hook=pairs)
            except (UnicodeError,json.JSONDecodeError): raise Refused('maintenance_source_semantics')
            if isinstance(raw_document,dict) and raw_document.get('protocol')=='oo.source.doris.audit/v1':
                bundle_root=Path(self.c.get('sealed_bundle_host',str(Path(self.c['work_dir_host']).parent/'bundle')))
                archive_root=self.out/'source-semantics-archive'
                receipt_root=self.out/'source-semantics-normalization'
                semantics_normalized_bytes,semantics_normalization_receipt,_=_doris_source_semantics(
                    semantics_original_bytes,bundle_root,build_root,catalog_sha,source,archive_root,receipt_root)
            else:
                semantics_normalized_bytes=semantics_original_bytes
            semantics_sha=hashlib.sha256(semantics_normalized_bytes).hexdigest()
            semantics_dir=build_root/'source-semantics'
            semantics_dir.mkdir(mode=0o700,exist_ok=True)
            require(not semantics_dir.is_symlink() and not semantics_dir.stat().st_mode & 0o077, 'maintenance_source_semantics')
            staged=semantics_dir/(semantics_sha+'.json')
            try:
                fd=os.open(staged, os.O_WRONLY|os.O_CREAT|os.O_EXCL, 0o600)
            except FileExistsError:
                existing=self.owned(staged, stat.S_ISREG).read_bytes()
                require(existing==semantics_normalized_bytes, 'maintenance_source_semantics')
            else:
                with os.fdopen(fd,'wb') as target:
                    target.write(semantics_normalized_bytes);target.flush();os.fsync(target.fileno())
                staged.chmod(0o600)
            body['source_semantics_sha256']=semantics_sha
        self.call('admin', path+'/maintenance', body, 'current-maintenance')
        maint_prefix=self.last_prefix
        role_body=self.call('admin', path+'/role-pack', {'policy_version':'oos/v1','role':role_name}, 'current-role-pack')
        role_prefix=self.last_prefix
        overlay=self.call('admin', path+'/overlay', name='current-overlay')
        native=role_body.get('native_maintenance') if isinstance(role_body, dict) else None
        require(isinstance(native, dict) and isinstance(native.get('artifact_directory'), str) and isinstance(native.get('artifact_revision'), str), 'maintenance_not_invoked')
        try:
            relative=Path(native['artifact_directory']).relative_to(Path(self.c['work_dir_container']))
        except ValueError:
            raise Refused('maintenance_not_invoked')
        artifact=Path(self.c['work_dir_host'])/relative
        require(artifact.is_dir() and not artifact.is_symlink() and artifact.name==native['artifact_revision'] and artifact.resolve().is_relative_to(Path(self.c['work_dir_host']).resolve()), 'maintenance_not_invoked')
        def regular(name, token):
            found=artifact/name
            require(found.is_file() and not found.is_symlink(), token)
            return found.read_bytes()
        pack_bytes=regular('role-pack.json', 'role_pack_not_current')
        command_bytes=regular('command.json', 'maintenance_not_invoked')
        visibility_bytes=regular('visibility.json', 'maintenance_not_invoked')
        source_bytes=regular('source.json', 'maintenance_source_semantics') if native.get('source_semantics_sha256') else None
        current_effective_bytes=regular('effective.json', 'maintenance_source_semantics') if native.get('source_semantics_sha256') else None
        if source_bytes is not None:
            require(hashlib.sha256(source_bytes).hexdigest()==native.get('source_json_sha256'), 'maintenance_source_semantics')
            require(current_effective_bytes is not None and hashlib.sha256(current_effective_bytes).hexdigest()==native.get('effective_sha256'), 'maintenance_source_semantics')
        try:
            command=json.loads(command_bytes.decode(), object_pairs_hook=pairs)
            visibility=json.loads(visibility_bytes.decode(), object_pairs_hook=pairs)
        except (UnicodeError, json.JSONDecodeError):
            raise Refused('maintenance_not_invoked')
        role_bytes=role_prefix.with_suffix('.response.json').read_bytes()
        maint_bytes=maint_prefix.with_suffix('.response.json').read_bytes()
        role_exit=load(role_prefix.with_suffix('.exit.json'))
        maint_exit=load(maint_prefix.with_suffix('.exit.json'))
        require(isinstance(role_exit, dict) and isinstance(maint_exit, dict), 'maintenance_not_invoked')
        overlay_bytes=(json.dumps(overlay, indent=2)+'\n').encode()
        fresh=maintenance_consumption(receipt, digest(self.out/'product-receipt.json'), receipt['native_role_pack_sha256'], pack_bytes, command, visibility, role_bytes, maint_bytes, role_exit.get('exit'), maint_exit.get('exit'), overlay_bytes)
        if source_bytes is not None:
            fresh['source_json_sha256']=hashlib.sha256(source_bytes).hexdigest()
            fresh['effective_json_sha256']=hashlib.sha256(current_effective_bytes).hexdigest()
        def put(name, data):
            target=self.out/name
            target.write_bytes(data); target.chmod(0o600)
        put('current-role-pack.json', pack_bytes)
        put('current-command.json', command_bytes)
        put('current-visibility.json', visibility_bytes)
        put('current-role-response.json', role_bytes)
        put('current-maintenance-response.json', maint_bytes)
        put('current-role-exit.json', role_prefix.with_suffix('.exit.json').read_bytes())
        put('current-maintenance-exit.json', maint_prefix.with_suffix('.exit.json').read_bytes())
        if source_bytes is not None: put('current-source.json', source_bytes)
        if current_effective_bytes is not None: put('current-effective-oos.json', current_effective_bytes)
        if self.current_source_semantics_file is not None:
            require(semantics_original_bytes is not None and semantics_normalized_bytes is not None,'maintenance_source_semantics')
            put('current-source-semantics.json', semantics_normalized_bytes)
            if semantics_normalization_receipt is not None:
                put('current-source-semantics-original.json',semantics_original_bytes)
                put('current-source-semantics-normalization.json',semantics_normalization_receipt)
        put('current-prepared-overlay.json', overlay_bytes)
        save(self.out/'maintenance-consumption.json', fresh)
        if current_effective_bytes is not None:
            exporter=ROOT/'tools/selfbuild-control-api/recipes/export_oos.py'
            require(digest(exporter)==self.c['export_oos_sha256'],'exporter_drift')
            exported=subprocess.run([sys.executable,str(exporter),'--effective',str(self.out/'current-effective-oos.json'),'--out',str(self.out/'current-oos')],capture_output=True,timeout=30)
            require(exported.returncode==0,'oos_export_failed')
            (self.out/'current-oos').chmod(0o700)
            for file in (self.out/'current-oos').rglob('*'): file.chmod(0o700 if file.is_dir() else 0o600)
            require(load(self.out/'current-oos/source.json')['actual_core_effective_sha256']==digest(self.out/'current-effective-oos.json'),'oos_source_binding')
            save(self.out/'current-oos-receipt.json',{'protocol':'oo.source-onboarding.current-standard-oos/v1','source_id':fresh['source_id'],'source_generation':fresh['source_generation'],'current_effective_sha256':digest(self.out/'current-effective-oos.json'),'files':{name:digest(self.out/'current-oos'/name) for name in ('ontology.yaml','effective-oos.yaml','source.json')}})


def main():
    parser=argparse.ArgumentParser();parser.add_argument('--config',required=True);parser.add_argument('--out',required=True);parser.add_argument('--prepare-only',action='store_true');parser.add_argument('--decisions');parser.add_argument('--reuse-product');parser.add_argument('--reuse-receipt');parser.add_argument('--current-source-semantics');parser.add_argument('--current-sealed-bundle');a=parser.parse_args()
    try:
        p=private(a.config);c=load(p);c['_sha256']=digest(p)
        if a.current_sealed_bundle: c['sealed_bundle_host']=a.current_sealed_bundle
        print(json.dumps(Flow(c,Path(a.out)).run(a.prepare_only,a.decisions,a.reuse_product,a.reuse_receipt,a.current_source_semantics)));return 0
    except (Refused,OSError,KeyError,ValueError,subprocess.TimeoutExpired):
        print('source-onboarding-product: refused; private evidence retained',file=sys.stderr);return 2

if __name__=='__main__':sys.exit(main())
