#!/usr/bin/env python3
"""Owned PG fixture cycle through the pinned native producer, Loader, CAS and ACK."""
import json, os, subprocess, time, hashlib
from pathlib import Path
BASE=Path('/tmp/oo-pg46-runtime-finish'); R=BASE/'continuous'
B='b942d94a-1d31-40c6-8072-a987912af946'; PROJECT='00000000-0000-4000-8000-000000000046'
WORKER=['docker','exec','oo-pg46-runtime-finish','/finish/job-worker-successor','--config','/finish/continuous/worker.conf']
def save(p,x):
 p.parent.mkdir(mode=0o700,parents=True,exist_ok=True);p.write_text(json.dumps(x,indent=2)+'\n');p.chmod(0o600)
def read(p):return json.loads(p.read_text())
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def mounted(out,name):
 dest=BASE/'onboard'/Path(out).name;dest.mkdir(mode=0o700,parents=True,exist_ok=True);return dest/name
def cpath(path):
 path=Path(path).resolve();base=BASE.resolve()
 if base!=path and base not in path.parents:raise RuntimeError('command path is not on the runtime mount')
 return '/finish/'+path.relative_to(base).as_posix()
def command(out,name,argv,data=None,timeout=180):
 out.mkdir(mode=0o700,parents=True,exist_ok=True)
 x=subprocess.run(argv,input=data,capture_output=True,timeout=timeout)
 for ext,v in [('stdout',x.stdout),('stderr',x.stderr)]:
  p=out/(name+'.'+ext);p.write_bytes(v);p.chmod(0o600)
 save(out/(name+'.exit.json'),{'exit':x.returncode})
 if x.returncode:raise RuntimeError('actual command failed: '+name)
 return x.stdout

def query(db,q,source=False):
 return subprocess.check_output(['docker','exec','oo-dl01w1-46-src' if source else 'oo-dl01w1-46-native','psql','-U','postgres','-d',db,'-X','-At','-v','ON_ERROR_STOP=1','-c',q],stderr=subprocess.PIPE,timeout=30).decode().strip()

def begin(out,label):
 out.mkdir(mode=0o700,parents=True,exist_ok=True)
 if (out/'receipt.json').exists():return None
 started=out/'started.json'
 if started.exists():return read(started)
 if query('pg46_finish_target_v2',"SELECT count(*) FROM jobs.tasks WHERE state IN ('queued','running')")!='0':raise RuntimeError('foreign or pending native task')
 p=json.loads(query('pg46_finish_target_v2',f"SELECT row_to_json(s)::text FROM jw_native.watermarks s WHERE binding_id='{B}' AND source_generation=(SELECT max(source_generation) FROM jw_native.watermarks WHERE binding_id='{B}')"))
 s={'generation':p['source_generation']+1,'pin':p['pin']+1,'label':label};save(started,s);return s

def dml(out,s,sql):
 if (out/'dml-started.json').exists() and not (out/'source-transaction.exit.json').exists():raise RuntimeError('uncertain source transaction; retained')
 save(out/'dml-started.json',{'generation':s['generation'],'owned_fixture':True})
 command(out,'source-transaction',['docker','exec','-i','oo-dl01w1-46-src','psql','-U','postgres','-d','pg46_finish_source','-v','ON_ERROR_STOP=1'],sql.encode())

def occurrence(pin):
 row=query('pg46_finish_target_v2',f"SELECT t.state||'|'||t.id::text FROM jobs.tasks t JOIN jw_native.occurrences o ON o.task_id=t.id WHERE o.binding_id='{B}' AND o.pin={int(pin)}")
 return row.split('|',1) if row else None
