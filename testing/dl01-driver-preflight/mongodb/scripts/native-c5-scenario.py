#!/usr/bin/env python3
"""Owned standalone synthetic continuous write window and independent full truth."""
import json,pathlib,subprocess,time,sys

root=pathlib.Path(__file__).resolve().parents[4]
raw=root/'evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/native-c5-20261012'
container='oo-dl01w1-41-native-c5'
config=raw/'config.json'
config.write_text(json.dumps({'container':container,'every_ms':1500,'consumer_command':None})+'\n')
def run(name,mode,step=None):
    command=['docker','exec','-e','C5_MODE='+mode,'-e','C5_OUT=/tmp/'+name+'.json']
    if step is not None:command+=['-e','C5_STEP='+str(step)]
    script='/tmp/native-c5-maintain.js' if step is not None else '/tmp/c5-live.js'
    result=subprocess.run(command+[container,'mongosh','--quiet','--norc','--file',script],capture_output=True)
    (raw/(name+'.stdout')).write_bytes(result.stdout);(raw/(name+'.stderr')).write_bytes(result.stderr);(raw/(name+'.exit')).write_text(str(result.returncode)+'\n')
    if result.returncode:raise RuntimeError(name+' failed')

loop=subprocess.Popen([sys.executable,str(root/'tools/mongodb-preview/scripts/native-c5-loop.py'),'--config',str(config),'--out',str(raw/'cycles'),'--cycles','12'],stdout=(raw/'loop.stdout').open('wb'),stderr=(raw/'loop.stderr').open('wb'))
time.sleep(2)
run('dba-a','dba-a')
time.sleep(2)
run('dba-b','dba-b')
for step in range(1,4):
    time.sleep(2)
    run('maintain-'+str(step),'maintain',step)
    if step==1:config.write_text(json.dumps({'container':container,'every_ms':2100,'consumer_command':None})+'\n')
rc=loop.wait();(raw/'loop.exit').write_text(str(rc)+'\n')
if rc:raise RuntimeError('C5 loop failed')
initial=json.loads((raw/'initial.json').read_text())
expected={r['key']:r['document'] for r in initial['rows']}
del expected['c141d04141eef0410000000f']
# The original row0 and inserted row16 carry unclassified hiddenNote and must
# stay absent from the approved full document target, rather than be projected.
del expected['c141d04141eef04100000000']
expected['c141d04141eef04100000001']['memo']='native-cycle-3'
expected['c141d04141eef04100000001']['weight']={'$numberDouble':'45.25'}
final=json.loads((raw/'cycles/0011/approved.json').read_text())
diff=[{'key':k,'expected':expected.get(k),'actual':final.get(k)} for k in sorted(set(expected)|set(final)) if expected.get(k)!=final.get(k)]
(raw/'independent-full-truth.json').write_text(json.dumps({'expected_rows':len(expected),'actual_rows':len(final),'rows_differ':len(diff),'full_unsampled':True,'diff':diff,'product_target_verified':False,'source_acceptance':False},sort_keys=True)+'\n')
if diff:raise RuntimeError('full typed truth mismatch')
