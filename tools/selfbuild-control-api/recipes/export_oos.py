#!/usr/bin/env python3
"""Serialize the actual compiled OOS object; publication and role approval stay separate."""
import argparse,json,pathlib,hashlib

def yaml(value,level=0):
    indent='  '*level
    if isinstance(value,dict):
        return '\n'.join(indent+json.dumps(str(k))+':'+ (' '+json.dumps(v,ensure_ascii=False) if not isinstance(v,(dict,list)) else '\n'+yaml(v,level+1)) for k,v in value.items()) if value else indent+'{}'
    if isinstance(value,list):
        return '\n'.join(indent+'-'+ (' '+json.dumps(v,ensure_ascii=False) if not isinstance(v,(dict,list)) else '\n'+yaml(v,level+1)) for v in value) if value else indent+'[]'
    return indent+json.dumps(value,ensure_ascii=False)

p=argparse.ArgumentParser();p.add_argument('--effective',required=True);p.add_argument('--out',required=True);a=p.parse_args()
source=pathlib.Path(a.effective);effective=json.loads(source.read_text())
if effective.get('protocol')!='oo.builder.effective-oos/v1':raise ValueError('actual core compiled effective OOS required')
out=pathlib.Path(a.out);out.mkdir(parents=True,exist_ok=False);(out/'datasets').mkdir()
(out/'ontology.yaml').write_text(yaml(effective['ontology'])+'\n')
for dataset in effective['datasets']:
    name=dataset['dataset']
    if '/' in name or '\\' in name or '..' in name:raise ValueError('invalid dataset name')
    (out/'datasets'/f'{name}.yaml').write_text(yaml(dataset)+'\n')
if effective.get('relationships'):
    (out/'relationships.yaml').write_text(yaml({'schema_version':'oos/v1','relationships':effective['relationships']})+'\n')
(out/'effective-oos.yaml').write_text(yaml(effective)+'\n')
(out/'source.json').write_text(json.dumps({'actual_core_effective_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'source_path':str(source.resolve()),'publication':'not_claimed','role_approval':'not_claimed'},indent=2)+'\n')
print('serialized_actual_oos_datasets',len(effective['datasets']))