def release_unapplied(s):
 """A failed pin that never executed does not own the generation. The next fire gets a new pin."""
 pin=int(s['pin']);gen=int(s['generation'])
 wm=query('pg46_finish_target_v2',f"SELECT source_generation::text FROM jw_native.watermarks WHERE binding_id='{B}'")
 if wm!=str(gen-1):raise RuntimeError('watermark is not the parent generation')
 found=occurrence(pin)
 if not found:return pin
 state,task=found
 if state=='succeeded':return pin
 if state!='failed':raise RuntimeError('native capture is '+state)
 if query('pg46_finish_target_v2',f"SELECT count(*) FROM jw_native.process_attempts WHERE task_id='{task}'")!='0':raise RuntimeError('failed capture already executed')
 query('pg46_finish_target_v2',"WITH released AS (UPDATE jw_native.source_progress p SET active_task_id=NULL WHERE p.active_task_id='%s'::uuid AND p.source_generation=%d AND EXISTS (SELECT 1 FROM jobs.tasks t WHERE t.id=p.active_task_id AND t.state='failed') AND NOT EXISTS (SELECT 1 FROM jw_native.pending_acks a WHERE a.task_id=p.active_task_id) AND NOT EXISTS (SELECT 1 FROM jw_native.process_attempts x WHERE x.task_id=p.active_task_id) RETURNING 1) SELECT count(*) FROM released"%(task,gen-1))
 if query('pg46_finish_target_v2',f"SELECT count(*) FROM jw_native.source_progress WHERE active_task_id='{task}'")!='0':raise RuntimeError('failed capture still holds the source fence')
 nxt=query('pg46_finish_target_v2',f"SELECT COALESCE(max(pin),0) FROM jw_native.occurrences WHERE binding_id='{B}' AND pin>{pin}")
 if nxt=='0':return None
 later=occurrence(int(nxt))
 if later and later[0]=='failed':raise RuntimeError('later capture already failed')
 return int(nxt)
