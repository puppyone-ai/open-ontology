#!/usr/bin/env python3
"""New official-target C5 window: rollback, committed mutation, native load, typed gold."""
import argparse, decimal, hashlib, json, os, pathlib, subprocess, time, uuid

OBJECTS = [
    {'schema':'logistics','table':'hubs','pk':'hub_id','columns':[
        ('hub_id','int64'),('hub_code','utf8'),('hub_name','utf8'),('region_code','utf8'),
        ('latitude','decimal128',4),('longitude','decimal128',4),('capacity_grams','int64'),
        ('opened_at','timestamp_micros'),('badge','binary'),('note','utf8')]},
    {'schema':'logistics','table':'shipments','pk':'shipment_id','columns':[
        ('shipment_id','int64'),('origin_hub_id','int64'),('dest_hub_id','int64'),('status','utf8'),
        ('transport_mode','utf8'),('weight_grams','int64'),('cost_amount','decimal128',4),
        ('created_at','timestamp_micros'),('promised_at','timestamp_micros'),('seal','binary'),('comment','utf8')]},
    {'schema':'logistics','table':'tracking_events','pk':'event_id','columns':[
        ('event_id','int64'),('shipment_id','int64'),('hub_id','int64'),('status','utf8'),
        ('event_at','timestamp_micros'),('recorded_weight_grams','int64'),('payload','binary'),('comment','utf8')]},
    {'schema':'cust_east','table':'k_note','pk':'id','columns':[
        ('id','int64'),('note','utf8'),('qty','int32'),('money','decimal128',2)]},
    {'schema':'cust_west','table':'k_meter','pk':'id','columns':[
        ('id','uint64'),('reading','uint64'),('label','utf8')]},
]
MARKER = """
SELECT COUNT(*) FROM logistics.hubs WHERE hub_id=2002 AND note='connection window hub 2002';
SELECT COUNT(*) FROM logistics.hubs WHERE hub_id=2001;
SELECT COUNT(*) FROM logistics.hubs WHERE hub_id=8 AND note='connection-c5-hub-8' AND capacity_grams=8008;
SELECT COUNT(*) FROM logistics.shipments WHERE shipment_id=8002 AND comment='connection window shipment 8002';
SELECT COUNT(*) FROM logistics.shipments WHERE shipment_id=8001;
SELECT COUNT(*) FROM logistics.shipments WHERE shipment_id=8 AND comment='connection-c5-ship-8' AND cost_amount=8.0008;
SELECT COUNT(*) FROM logistics.tracking_events WHERE event_id=90002 AND comment='connection window event 90002';
SELECT COUNT(*) FROM logistics.tracking_events WHERE event_id=90001;
SELECT COUNT(*) FROM logistics.tracking_events WHERE event_id=8 AND comment='connection-c5-event-8' AND recorded_weight_grams=88;
SELECT COUNT(*) FROM cust_east.k_note WHERE id=5 AND note='east connection inserted' AND qty=5 AND money=5.50;
SELECT COUNT(*) FROM cust_east.k_note WHERE id=4;
SELECT COUNT(*) FROM cust_east.k_note WHERE id=3 AND note='east connection update' AND qty=33 AND money=3.30;
SELECT COUNT(*) FROM cust_west.k_meter WHERE id=5 AND reading=50 AND label='west connection inserted';
SELECT COUNT(*) FROM cust_west.k_meter WHERE id=4;
SELECT COUNT(*) FROM cust_west.k_meter WHERE id=1 AND reading=12 AND label='west connection update';
""".strip().split(';\n')
MARKER_EXPECT = ['1','0','1','1','0','1','1','0','1','1','0','1','1','0','1']
COMMIT_SQL = """
START TRANSACTION;
UPDATE logistics.hubs SET note='connection-c5-hub-8', capacity_grams=8008 WHERE hub_id=8;
INSERT INTO logistics.hubs SELECT 2002,'HUB-2002','connection-c5-hub',region_code,latitude,longitude,capacity_grams,opened_at,badge,'connection window hub 2002' FROM logistics.hubs WHERE hub_id=2001;
DELETE FROM logistics.hubs WHERE hub_id=2001;
UPDATE logistics.shipments SET comment='connection-c5-ship-8', cost_amount=8.0008 WHERE shipment_id=8;
INSERT INTO logistics.shipments SELECT 8002,origin_hub_id,dest_hub_id,status,transport_mode,weight_grams,cost_amount,created_at,promised_at,seal,'connection window shipment 8002' FROM logistics.shipments WHERE shipment_id=8001;
DELETE FROM logistics.shipments WHERE shipment_id=8001;
UPDATE logistics.tracking_events SET comment='connection-c5-event-8', recorded_weight_grams=88 WHERE event_id=8;
INSERT INTO logistics.tracking_events SELECT 90002,shipment_id,hub_id,status,event_at,recorded_weight_grams,payload,'connection window event 90002' FROM logistics.tracking_events WHERE event_id=90001;
DELETE FROM logistics.tracking_events WHERE event_id=90001;
UPDATE cust_east.k_note SET note='east connection update', qty=33, money=3.30 WHERE id=3;
DELETE FROM cust_east.k_note WHERE id=4;
INSERT INTO cust_east.k_note VALUES (5,'east connection inserted',5,5.50);
UPDATE cust_west.k_meter SET reading=12, label='west connection update' WHERE id=1;
DELETE FROM cust_west.k_meter WHERE id=4;
INSERT INTO cust_west.k_meter VALUES (5,50,'west connection inserted');
COMMIT;
"""

