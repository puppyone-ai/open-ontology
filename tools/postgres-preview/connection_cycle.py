#!/usr/bin/env python3
"""Owned PG fixture cycle through pinned native producer, Loader, CAS and ACK."""
import json, os, subprocess, time, hashlib
from pathlib import Path
BASE=Path('/tmp/oo-pg46-runtime-finish'); R=BASE/'continuous'
B='b942d94a-1d31-40c6-8072-a987912af946'; PROJECT='00000000-0000-4000-8000-000000000046'
WORKER=['docker','exec','oo-pg46-runtime-finish','/finish/job-worker-successor','--config','/finish/continuous/worker.conf']
def save(p,x):
 p.parent.mkdir(mode=0o700,parents=True,exist_ok=True);p.write_text(json.dumps(x,indent=2)+'\n');p.chmod(0o600)
def read(p):return json.loads(p.read_text())
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def command(out,name,argv,data=None):
 x=subprocess.run(argv,input=data,capture_output=True,timeout=180)
 for ext,v in [('stdout',x.stdout),('stderr',x.stderr)]:
  p=out/(name+'.'+ext);p.write_bytes(v);p.chmod(0o600)
 save(out/(name+'.exit.json'),{'exit':x.returncode})
 if x.returncode:raise RuntimeError('actual command failed: '+name)
 return x.stdout

def query(db,q,source=False):
 return subprocess.check_output(['docker','exec','oo-dl01w1-46-src' if source else 'oo-dl01w1-46-native','psql','-U','postgres','-d',db,'-X','-At','-v','ON_ERROR_STOP=1','-c',q],stderr=subprocess.PIPE,timeout=30).decode().strip()

