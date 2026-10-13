from pathlib import Path
import copy,importlib.util,json,tempfile,sys
sys.dont_write_bytecode=True
r=Path.cwd();spec=importlib.util.spec_from_file_location('loop_render',r/'execution/scripts/render_todo.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
base=json.loads(m.LEDGER.read_text());m.render();original=(r/'execution/PRODUCT-LOOPS.md').read_text()
with tempfile.TemporaryDirectory(prefix='oo-product-loop-') as td:
 tmp=Path(td);m.LEDGER=tmp/'ledger.json';doc=tmp/'loops.md'
 def check(label,data,checked=()):
  content=original
  for task in checked:content=content.replace('- [ ] '+task+'：','- [x] '+task+'：',1)
  doc.write_text(content);data['rules']['product_loop_registry']=str(doc);m.LEDGER.write_text(json.dumps(data))
  try:m.render()
  except ValueError as e:print('REJECTED:',label,':',e)
  else:raise SystemExit('FAIL: '+label)
 a=copy.deepcopy(base);a['current_delivery_loop']='DL02';check('advance without DL01 acceptance',a)
 a=copy.deepcopy(base);a['delivery_loops'][1]['status']='accepted';check('accept DL02 before DL01',a,('DL02',))
 a=copy.deepcopy(base);a['items'][0]['final_delivery_loop']=None;check('required item loses delivery mapping',a)
 a=copy.deepcopy(base);a['items'][0]['required_for_current_goal']=False;check('mandatory SP silently deferred',a)
 a=copy.deepcopy(base);a['items'][-1]['required_for_current_goal']=False;check('DBX silently deferred',a)
 a=copy.deepcopy(base);a['phases'][14]['required_for_current_goal']=False;check('mandatory Phase silently deferred',a)
 valid=copy.deepcopy(base)
 for loop in valid['delivery_loops']:
  loop.update(status='accepted',source_commit='unit-test-only',artifact_digest='unit-test-only',evidence=['unit-test-only'],independent_review='unit-test-only',acceptance_record='unit-test-only')
 for item in valid['items']:
  if item['required_for_current_goal']:item.update(status='accepted',source_commit='unit-test-only',evidence=['unit-test-only'],independent_review='unit-test-only')
 for phase in valid['phases']:
  if phase['required_for_current_goal']:phase.update(formal_status='accepted',acceptance_record='unit-test-only')
 valid['delivery_loops'][0]['source_acceptance']=[{'source_product_id':product,'status':'accepted','version':'unit-test-only','deployment':'unit-test-only','driver':'unit-test-only','evidence':['unit-test-only'],'independent_review':'unit-test-only'} for product in valid['delivery_loops'][0]['required_source_product_ids']]
 all_checked=tuple(f'DL{i:02}' for i in range(1,6))
 a=copy.deepcopy(valid);a['delivery_loops'][0]['source_acceptance']=a['delivery_loops'][0]['source_acceptance'][:-1];check('DL01 with only nine accepted source products',a,all_checked)
 a=copy.deepcopy(valid);a['delivery_loops'][0]['source_acceptance']=[t for t in a['delivery_loops'][0]['source_acceptance'] if t['source_product_id']!='oracle'];check('DL01 missing Oracle',a,all_checked)
 a=copy.deepcopy(valid);next(i for i in a['items'] if i['id']=='SP14-W06')['status']='needs_acceptance_reconciliation';check('DL05 while required Plan item pending',a,all_checked)
 a=copy.deepcopy(valid);next(i for i in a['phases'] if i['id']=='SP14')['formal_status']='needs_acceptance_reconciliation';check('DL05 while required Phase pending',a,all_checked)
print('PASS: 10 unsafe delivery/Plan transitions rejected on private copies; real acceptance states unchanged')