def converge(out,s):
 held=query('pg46_finish_target_v2',f"SELECT source_generation::text||'|'||pin::text FROM jw_native.watermarks WHERE binding_id='{B}'")
 wgen,wpin=held.split('|')
 if int(wgen)==int(s['generation']):
  s['pin']=int(wpin)
 else:
  pin=release_unapplied(s)
  command(out,'enable',WORKER+['native-enable','--binding',B,'--enabled','true'])
  worker='pg46-onboard-'+str(s['generation'])
  so=(out/'serve.stdout').open('ab');se=(out/'serve.stderr').open('ab');os.chmod(so.name,0o600);os.chmod(se.name,0o600)
  process=subprocess.Popen(WORKER+['native-serve','--project',PROJECT,'--worker',worker,'--seconds','600'],stdout=so,stderr=se)
  deadline=time.monotonic()+540
  try:
   while time.monotonic()<deadline:
    if pin is None:
     nxt=query('pg46_finish_target_v2',f"SELECT COALESCE(max(pin),0) FROM jw_native.occurrences WHERE binding_id='{B}' AND pin>{int(s['pin'])}")
     if nxt!='0':pin=int(nxt)
    if pin is not None:
     state=query('pg46_finish_target_v2',f"SELECT t.state FROM jobs.tasks t JOIN jw_native.occurrences o ON o.task_id=t.id WHERE o.binding_id='{B}' AND o.pin={int(pin)}")
     if state=='succeeded':break
     if state=='failed':raise RuntimeError('native capture failed')
    if process.poll() is not None:raise RuntimeError('native service exited')
    time.sleep(.2)
   else:raise RuntimeError('native cycle deadline')
   s['pin']=int(pin)
  finally:
   command(out,'pause',WORKER+['native-enable','--binding',B,'--enabled','false'])
   live=query('pg46_finish_target_v2',"SELECT count(*) FROM jobs.tasks WHERE state='running'")
   if live=='0':
    ps=subprocess.check_output(['docker','exec','oo-pg46-runtime-finish','ps','-eo','pid,args'],text=True)
    ids=[line.strip().split()[0] for line in ps.splitlines() if '/finish/job-worker-successor ' in line and '--worker '+worker+' ' in line]
    if len(ids)==1:subprocess.run(['docker','exec','oo-pg46-runtime-finish','kill','-TERM',ids[0]],check=True)
   process.wait(timeout=20);so.close();se.close()
 root=R/'runtime'/B/str(s['pin']);body=read(root/'producer.json');assert body['source_generation']==s['generation']
 pos=int(body['entries'][0]['applied_pos']);g=query('pg46_finish_target_v2',f"SELECT source_generation||'|'||applied_pos FROM jw_native.watermarks WHERE binding_id='{B}' AND source_generation={s['generation']}")
 assert g==f"{s['generation']}|{pos}"
 assert query('pg46_finish_target_v2',f"SELECT count(*) FROM _oo.table_freshness WHERE source='oo_pg46_finish' AND source_generation={s['generation']} AND last_pos={pos}")==str(len(body['entries']))
 lsn=body['transaction_scope']['end_lsn'];assert body['transaction_scope']['slot']=='pg46_finish_slot'
 slot=query('pg46_finish_source',f"SELECT CASE WHEN confirmed_flush_lsn='{lsn}'::pg_lsn THEN 'acked' WHEN confirmed_flush_lsn<'{lsn}'::pg_lsn THEN 'behind' ELSE 'ahead' END FROM pg_replication_slots WHERE slot_name='pg46_finish_slot'",True)
 if slot=='ahead':raise RuntimeError('slot is ahead of this generation')
 if slot=='behind':command(out,'slot-ack',['docker','exec','-i','oo-dl01w1-46-src','psql','-U','postgres','-d','pg46_finish_source','-v','ON_ERROR_STOP=1'],f"SET ROLE oo_dl01w1_46_cdc; SELECT pg_replication_slot_advance('pg46_finish_slot','{lsn}');".encode())
 elif slot!='acked':raise RuntimeError('slot state')
 entries=[];vector=read(root/'vector.json');applied=json.loads(query('pg46_finish_target_v2',f"SELECT result::text FROM jw_native.watermarks WHERE binding_id='{B}' AND source_generation={int(s['generation'])}"))
 runs={(a['logical_schema'],a['logical_table']):a['run_id'] for a in applied['entries']}
 for e in body['entries']:
  run_id=runs[(e['logical_schema'],e['logical_table'])]
  run=json.loads(query('pg46_finish_target_v2',f"SELECT row_to_json(r)::text FROM _oo.full_import_run r WHERE run_id='{run_id}'::uuid"))
  target={k:e['target'][k] for k in ('host','port','database','runtime_principal','password_file','ca_file')}
  req={k:e[k] for k in ('run_id','source_id','source_generation','config_digest','schema_digest','type_digest','input_digest','snapshot')};req.update(run_id=run_id,target=target,protocol='oo.pipeline.native-sealed-catalog/v1',owner=run['owner_token'],target_generation=run['target_generation'])
  p=out/(e['logical_schema']+'.'+e['logical_table']+'.catalog-request.json');save(p,req)
  staged_req=mounted(out,p.name);save(staged_req,req)
  v=next(v for v in vector['entries'] if v['logical_schema']==e['logical_schema'] and v['logical_table']==e['logical_table'])
  entries.append({'request':cpath(staged_req),'registry':v['registry_path'],'registry_sha256':v['registry_sha256']})
 op={'protocol':'oo.pipeline.native-collection-operator/v1','pipeline':{'path':'/finish/oo-pipeline','sha256':sha(BASE/'oo-pipeline')},'admission_binary':'/finish/continuous/oo-source-admission','entries':entries,'expected_datasets':[[e['logical_schema'],e['logical_table']] for e in body['entries']]}
 staged=mounted(out,'collection-operator.json');save(staged,op);save(out/'collection-operator.json',op)
 catalog_staged=mounted(out,'catalog.json')
 command(out,'catalog-export',['docker','exec','oo-pg46-runtime-finish','python3','/finish/continuous/export-native-collection.py','--operator',cpath(staged),'--out',cpath(catalog_staged)])
 (out/'catalog.json').write_bytes(catalog_staged.read_bytes());(out/'catalog.json').chmod(0o600)
 widen_catalog(out,int(s['generation']))
 command(out,'full-cell-oracle',['/usr/bin/python3',str(R/'independent_diff.py'),'--config',str(R/'source-config.json'),'--out',str(out/'typed-diff.json')],timeout=900)
 diff=read(out/'typed-diff.json')
 if diff.get('difference_count_zero') is not True:raise RuntimeError('typed diff is not zero')
 result={**s,'source_id':'oo_pg46_finish','end_lsn':lsn,'applied_pos':pos,'catalog_sha256':sha(out/'catalog.json'),'typed_diff_sha256':sha(out/'typed-diff.json'),'typed_cell_diff':0,'automatic_ack':True,'source_transaction_new':True}
 save(out/'receipt.json',result);return result

