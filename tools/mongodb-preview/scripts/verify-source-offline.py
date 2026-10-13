#!/usr/bin/env python3
"""Prove the sealed MongoDB target remains readable while the source server is stopped."""
import hashlib, json, os, pathlib, subprocess
ROOT=pathlib.Path(__file__).resolve().parents[3]
PRIVATE=pathlib.Path('/tmp/oo-dl01w1-41-gateway-preview')
SOURCE='oo-dl01w1-41-native-c5'
GOLD=ROOT/'evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/native-c5-20261012/cfc-selfbuild-fixed-76b/gold-target-aggregate.json'
OUT=pathlib.Path(os.environ.get('OO_W41_OFFLINE_OUT',str(ROOT/'evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/native-c5-20261012/cfc-gateway/source-offline.json')))
assert OUT.is_relative_to(ROOT/'evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/native-c5-20261012/cfc-gateway')
result=json.loads(GOLD.read_text())['result']; assert result['approved_rows']==14
state=lambda:subprocess.check_output(['docker','inspect','--format','{{.State.Status}}',SOURCE],text=True).strip()
assert state()=='running', 'source must start in running state'
addr=(PRIVATE/'listen.addr').read_text().strip(); assert addr.startswith('127.0.0.1:')
token=pathlib.Path(os.environ.get('OO_W41_TOKEN_FILE',str(PRIVATE/'client-20261013-01.token')))
assert token.parent==PRIVATE and token.name.startswith('client-') and token.suffix=='.token'
common=['--json','--auth','external','--token-file',str(token),'--gateway','http://'+addr,'--transport','local-development']
def query(sql):
 p=subprocess.run([str(ROOT/'tools/mongodb-preview/scripts/oo-gateway-cli.py'),'query',sql,*common],capture_output=True,text=True,timeout=30)
 if p.returncode: raise RuntimeError('OO query refused: '+p.stdout[:300])
 return json.loads(p.stdout)
subprocess.run(['docker','stop',SOURCE],check=True,capture_output=True)
try:
 assert state()=='exited'
 count=query('SELECT COUNT(*) AS shipment_count FROM oo_w141.shipments')
 units=query('SELECT SUM(units) AS units_sum FROM oo_w141.shipments')
 assert count['rows']==[[str(result['approved_rows'])]]
 assert units['rows']==[[result['units_sum']]]
 assert all(x['sources'][0]['source']=='oo_w141_native_c5' for x in [count,units])
 proof={'protocol':'oo.mongodb.source-offline-gateway/v1','source_state_during_query':'exited','target_generation':4,'count':count,'units_sum':units,'gold_sha256':hashlib.sha256(GOLD.read_bytes()).hexdigest()}
 OUT.parent.mkdir(parents=True,exist_ok=True);OUT.write_text(json.dumps(proof,sort_keys=True,separators=(',',':'))+'\n')
finally:
 subprocess.run(['docker','start',SOURCE],check=True,capture_output=True)
 assert state()=='running'
print(json.dumps({'proof':str(OUT),'sha256':hashlib.sha256(OUT.read_bytes()).hexdigest(),'source_restored':'running','query_ids':[count['query_id'],units['query_id']]}))
