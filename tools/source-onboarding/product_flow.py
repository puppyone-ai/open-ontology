#!/usr/bin/env python3
"""Shared real fixture S0–S10/Pack caller; identity remains the actual OIDC service."""
import argparse
import json
import re
from pathlib import Path
import ssl
import subprocess
import sys
import time
import urllib.request
import uuid
from onboard import private, load, save, require, digest, Refused

ROOT = Path(__file__).resolve().parents[2]
OPERATOR = ROOT/'tools/selfbuild-control-api/recipes/operator.py'

class Flow:
    def __init__(self, config, out):
        self.c=config;self.out=out;out.mkdir(mode=0o700,parents=True,exist_ok=True)
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
    def run(self, prepare_only=False, decisions_path=None):
        c=self.c
        require(c.get('fixture_authorized') is True, 'fixture_authority_required')
        require(digest(OPERATOR) == c['operator_sha256'], 'operator_drift')
        receipt=self.out/'product-receipt.json'
        require(not receipt.exists(), 'completed_run_exists')
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
        standard={name:digest(self.out/'oos'/name) for name in oos_names}
        require(load(self.out/'oos'/'source.json').get('actual_core_effective_sha256')==digest(self.out/'effective.json'),'oos_source_binding')
        s5_proof={'outcome':s5['outcome'],'builder_spawned':s5['builder_spawned'],'structural_draft':s5['structural_draft'],'cache_hit':s5['cache_hit'],'llm_calls':s5['llm_calls'],'model':identity['model'],'input_sha256':generation['input_sha256'],'evidence_hash':generation['evidence_hash']}
        save(receipt,{'protocol':'oo.source-onboarding.product/v1','build_id':build,'stages':list(stages.values()),'pack':pack,'pack_artifact_sha256':digest(self.out/'pack.json'),'effective_oos_sha256':digest(self.out/'effective.json'),'native_role_pack_sha256':digest(self.out/'role-pack.json'),'native_artifact_revision':native['artifact_revision'],'overlay_sha256':digest(self.out/'overlay.json'),'catalog_sha256':c['catalog_sha256'],'source_fresh':True,'s5':s5_proof,'standard_oos':standard,'producer':'tools/source-onboarding/product_flow.py','producer_sha256':digest(Path(__file__).resolve()),'fixture_review':True,'business_owner_signed':False,'published':False,'gateway_dialogue_complete':False})
        return {'build_id':build,'s0_s10':'succeeded','fixture_review':True,'published':False}

def main():
    parser=argparse.ArgumentParser();parser.add_argument('--config',required=True);parser.add_argument('--out',required=True);parser.add_argument('--prepare-only',action='store_true');parser.add_argument('--decisions');a=parser.parse_args()
    try:
        p=private(a.config);c=load(p);c['_sha256']=digest(p)
        print(json.dumps(Flow(c,Path(a.out)).run(a.prepare_only,a.decisions)));return 0
    except (Refused,OSError,KeyError,ValueError,subprocess.TimeoutExpired):
        print('source-onboarding-product: refused; private evidence retained',file=sys.stderr);return 2

if __name__=='__main__':sys.exit(main())