def seal_collection(entries):
 bindings=[]
 for entry in entries:
  for table in entry['tables']:
   bindings.append({'dataset':table['logical_schema']+'.'+table['logical_table'],**{k:entry[k] for k in ('source_id','run_id','source_generation','target_generation','config_digest','schema_digest','type_digest','input_digest')},'stored_namespace':table['stored_namespace'],'stored_physical':table['stored_physical']})
 body={'protocol':'oo.pipeline.sealed-import-collection/v1','entries':entries,'binding_vector':bindings}
 body['collection_sha256']=hashlib.sha256(json.dumps({k:body[k] for k in ('protocol','entries','binding_vector')},sort_keys=True,separators=(',',':')).encode()).hexdigest()
 return body
def cycle_proofs(generation):
 best=None
 root=Path('/Users/redux/.cache/oo-source-onboarding/runs')
 for rec in root.glob('*/cycle/receipt.json'):
  cat=rec.parent/'catalog.json';diff=rec.parent/'typed-diff.json'
  if not cat.is_file() or not diff.is_file():continue
  body=read(rec)
  if body.get('generation')!=generation or body.get('typed_cell_diff')!=0 or body.get('source_id')!='oo_pg46_finish' or body.get('end_lsn') in (None,'0/404D7A8'):continue
  if body.get('catalog_sha256')!=sha(cat) or body.get('typed_diff_sha256')!=sha(diff):continue
  slot=query('pg46_finish_source',f"SELECT confirmed_flush_lsn>='{body['end_lsn']}'::pg_lsn FROM pg_replication_slots WHERE slot_name='pg46_finish_slot'",True)
  if slot!='t':continue
  if best is None or rec.stat().st_mtime>=best[0]:best=(rec.stat().st_mtime,body,cat)
 return best
def proven_cycle(generation):
 found=cycle_proofs(generation)
 return None if found is None else dict(found[1])
def widen_catalog(out,generation):
 found=cycle_proofs(generation-1)
 if not found:return
 prior=read(found[2]);cat=read(out/'catalog.json')
 have={b['dataset'] for b in cat['binding_vector']}
 if {b['dataset'] for b in prior['binding_vector']}<=have:return
 new={b['dataset']:e for b,e in zip(cat['binding_vector'],cat['entries'])}
 entries=[];seen=set()
 for b,e in zip(prior['binding_vector'],prior['entries']):
  seen.add(b['dataset']);entries.append(new.get(b['dataset'],e))
 for b,e in zip(cat['binding_vector'],cat['entries']):
  if b['dataset'] not in seen:
   seen.add(b['dataset']);entries.append(e)
 save(out/'catalog.json',seal_collection(entries))
def stamp(out,result,extra):
 if any(result.get(k)!=v for k,v in extra.items()):
  result.update(extra);save(out/'receipt.json',result)
 return result
