#!/usr/bin/env python3
"""Adapt actual local catalog/profile to native suggestion input; no approvals."""
import json,pathlib,subprocess,hashlib
root=pathlib.Path(__file__).resolve().parents[2];out=root/'evidence/gpt-minimal-closure/stages'
def read(name):
 r=subprocess.run(['docker','exec','oo-a419-product-run','cat','/var/lib/oo/work/'+name+'.json'],capture_output=True,check=True);return json.loads(r.stdout)
catalog=read('catalog');p=read('profile')
profile={'schema':'oo.selfbuild-suggestions.profile/v1','sample_policy':'none','tables':[{'schema':t['schema'],'name':t['name'],'reltuples':t['reltuples'],'columns':[{'name':c['name']} for c in t['columns']]} for t in p['tables']]}
relationships={'schema':'oo.selfbuild-suggestions.relationships/v1','items':[]}
feedback={'schema':'oo.selfbuild-suggestions.feedback/v1','items':[]}
def sha(v):return hashlib.sha256(json.dumps(v,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode()).hexdigest()
d={'schema':'oo.selfbuild-suggestions.command/v1','scope':{'project':'logistics','role':'oo_r_devread','evidence_sha256':sha(catalog),'source_id':'logistics','schema_version':'oos/v1','model_scope':'none'},'authorization':{'authorized':True,'catalog_sha256':sha(catalog),'relationships_sha256':sha(relationships),'profile_sha256':sha(profile),'feedback_sha256':sha(feedback)},'catalog':catalog,'relationships':relationships,'profile':profile,'feedback':feedback,'negative_pairs':[],'curated_overrides':[],'prior_candidate_ids':[]}
body=json.dumps(d,ensure_ascii=False).encode();(out/'suggestions-input.json').write_bytes(body)
r=subprocess.run(['docker','exec','-i','oo-a419-product-run','/target/loop/debug/selfbuild-suggestions','produce'],input=body,capture_output=True)
for suffix,value in [('stdout',r.stdout),('stderr',r.stderr),('exit',(str(r.returncode)+'\n').encode())]:(out/f'suggestions.{suffix}').write_bytes(value)
print('suggestions',r.returncode);raise SystemExit(r.returncode)
