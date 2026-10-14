#!/usr/bin/env python3
"""Two real C5 reads of the explicitly owned connection fixture; no publication claim."""
import argparse,hashlib,json,pathlib,subprocess,time
ROOT=pathlib.Path(__file__).resolve().parents[3]
IMAGE='docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5'
def save(p,v):p.write_text(json.dumps(v,sort_keys=True)+'\n')
def main():
 p=argparse.ArgumentParser();p.add_argument('--config',required=True);p.add_argument('--out',required=True);a=p.parse_args()
 cfg=json.loads(pathlib.Path(a.config).read_text());out=pathlib.Path(a.out);private=pathlib.Path(cfg['private_root'])
 assert cfg['fixture_container']=='oo-mongodb-connection-whole-source' and cfg['every_ms']>0
 assert cfg['host']==cfg['fixture_container'], 'full capture requires the explicitly owned endpoint'
 assert subprocess.check_output(['docker','inspect',cfg['fixture_container'],'--format','{{.Config.Image}}'],text=True).strip()=='mongo@sha256:d0d926f94df099bff534b7ee5b5986458131a22489dfff8664509af0c1e2ca9c'
 connection={k:cfg[k] for k in ['host','port','user','auth_database']};save(out/'connection.json',connection)
 customer=json.loads((ROOT/'tools/mongodb-preview/examples/customer.shipments.json').read_text())
 customer.update(namespace=cfg['database'],secret_ref=cfg['secret_ref'],approved_objects=[cfg['database']+'.shipments'],batch_object=cfg['database']+'.shipments',deny_object=cfg['database']+'.not_approved')
 save(out/'customer.json',customer);save(out/'driver-registry.json',{'protocol':'oo.operator.secret-registry/v1','entries':[{'secret_ref':cfg['secret_ref'],'file':cfg['password_file']}]})
 outputs=[]
 for n in range(2):
  started=time.monotonic();cycle=out/f'cycle-{n}';cycle.mkdir()
  command=['docker','run','--rm','--name','oo-mongodb-connection-whole-reader','--network','oo-mongodb-connection-whole-net','-v',str(private)+':'+str(private),'-e','OO_MONGO_CONNECTION='+str(out/'connection.json'),'-e','OO_W141_CUSTOMER='+str(out/'customer.json'),'-e','OO_W141_SECRET_REGISTRY='+str(out/'driver-registry.json'),'-e','LD_PRELOAD='+str(private/'bin/liboo-opmsg-names.so'),'-e','OO_W141_OPMSG_LOG='+str(cycle/'opmsg-names.jsonl'),'-e','OO_C5_CAPTURE_ONLY=1','-e','OO_OUT='+str(cycle),IMAGE,str(private/'bin/oo-mongodb-driver-probe')]
  r=subprocess.run(command,capture_output=True,timeout=60);(cycle/'stdout').write_bytes(r.stdout);(cycle/'stderr').write_bytes(r.stderr);(cycle/'exit').write_text(str(r.returncode)+'\n')
  if r.returncode:raise RuntimeError('real driver refused')
  census=json.loads((cycle/'c5-census.json').read_text());commands=json.loads((cycle/'commands.json').read_text())
  assert not any(k in {'insert','update','delete','findAndModify','drop','create'} for k in commands['names'])
  outputs.append({'cycle':n,'rows':census['approved_rows'],'raw_bson_sha256':hashlib.sha256((cycle/'full-approved.bson.hex').read_bytes()).hexdigest(),'driver_exit':r.returncode})
  if n==0:
   mutation=private/'mutation.js'
   r=subprocess.run(['docker','exec',cfg['fixture_container'],'mongosh','--quiet','--norc','--file','/private/mutation.js'],capture_output=True,timeout=30)
   (out/'fixture-mutation.stdout').write_bytes(r.stdout);(out/'fixture-mutation.stderr').write_bytes(r.stderr);(out/'fixture-mutation.exit').write_text(str(r.returncode)+'\n')
   if r.returncode:raise RuntimeError('owned fixture mutation refused')
   time.sleep(max(0,cfg['every_ms']/1000-(time.monotonic()-started)))
 assert outputs[0]['raw_bson_sha256']!=outputs[1]['raw_bson_sha256']
 save(out/'capture-manifest.json',{'protocol':'oo.mongodb.connection-capture/v1','actual_connection_consumed':True,'cycles':outputs,'capture_method':'C5','every_ms':cfg['every_ms'],'delete_detection':'reconcile','consistency':'B','quality':['DELETES_LAGGED'],'source_reader_writes':0,'source_process_observed':'running','S0_S10_complete':False,'maintenance_published':False,'pack_updated':False,'gateway_dialogue_verified':False,'source_acceptance':False})
if __name__=='__main__':main()