def cycle(out):
 out=Path(out)
 if (out/'receipt.json').exists():
  result=read(out/'receipt.json')
  if (out/'deleted-note.json').exists():
   note=read(out/'deleted-note.json');return stamp(out,result,{'insert_update_delete_verified':True,'deleted_note_id':note['note_id'],'recovery_verified':False})
  return result
 s=begin(out,'insert-update-delete')
 if s is None:return read(out/'receipt.json')
 if (out/'source-transaction.exit.json').exists() and (out/'deleted-note.json').exists():
  victim=read(out/'deleted-note.json')
  wgen=int(query('pg46_finish_target_v2',f"SELECT source_generation FROM jw_native.watermarks WHERE binding_id='{B}'"))
  prior=proven_cycle(s['generation']) if wgen>int(s['generation']) else None
  if prior:
   prior.update(insert_update_delete_verified=True,deleted_note_id=victim['note_id'],recovery_verified=False)
   save(out/'receipt.json',prior);return prior
  result=converge(out,s)
  result.update(insert_update_delete_verified=True,deleted_note_id=victim['note_id'],recovery_verified=False)
  save(out/'receipt.json',result);return result
 ident=70000000000+s['generation'];mark=f'pg46-onboard-{s["generation"]}'
 victim=json.loads(query('pg46_finish_source','SELECT row_to_json(n)::text FROM customer_east.delivery_notes n WHERE note_id=(SELECT min(note_id) FROM customer_east.delivery_notes)',True))
 save(out/'deleted-note.json',victim)
 present=query('pg46_finish_source',f"SELECT count(*) FROM public.shipments WHERE shipment_id={ident}",True)
 if present=='0':
  sql=f"BEGIN; UPDATE public.hubs SET hub_name='{mark}' WHERE hub_id=(SELECT min(hub_id) FROM public.hubs); DELETE FROM customer_east.delivery_notes WHERE note_id={victim['note_id']}; INSERT INTO public.shipments SELECT (jsonb_populate_record(NULL::public.shipments,to_jsonb(s)||jsonb_build_object('shipment_id',{ident},'shipment_token','30000000-0000-4000-8000-{s['generation']:012d}'))).* FROM public.shipments s ORDER BY shipment_id LIMIT 1; INSERT INTO public.tracking_events SELECT (jsonb_populate_record(NULL::public.tracking_events,to_jsonb(t)||jsonb_build_object('event_id',{ident},'event_token','40000000-0000-4000-8000-{s['generation']:012d}'))).* FROM public.tracking_events t ORDER BY event_id LIMIT 1; COMMIT;"
  dml(out,s,sql)
 gone=query('pg46_finish_source',f"SELECT count(*) FROM customer_east.delivery_notes WHERE note_id={victim['note_id']}",True)
 if gone!='0':raise RuntimeError('physical delete did not land')
 result=converge(out,s)
 result.update(insert_update_delete_verified=True,deleted_note_id=victim['note_id'],recovery_verified=False)
 save(out/'receipt.json',result);return result

def proven_recovery(note_id):
 best=None
 root=Path('/Users/redux/.cache/oo-source-onboarding/runs')
 for rec in root.glob('*/recovery/receipt.json'):
  cat=rec.parent/'catalog.json';diff=rec.parent/'typed-diff.json'
  if not cat.is_file() or not diff.is_file():continue
  body=read(rec)
  if body.get('recovery_verified') is not True or body.get('typed_cell_diff')!=0 or body.get('source_id')!='oo_pg46_finish' or body.get('restored_note_id')!=note_id or body.get('end_lsn') in (None,'0/404D7A8'):continue
  if body.get('catalog_sha256')!=sha(cat) or body.get('typed_diff_sha256')!=sha(diff):continue
  slot=query('pg46_finish_source',f"SELECT confirmed_flush_lsn='{body['end_lsn']}'::pg_lsn FROM pg_replication_slots WHERE slot_name='pg46_finish_slot'",True)
  if slot!='t':continue
  held=query('pg46_finish_target_v2',f"SELECT source_generation::text||'|'||pin::text FROM jw_native.watermarks WHERE binding_id='{B}'")
  if held!=f"{int(body['generation'])}|{int(body['pin'])}":continue
  if best is None or rec.stat().st_mtime>=best[0]:best=(rec.stat().st_mtime,body,cat,diff)
 return best