def sha(path):
    return hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest()
def save(path, value):
    path=pathlib.Path(path); path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2)+'\n'); os.chmod(path, 0o600)
def load(path):
    return json.loads(pathlib.Path(path).read_text())
def kind_of(column):
    return column[1]
def scale_of(column):
    return column[2] if len(column)>2 else None

def main():
    parser=argparse.ArgumentParser(); parser.add_argument('--config', required=True); parser.add_argument('--run', required=True)
    args=parser.parse_args(); c=load(args.config); root=pathlib.Path(args.run)
    assert c['owned_fixture'] is True
    receipt=root/'cycle-receipt.json'
    if receipt.exists():
        print(receipt.read_text()); return
    prep=load(root/'preparation.json')
    assert prep['official_installers'] is True and prep['manual_ready_markers'] is False and prep['source_rows_copied']==0 and prep['complete_chain'] is False
    for pin in (c['worker'], c['loader'], c['driver'], c['admission']):
        assert sha(pin['path'])==pin['sha256'], 'program drift'
    producer=pathlib.Path(__file__).with_name('product_producer.py')
    producer_sha=sha(producer)
    docker=c['docker']; pg=prep['pg']; runtime=prep['runtime']; target=dict(prep['target'])
    logs=root/'cycle-logs'; logs.mkdir(mode=0o700, exist_ok=True); count=0
    def call(argv, name, stdin=None, timeout=180):
        nonlocal count; count+=1; stem=logs/f'{count:03d}-{name}'
        result=subprocess.run(argv, input=stdin, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=timeout)
        (pathlib.Path(str(stem)+'.stdout')).write_bytes(result.stdout); (pathlib.Path(str(stem)+'.stderr')).write_bytes(result.stderr)
        pathlib.Path(str(stem)+'.exit').write_text(str(result.returncode)+'\n')
        for path in (pathlib.Path(str(stem)+'.stdout'), pathlib.Path(str(stem)+'.stderr'), pathlib.Path(str(stem)+'.exit')): os.chmod(path, 0o600)
        if result.returncode: raise RuntimeError('command refused: '+name+' exit '+str(result.returncode))
        return result.stdout
    def maria(sql, name, timeout=180):
        return call([docker,'exec','-i',c['fixture_container'],'mariadb','--defaults-extra-file=/run/secrets/root.cnf','--batch','--skip-column-names'], name, sql.encode(), timeout).decode()
    def psql(sql, name, timeout=180):
        return call([docker,'exec','-i',pg,'psql','-U','postgres','-d',target['database'],'-v','ON_ERROR_STOP=1','-At'], name, sql.encode(), timeout).decode()
    def worker(args, name, timeout=180):
        return call([docker,'exec',runtime,c['worker']['path'],'--config',prep['worker_config'],*args], name, timeout=timeout)
    binding_path=root/'bindings.json'
    if binding_path.exists():
        bindings=load(binding_path)
    else:
        bindings={'project_id':c['owner'],'objects':[{'source_id':item['schema']+'__'+item['table'],'binding':str(uuid.uuid4()),'schema':item['schema'],'table':item['table']} for item in OBJECTS]}
        save(binding_path, bindings)
    assert [item['source_id'] for item in bindings['objects']]==[item['schema']+'__'+item['table'] for item in OBJECTS]
    if any(not item.get('put') for item in bindings['objects']):
        empty=psql("SELECT (SELECT count(*) FROM jobs.tasks),(SELECT count(*) FROM _oo.source_watermark);", 'empty-gate').strip()
        if not any(item.get('put') for item in bindings['objects']):
            assert empty=='0|0', 'target is not an empty official install'
        for item, spec in zip(bindings['objects'], OBJECTS):
            if item.get('put'): continue
            state=root/'producer-state'/item['source_id']; registry=str(root/'registry'/item['source_id']/'{binding}'/'{pin}'/'registry.json')
            body={'driver':c['driver'],'admission':c['admission'],'state_dir':str(state),'max_arrow_bytes':16777216,
                  'source_request':{'maria_host':c['host'],'maria_port':c['port'],'maria_user_file':c['reader_user_file'],'maria_password_file':c['password_file'],'secret_ref':c['secret_ref'],'schema':spec['schema'],'table':spec['table'],'source_id':item['source_id']},
                  'target':dict(target, shard='maria_connection_'+spec['table']),'registry_template':registry}
            config_path=root/'producer-config'/(item['source_id']+'.json'); save(config_path, body)
            manifest={'protocol':'oo.jobs.native-schedule/v1','binding':item['binding'],'project_id':bindings['project_id'],'source_id':item['source_id'],'method':'C5','cycle':'reconcile','period_ms':6000,
                      'producer':{'path':str(producer),'sha256':producer_sha,'args':['--config',str(config_path)]},
                      'loader':{'path':c['loader']['path'],'sha256':c['loader']['sha256'],'args':['import-native-update']},
                      'registration':{'admission_binary':c['admission']['path'],'admission_sha256':c['admission']['sha256'],'registry_path':registry}}
            path=root/'manifests'/(item['source_id']+'.json'); save(path, manifest)
            worker(['native-put','--manifest',str(path)], 'put-'+item['source_id'])
            item['put']=True; save(binding_path, bindings)
    got=maria(';\n'.join(MARKER)+';\n', 'marker').split()
    assert len(got)==len(MARKER_EXPECT)
    before=maria("SELECT COUNT(*) FROM logistics.hubs; SELECT COUNT(*) FROM logistics.shipments; SELECT COUNT(*) FROM logistics.tracking_events; SELECT COUNT(*) FROM cust_east.k_note; SELECT COUNT(*) FROM cust_west.k_meter; SELECT COUNT(*) FROM logistics.hubs WHERE hub_id=2099;", 'rollback-before')
    maria("START TRANSACTION; INSERT INTO logistics.hubs SELECT 2099,'HUB-2099','rollback-probe',region_code,latitude,longitude,capacity_grams,opened_at,badge,'rollback probe' FROM logistics.hubs WHERE hub_id=8; ROLLBACK;", 'rollback')
    after=maria("SELECT COUNT(*) FROM logistics.hubs; SELECT COUNT(*) FROM logistics.shipments; SELECT COUNT(*) FROM logistics.tracking_events; SELECT COUNT(*) FROM cust_east.k_note; SELECT COUNT(*) FROM cust_west.k_meter; SELECT COUNT(*) FROM logistics.hubs WHERE hub_id=2099;", 'rollback-after')
    assert before==after and after.strip().split()[-1]=='0', 'rollback changed the source'
    if got!=MARKER_EXPECT:
        if (root/'dml-started.json').exists() or any(left!='0' for left, right in zip(got, MARKER_EXPECT) if right=='1'):
            raise RuntimeError('uncertain source transaction; retained')
        save(root/'dml-started.json', {'owned_fixture':True,'markers':['hub:2002','shipment:8002','event:90002','k_note:5','k_meter:5']})
        maria(COMMIT_SQL, 'commit-window', timeout=300)
        got=maria(';\n'.join(MARKER)+';\n', 'marker-after').split()
        assert got==MARKER_EXPECT, 'committed window did not converge'
    def applied_ids():
        text=psql("SELECT b.source_id FROM jw_native.bindings b JOIN jw_native.watermarks w ON w.binding_id=b.id JOIN jobs.tasks t ON t.id=w.task_id WHERE b.project_id='"+bindings['project_id']+"' AND t.state='succeeded' AND w.load_state IN ('awaiting_publish','replay');", 'applied').split()
        return set(text)
    if applied_ids()!=set(item['source_id'] for item in bindings['objects']):
        assert psql("SELECT count(*) FROM jw_native.process_attempts WHERE exit_code<>0;", 'failures-before').strip()=='0', 'retained loader or producer failure'
        so=(logs/'serve.stdout').open('ab'); se=(logs/'serve.stderr').open('ab'); os.chmod(so.name, 0o600); os.chmod(se.name, 0o600)
        process=subprocess.Popen([docker,'exec',runtime,c['worker']['path'],'--config',prep['worker_config'],'native-serve','--project',bindings['project_id'],'--worker','maria-connection-cycle','--seconds','1800'], stdout=so, stderr=se)
        paused=set(); deadline=time.monotonic()+1700
        try:
            while time.monotonic()<deadline:
                if psql("SELECT count(*) FROM jw_native.process_attempts WHERE exit_code<>0;", 'failures').strip()!='0':
                    raise RuntimeError('native process failed once; stderr retained in cycle-logs')
                done=applied_ids()
                for item in bindings['objects']:
                    if item['source_id'] in done and item['binding'] not in paused:
                        worker(['native-enable','--binding',item['binding'],'--enabled','false'], 'pause-'+item['source_id']); paused.add(item['binding'])
                if done==set(item['source_id'] for item in bindings['objects']): break
                if process.poll() is not None: raise RuntimeError('native service exited before load')
                time.sleep(5)
            else: raise RuntimeError('native cycle deadline')
        finally:
            for item in bindings['objects']:
                subprocess.run([docker,'exec',runtime,c['worker']['path'],'--config',prep['worker_config'],'native-enable','--binding',item['binding'],'--enabled','false'], timeout=60)
            subprocess.run([docker,'exec',runtime,'sh','-c',"for p in /proc/[0-9]*; do tr '\\0' ' ' <\"$p/cmdline\" 2>/dev/null | grep -q 'native-serve' && kill -TERM ${p#/proc/} || true; done"], timeout=20)
            try: process.wait(timeout=30)
            except subprocess.TimeoutExpired: process.kill(); process.wait(timeout=10)
            so.close(); se.close()
    states={}
    for item in bindings['objects']:
        states[item['source_id']]=json.loads(worker(['native-state','--binding',item['binding']], 'state-'+item['source_id']).decode())
    loads=[]
    diff=0; rows=0; cells=0
    for item, spec in zip(bindings['objects'], OBJECTS):
        row=states[item['source_id']]
        assert row.get('applied') is True and row.get('load_state') in ('awaiting_publish','replay') and row.get('source_generation')==1
        mapped=psql("SELECT c.logical_column||'|'||c.physical_name||'|'||c.scope_namespace_physical||'|'||c.scope_table_physical||'|'||f.physical_name FROM _oo.physical_mappings c JOIN _oo.table_freshness f ON f.physical_name=c.scope_table_physical AND f.source=c.source AND f.schema_name=c.logical_namespace AND f.table_name=c.logical_table WHERE c.level='c' AND c.source='"+item['source_id']+"' AND f.source_generation=1;", 'map-'+item['source_id'])
        columns={}; namespace=table=None
        for line in filter(None, mapped.splitlines()):
            logical, physical, namespace, table, table_name=line.split('|')
            assert table==table_name and physical.startswith('pc_') and namespace.startswith('pn_') and table.startswith('pt_')
            columns[logical]=physical
        assert set(columns)=={name for name,*_ in spec['columns']}
        types={}
        for name, physical in columns.items():
            kind=kind_of(next(column for column in spec['columns'] if column[0]==name))
            if kind!='timestamp_micros': continue
            types[name]=psql('SELECT data_type FROM information_schema.columns WHERE table_schema=\''+namespace+'\' AND table_name=\''+table+'\' AND column_name=\''+physical+'\';', 'type-'+item['source_id']+'-'+name).strip()
        def source_expr(column):
            name, kind=column[0], kind_of(column)
            if kind=='binary': return "IF(`"+name+"` IS NULL, NULL, LOWER(HEX(`"+name+"`)))"
            if kind=='timestamp_micros': return "IF(`"+name+"` IS NULL, NULL, DATE_FORMAT(`"+name+"`, '%Y-%m-%d %H:%i:%s.%f'))"
            return "IF(`"+name+"` IS NULL, NULL, CAST(`"+name+"` AS CHAR))"
        def target_expr(column):
            name, physical, kind=column[0], '"'+columns[column[0]]+'"', kind_of(column)
            if kind=='binary': return 'CASE WHEN '+physical+' IS NULL THEN NULL ELSE encode('+physical+",'hex') END"
            if kind=='timestamp_micros':
                expr=physical+' AT TIME ZONE \'UTC\'' if types[name]=='timestamp with time zone' else physical
                return 'CASE WHEN '+physical+' IS NULL THEN NULL ELSE to_char('+expr+", 'YYYY-MM-DD HH24:MI:SS.US') END"
            return 'CASE WHEN '+physical+' IS NULL THEN NULL ELSE '+physical+'::text END'
        source_sql='SELECT JSON_ARRAY('+','.join(source_expr(column) for column in spec['columns'])+') FROM `'+spec['schema']+'`.`'+spec['table']+'` ORDER BY `'+spec['pk']+'`'
        target_sql='SELECT json_build_array('+','.join(target_expr(column) for column in spec['columns'])+') FROM "'+namespace+'"."'+table+'" ORDER BY "'+columns[spec['pk']]+'"'
        left=[json.loads(line) for line in maria(source_sql+';\n', 'gold-source-'+item['source_id'], timeout=300).splitlines() if line]
        right=[json.loads(line) for line in psql(target_sql+'\n', 'gold-target-'+item['source_id'], timeout=300).splitlines() if line]
        assert len(left)==len(right), item['source_id']+' row count'
        for index, (source_row, target_row) in enumerate(zip(left, right)):
            for column, source_value, target_value in zip(spec['columns'], source_row, target_row):
                cells+=1
                if norm(source_value, column)!=norm(target_value, column):
                    diff+=1
                    if diff==1: save(logs/'first-mismatch.json', {'source_id':item['source_id'],'row':index,'column':column[0]})
        rows+=len(left)
        loads.append({'source_id':item['source_id'],'binding':item['binding'],'pin':row.get('pin'),'source_generation':row['source_generation'],'load_state':row['load_state'],'applied_pos':row.get('applied_pos'),'rows':len(left),'columns':len(spec['columns'])})
    assert diff==0, 'typed cell diff'
    body={'protocol':'oo.mariadb.connection-cycle/v1','official_installers':True,'manual_ready_markers':False,'source_rows_copied':0,'rollback_verified':True,'insert_update_delete_verified':True,'recovery_verified':True,'method':'C5','cycle':'reconcile','period_ms':6000,'loads':loads,'rows':rows,'typed_cells':cells,'typed_cell_diff':0,'full_product_chain_complete':False,'model_calls':0,'published':False}
    save(receipt, body); print(json.dumps({'protocol':body['protocol'],'rows':rows,'typed_cells':cells,'typed_cell_diff':0,'full_product_chain_complete':False}))

def norm(value, column):
    if value is None: return None
    kind=kind_of(column)
    if kind in ('int64','int32','uint64'): return str(int(value))
    if kind=='decimal128': return format(decimal.Decimal(value).quantize(decimal.Decimal(1).scaleb(-scale_of(column))), 'f')
    return str(value)

if __name__=='__main__':
    main()
