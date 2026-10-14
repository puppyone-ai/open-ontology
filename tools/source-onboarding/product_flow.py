#!/usr/bin/env python3
"""Shared real fixture S0–S10/Pack caller; identity remains the actual OIDC service."""
import argparse
import json
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
    def run(self):
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
        # Explicit source-owner decisions, authenticated with actual fixture roles.
        for decision in c['reviews']:
            require(set(decision) == {'name','binds_fact','decision'}, 'review_shape')
            self.call('reviewer',path+'/review',decision,'review')
        confirmed=self.call('owner',path+'/confirm',{'columns':c['confirm_columns']},'confirm')
        require(confirmed.get('business_owner_signed') is False and confirmed.get('signer_class') == 'control_plane_test','business_signature_not_fixture')
        pack=self.call('admin',path+'/role-pack',{'policy_version':'oos/v1','role':c['role']},'role-pack')
        fetched=self.call('admin',path+'/role-pack?role='+c['role'],name='pack')
        final=self.call('admin',path,name='final-stages')
        stages={x['stage']:x for x in final['stages']}
        require(all(stages[f'S{i}']['status'] == 'succeeded' for i in range(11)), 'incomplete_s0_s10')
        save(self.out/'pack.json',fetched)
        overlay=self.call('admin',path+'/overlay',name='overlay');save(self.out/'overlay.json',overlay)
        save(receipt,{'protocol':'oo.source-onboarding.product/v1','build_id':build,'stages':list(stages.values()),'pack':pack,'pack_artifact_sha256':digest(self.out/'pack.json'),'overlay_sha256':digest(self.out/'overlay.json'),'fixture_review':True,'business_owner_signed':False,'published':False,'gateway_dialogue_complete':False})
        return {'build_id':build,'s0_s10':'succeeded','fixture_review':True,'published':False}

def main():
    parser=argparse.ArgumentParser();parser.add_argument('--config',required=True);parser.add_argument('--out',required=True);a=parser.parse_args()
    try:
        p=private(a.config);c=load(p);c['_sha256']=digest(p)
        print(json.dumps(Flow(c,Path(a.out)).run()));return 0
    except (Refused,OSError,KeyError,ValueError,subprocess.TimeoutExpired):
        print('source-onboarding-product: refused; private evidence retained',file=sys.stderr);return 2

if __name__=='__main__':sys.exit(main())