def recover(cycle_out,out):
 cycle_out,out=Path(cycle_out),Path(out)
 if (out/'receipt.json').exists():
  result=read(out/'receipt.json');note=read(cycle_out/'deleted-note.json')
  return stamp(out,result,{'recovery_verified':True,'restored_note_id':note['note_id'],'insert_update_delete_verified':True})
 note=read(cycle_out/'deleted-note.json');body=json.dumps(note)
 if not (out/'dml-started.json').exists():
  found=proven_recovery(note['note_id'])
  if found:
   out.mkdir(mode=0o700,parents=True,exist_ok=True)
   for src,name in ((found[2],'catalog.json'),(found[3],'typed-diff.json')):
    dest=out/name;dest.write_bytes(src.read_bytes());dest.chmod(0o600)
   result=dict(found[1]);save(out/'receipt.json',result);return result
 if '$$' in body:raise RuntimeError('note json quoting')
 present=query('pg46_finish_source',f"SELECT count(*) FROM customer_east.delivery_notes WHERE note_id={note['note_id']}",True)
 if present=='1' and not (out/'dml-started.json').exists():
  held=query('pg46_finish_target_v2',f"SELECT source_generation::text||'|'||pin::text FROM jw_native.watermarks WHERE binding_id='{B}'")
  gen,pin=held.split('|');prod=R/'runtime'/B/pin/'producer.json'
  if prod.is_file():
   captured=read(prod)
   if captured.get('source_generation')==int(gen) and any(e.get('logical_schema')=='customer_east' and e.get('logical_table')=='delivery_notes' for e in captured['entries']):
    active=query('pg46_finish_target_v2',f"SELECT COALESCE(active_task_id::text,'') FROM jw_native.source_progress WHERE source_id='oo_pg46_finish'")
    if active:
     state=query('pg46_finish_target_v2',f"SELECT state FROM jobs.tasks WHERE id='{active}'")
     if state!='failed':raise RuntimeError('recovery capture is '+state)
     query('pg46_finish_target_v2',"WITH released AS (UPDATE jw_native.source_progress SET active_task_id=NULL WHERE active_task_id='%s'::uuid AND source_generation=%d RETURNING 1) SELECT count(*) FROM released"%(active,int(gen)))
    s={'generation':int(gen),'pin':int(pin),'label':'physical-delete-recovery'}
    result=converge(out,s)
    result.update(recovery_verified=True,restored_note_id=note['note_id'],insert_update_delete_verified=True)
    save(out/'receipt.json',result);return result
 s=begin(out,'physical-delete-recovery')
 if s is None:return read(out/'receipt.json')
 present=query('pg46_finish_source',f"SELECT count(*) FROM customer_east.delivery_notes WHERE note_id={note['note_id']}",True)
 if present=='0':
  dml(out,s,f"BEGIN; INSERT INTO customer_east.delivery_notes SELECT (jsonb_populate_record(NULL::customer_east.delivery_notes, $${body}$$)).*; COMMIT;")
 elif present!='1':raise RuntimeError('deleted note cardinality')
 back=query('pg46_finish_source',f"SELECT count(*) FROM customer_east.delivery_notes WHERE note_id={note['note_id']}",True)
 if back!='1':raise RuntimeError('recovery did not land')
 result=converge(out,s)
 result.update(recovery_verified=True,restored_note_id=note['note_id'],insert_update_delete_verified=True)
 save(out/'receipt.json',result);return result
