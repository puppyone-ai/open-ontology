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
    def run(self, prepare_only=False, decisions_path=None, reuse_product=None, reuse_receipt=None):
        c=self.c
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
        catalog=root/'bundle'/'catalog.json'
        manifest_path=root/'bundle'/'manifest.json'
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
        self.call('admin', path+'/maintenance', {'catalog_sha256':catalog_sha,'trigger':'source_generation_advanced'}, 'current-maintenance')
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
        put('current-prepared-overlay.json', overlay_bytes)
        save(self.out/'maintenance-consumption.json', fresh)

def main():
    parser=argparse.ArgumentParser();parser.add_argument('--config',required=True);parser.add_argument('--out',required=True);parser.add_argument('--prepare-only',action='store_true');parser.add_argument('--decisions');parser.add_argument('--reuse-product');parser.add_argument('--reuse-receipt');a=parser.parse_args()
    try:
        p=private(a.config);c=load(p);c['_sha256']=digest(p)
        print(json.dumps(Flow(c,Path(a.out)).run(a.prepare_only,a.decisions,a.reuse_product,a.reuse_receipt)));return 0
    except (Refused,OSError,KeyError,ValueError,subprocess.TimeoutExpired):
        print('source-onboarding-product: refused; private evidence retained',file=sys.stderr);return 2

if __name__=='__main__':sys.exit(main())
