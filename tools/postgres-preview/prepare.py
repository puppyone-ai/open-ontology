import pathlib,subprocess,json,hashlib,uuid
R=pathlib.Path('/finish/continuous')
def read(p):return json.loads(p.read_bytes())
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def cmd(args):
 p=subprocess.run(args,capture_output=True)
 if p.returncode:raise RuntimeError((args,p.returncode,p.stderr.decode(),p.stdout[:1000].decode()))
 return p.stdout

def prepare(root,config,baseline=False):
 # Observations are actual source catalog privileges, collected by the fixture DBA.
 cmd(['python3',str(R/'admission_inputs.py'),'--output-root',str(root),'--visible-root',str(root),'--config',str(config),'--permissions',str(R/'permissions.jsonl'),'--driver',read(config)['bins']['preview'],'--visible-driver',read(config)['bins']['preview']]+([] if baseline else ['--transaction-only']))
 entries=[];vector=[]
 captures=sorted((root/'native-admit').glob('*/*.capture.json'))
 latest=max((int(c['applied_pos']) for c in (read(q.with_name(q.name.replace('.capture.','.request.'))) for q in captures) if int(c['applied_pos'])>0),default=0)
 for item in sorted((root/'native-admit').glob('*/*.capture.json')):
  cap=read(item)
  if baseline and cap['source_generation']!=1:continue
  if not baseline and 'transaction_scope' not in cap:continue
  reqpath=item.with_name(item.name.replace('.capture.','.request.'));req=read(reqpath)
  if not baseline and int(req['applied_pos'])!=latest:continue
  generation=cap['source_generation'];table=cap['logical_schema']+'.'+cap['logical_table']
  seal=root/'sealed'/table/str(generation);seal.parent.mkdir(parents=True,exist_ok=True)
  inp=root/'admission-inputs'/table/str(generation)
  if not seal.exists():
   out=cmd([str(R/'oo-source-admission'),'seal-native','--report',str(inp/'report.json'),'--config',str(inp/'config.json'),'--out',str(seal)])
   (seal.parent/(str(generation)+'.seal.stdout')).write_bytes(out)
  reg=read(seal/'registry.json');b=reg['bindings'][0];assert read(seal/'decision.json')['admitted']
  fields={f['name']:f for f in b['fields']}
  for col in req['columns']:
   for k in ['source_oid','source_typmod','source_format']:col[k]=fields[col['name']][k]
  for k in ['schema_digest','type_digest','config_digest','run_id']:req[k]=b[k]
  req['audit_digest']=b['capture_receipt_sha256'];req['admission_path']=b['admission_receipt_path'];req['admission_expect']={'declared_receipt_sha256':b['admission_receipt_sha256']};req['max_arrow_bytes']=16*1024*1024
  entries.append(req);vector.append({'logical_schema':cap['logical_schema'],'logical_table':cap['logical_table'],'registry_path':str(seal/'registry.json'),'registry_sha256':sha(seal/'registry.json')})
 entries.sort(key=lambda e:(e['logical_schema'],e['logical_table']));vector.sort(key=lambda e:(e['logical_schema'],e['logical_table']))
 return entries,{'protocol':'oo.pipeline.native-transaction-registry-vector/v1','entries':vector}
if __name__=='__main__':
 import sys
 entries,vector=prepare(R/'capture',R/'source-config.json',True)
 binding='ade92eb8-bfd2-4b75-a570-b7b2171ca646'
 body={'protocol':'oo.jobs.native-baseline-producer/v1','binding':binding,'pin':1,'source_id':'oo_pg46_finish','capture_method':'c1_cdc','source_generation':1,'entries':entries}
 (R/'baseline.json').write_text(json.dumps(body,separators=(',',':'))+'\n');(R/'baseline-vector.json').write_text(json.dumps(vector,separators=(',',':'))+'\n')
 print('prepared baseline',len(entries))