def cycle(out,label):
 out.mkdir(mode=0o700,parents=True,exist_ok=True)
 if (out/'receipt.json').exists():return read(out/'receipt.json')
 started=out/'started.json'
 if started.exists():s=read(started)
 else:
  if query('pg46_finish_target_v2',"SELECT count(*) FROM jobs.tasks WHERE state IN ('queued','running')")!='0':raise RuntimeError('foreign or pending native task')
  p=json.loads(query('pg46_finish_target_v2',f"SELECT row_to_json(s)::text FROM jw_native.watermarks s WHERE binding_id='{B}'"))
  s={'generation':p['source_generation']+1,'pin':p['pin']+1,'label':label};save(started,s)
 ident=70000000000+s['generation'];mark=f'pg46-onboard-{s["generation"]}'
 # Presence of the unique insertion makes uncertain DML delivery recoverable without a second source transaction.
 present=query('pg46_finish_source',f'SELECT count(*) FROM public.shipments WHERE shipment_id={ident}',True)
 if present=='0':
  if (out/'dml-started.json').exists():raise RuntimeError('uncertain source transaction; retained')
  sql=f"BEGIN; UPDATE public.hubs SET hub_name='{mark}' WHERE hub_id=(SELECT min(hub_id) FROM public.hubs); DELETE FROM customer_east.delivery_notes WHERE note_id=(SELECT min(note_id) FROM customer_east.delivery_notes); INSERT INTO public.shipments SELECT (jsonb_populate_record(NULL::public.shipments,to_jsonb(s)||jsonb_build_object('shipment_id',{ident},'shipment_token','30000000-0000-4000-8000-{s['generation']:012d}'))).* FROM public.shipments s ORDER BY shipment_id LIMIT 1; INSERT INTO public.tracking_events SELECT (jsonb_populate_record(NULL::public.tracking_events,to_jsonb(t)||jsonb_build_object('event_id',{ident},'event_token','40000000-0000-4000-8000-{s['generation']:012d}'))).* FROM public.tracking_events t ORDER BY event_id LIMIT 1; COMMIT;"
  save(out/'dml-started.json',{'generation':s['generation'],'owned_fixture':True})
  command(out,'source-transaction',['docker','exec','-i','oo-dl01w1-46-src','psql','-U','postgres','-d','pg46_finish_source','-v','ON_ERROR_STOP=1'],sql.encode())
 command(out,'enable',WORKER+['native-enable','--binding',B,'--enabled','true'])
 worker='pg46-onboard-'+str(s['generation'])
 so=(out/'serve.stdout').open('ab');se=(out/'serve.stderr').open('ab');os.chmod(so.name,0o600);os.chmod(se.name,0o600)
 process=subprocess.Popen(WORKER+['native-serve','--project',PROJECT,'--worker',worker,'--seconds','120'],stdout=so,stderr=se)
 deadline=time.monotonic()+100
 try:
  while time.monotonic()<deadline:
   state=query('pg46_finish_target_v2',f"SELECT t.state FROM jobs.tasks t JOIN jw_native.occurrences o ON o.task_id=t.id WHERE o.binding_id='{B}' AND o.pin={s['pin']}")
   if state=='succeeded':break
   if process.poll() is not None:raise RuntimeError('native service exited')
   time.sleep(.2)
  else:raise RuntimeError('native cycle deadline')
 finally:
  command(out,'pause',WORKER+['native-enable','--binding',B,'--enabled','false'])
  live=query('pg46_finish_target_v2',"SELECT count(*) FROM jobs.tasks WHERE state='running'")
  if live=='0':
   ps=subprocess.check_output(['docker','exec','oo-pg46-runtime-finish','ps','-eo','pid,args'],text=True)
   ids=[line.strip().split()[0] for line in ps.splitlines() if '/finish/job-worker-successor ' in line and '--worker '+worker+' ' in line]
   if len(ids)==1:subprocess.run(['docker','exec','oo-pg46-runtime-finish','kill','-TERM',ids[0]],check=True)
  process.wait(timeout=15);so.close();se.close()
 root=R/'runtime'/B/str(s['pin']);body=read(root/'producer.json');assert body['source_generation']==s['generation']
 pos=int(body['entries'][0]['applied_pos']);g=query('pg46_finish_target_v2',f"SELECT source_generation||'|'||applied_pos FROM jw_native.watermarks WHERE binding_id='{B}'")
 assert g==f"{s['generation']}|{pos}"
 assert query('pg46_finish_target_v2',f"SELECT count(*) FROM _oo.table_freshness WHERE source='oo_pg46_finish' AND source_generation={s['generation']} AND last_pos={pos}")=='4'
 lsn=body['transaction_scope']['end_lsn'];assert body['transaction_scope']['slot']=='pg46_finish_slot'
 command(out,'slot-ack',['docker','exec','-i','oo-dl01w1-46-src','psql','-U','postgres','-d','pg46_finish_source','-v','ON_ERROR_STOP=1'],f"SET ROLE oo_dl01w1_46_cdc; SELECT pg_replication_slot_advance('pg46_finish_slot','{lsn}');".encode())
 entries=[];vector=read(root/'vector.json')
 for e in body['entries']:
  run=json.loads(query('pg46_finish_target_v2',f"SELECT row_to_json(r)::text FROM _oo.full_import_run r WHERE run_id='{e['run_id']}'"))
  req={k:e[k] for k in ('run_id','source_id','source_generation','config_digest','schema_digest','type_digest','input_digest','snapshot','target')};req.update(protocol='oo.pipeline.native-sealed-catalog/v1',owner=run['owner_token'],target_generation=run['target_generation'])
  p=out/(e['logical_schema']+'.'+e['logical_table']+'.catalog-request.json');save(p,req)
  v=next(v for v in vector['entries'] if v['logical_schema']==e['logical_schema'] and v['logical_table']==e['logical_table'])
  entries.append({'request':str(p).replace(str(BASE),'/finish'),'registry':v['registry_path'],'registry_sha256':v['registry_sha256']})
 op={'protocol':'oo.pipeline.native-collection-operator/v1','pipeline':{'path':'/finish/oo-pipeline','sha256':sha(BASE/'oo-pipeline')},'admission_binary':'/finish/continuous/oo-source-admission','entries':entries,'expected_datasets':[[e['logical_schema'],e['logical_table']] for e in body['entries']]};save(out/'collection-operator.json',op)
 command(out,'catalog-export',['docker','exec','oo-pg46-runtime-finish','python3','/finish/continuous/export-native-collection.py','--operator',str(out/'collection-operator.json').replace(str(BASE),'/finish'),'--out',str(out/'catalog.json').replace(str(BASE),'/finish')])
 command(out,'full-cell-oracle',['/usr/bin/python3',str(R/'independent_diff.py'),'--config',str(R/'source-config.json'),'--out',str(out/'typed-diff.json')])
 result={**s,'source_id':'oo_pg46_finish','end_lsn':lsn,'applied_pos':pos,'catalog_sha256':sha(out/'catalog.json'),'typed_diff_sha256':sha(out/'typed-diff.json'),'automatic_ack':True,'source_transaction_new':True};save(out/'receipt.json',result);return result
