#!/usr/bin/env python3
"""One bounded live cancel probe through the public MongoDB OO CLI."""
import json, pathlib, subprocess, time
ROOT=pathlib.Path(__file__).resolve().parents[3]
PRIVATE=pathlib.Path('/tmp/oo-dl01w1-41-gateway-preview')
OUT=ROOT/'evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/native-c5-20261012/cfc-gateway/cancel-once'
OUT.mkdir(parents=True,exist_ok=False)
addr=(PRIVATE/'listen.addr').read_text().strip(); assert addr.startswith('127.0.0.1:')
cli=str(ROOT/'tools/mongodb-preview/scripts/oo-gateway-cli.py')
common=['--json','--auth','external','--token-file',str(PRIVATE/'client-20261013-01.token'),'--gateway','http://'+addr,'--transport','local-development']
table='oo_w141.shipments'
sql='SELECT COUNT(*) AS large_count FROM '+(' CROSS JOIN '.join(table+' '+alias for alias in 'abcdefgh'))
query=subprocess.Popen([cli,'query',sql,*common,'--timeout-ms','10000'],stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
ops=[];server_id=None
for _ in range(30):
 time.sleep(.1)
 call=subprocess.run([cli,'operations',*common],capture_output=True,text=True,timeout=10)
 ops.append({'exit':call.returncode,'stdout':call.stdout,'stderr':call.stderr})
 try:
  body=json.loads(call.stdout)
  for entry in body if isinstance(body,list) else body.get('operations',[]):
   if entry.get('server_id') and entry.get('status')=='running':server_id=entry['server_id'];break
 except (ValueError,TypeError):pass
 if server_id or query.poll() is not None:break
cancel=None
if server_id:
 cancel=subprocess.run([cli,'cancel',server_id,*common],capture_output=True,text=True,timeout=10)
try:stdout,stderr=query.communicate(timeout=20)
except subprocess.TimeoutExpired:
 query.terminate();stdout,stderr=query.communicate(timeout=5)
(OUT/'query.stdout').write_text(stdout);(OUT/'query.stderr').write_text(stderr);(OUT/'query.exit').write_text(str(query.returncode)+'\n')
(OUT/'operations.json').write_text(json.dumps(ops,sort_keys=True)+'\n')
if cancel:
 (OUT/'cancel.stdout').write_text(cancel.stdout);(OUT/'cancel.stderr').write_text(cancel.stderr);(OUT/'cancel.exit').write_text(str(cancel.returncode)+'\n')
result={'server_id':server_id,'query_exit':query.returncode,'query_code':json.loads(stdout).get('code') if stdout.startswith('{') else None,'cancel_exit':cancel.returncode if cancel else None,'cancel':json.loads(cancel.stdout) if cancel and cancel.stdout.startswith('{') else None}
(OUT/'result.json').write_text(json.dumps(result,sort_keys=True)+'\n')
print(json.dumps(result))
if result['query_code']!='TIMEOUT' or result['cancel_exit']!=0:raise SystemExit(1)
