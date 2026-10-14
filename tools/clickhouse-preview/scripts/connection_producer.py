#!/usr/bin/python3
"""Read one changed C4 partition; advance source map only from real worker ACK."""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys

from fresh_witness import TOKENS


def sha(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()
def save(path,value):
    pending=path.with_suffix('.pending')
    with pending.open('w') as stream:
        os.chmod(pending,0o600);json.dump(value,stream,indent=2);stream.write('\n');stream.flush();os.fsync(stream.fileno())
    pending.replace(path)

def identifiers(source,source_id):
    if not all(re.fullmatch(r'[A-Za-z_][A-Za-z0-9_]*',x) for x in (source['database'],source['table'],source['user'],source_id)):
        raise ValueError('source identifiers refused')
    if not re.fullmatch(r'[A-Za-z0-9_.-]+',source['host']) or type(source['port']) is not int or not 1<=source['port']<=65535:
        raise ValueError('source endpoint refused')

def signatures(parts):
    grouped={}
    for part in parts:grouped.setdefault(str(part['partition']),[]).append(part)
    if any(not re.fullmatch(r'[0-9]{6}',key) for key in grouped):raise ValueError('partition codec unsupported')
    return {key:'|'.join((key,str(sum(int(p['rows']) for p in rows)),str(sum(int(p['bytes_on_disk']) for p in rows)),
        max(str(p['modification_time']) for p in rows),','.join(sorted(str(p['name']) for p in rows)))) for key,rows in grouped.items()}

def main():
    os.umask(0o077)
    parser=argparse.ArgumentParser();parser.add_argument('--config',type=Path,required=True);a=parser.parse_args()
    conf=json.loads(a.config.read_text());job=json.load(sys.stdin);source=conf['source'];source_id=conf['source_id'];identifiers(source,source_id)
    if ({'database':source['database'],'table':source['table']} not in conf['approved_tables']
        or job.get('protocol')!='oo.jobs.native-producer/v1' or job.get('source_id')!=source_id
        or job.get('method')!='C4' or job.get('cycle')!='pull' or job.get('binding')!=conf['binding']
        or type(job.get('pin')) is not int or job['pin']<1):raise ValueError('job/config scope mismatch')
    for name in ('driver','sealer','worker'):
        if sha(conf[name])!=conf[name+'_sha256']:raise ValueError(name+' identity changed')
    root=Path(conf['output']);root.mkdir(mode=0o700,parents=True,exist_ok=True)
    with (root/'producer.lock').open('a') as lock:
        fcntl.flock(lock,fcntl.LOCK_EX)
        out=root/str(job['pin']);request_file=out/'request.json'
        if request_file.exists():
            request=json.loads(request_file.read_text())
            if sha(request['arrow_path'])!=request['input_digest']:raise ValueError('saved pin changed')
            sys.stdout.write(request_file.read_text());return
        out.mkdir(mode=0o700,exist_ok=True)
        def run(name,argv,data=None):
            if (out/(name+'.exit')).exists():raise ValueError('partial source attempt needs explicit recovery')
            result=subprocess.run(argv,input=data,capture_output=True,timeout=90)
            for suffix,body in [('stdout',result.stdout),('stderr',result.stderr),('exit',(str(result.returncode)+'\n').encode())]:
                (out/(name+'.'+suffix)).write_bytes(body)
            if result.returncode:raise RuntimeError(name+' failed; private raw retained')
            return result.stdout
        checkpoint={'pin':0,'partitions':{},'table_epoch':None}
        if (root/'acked-map.json').exists():checkpoint=json.loads((root/'acked-map.json').read_text())
        if job['pin']>1:
            state=json.loads(run('previous-ack',[conf['worker'],'--config',conf['worker_config'],'native-state','--binding',conf['binding']]))
            previous=root/str(job['pin']-1)/'request.json'
            if (state.get('applied') is not True or state.get('pin')!=job['pin']-1 or state.get('source_generation')!=job['pin']-1
                or not previous.exists()):raise ValueError('previous authoritative ACK absent')
            prior=json.loads(previous.read_text());capturer=json.loads((previous.parent/'capturer.json').read_text());scope=capturer['partition_scope']
            if checkpoint['pin'] not in (job['pin']-2,job['pin']-1):raise ValueError('ACK map is not contiguous')
            checkpoint.update(pin=job['pin']-1,table_epoch=scope['table_epoch'])
            checkpoint['partitions'][scope['partition_id']]=scope['current_signature']
            save(root/'acked-map.json',checkpoint)
        (out/'reader.user').write_text(source['user']+'\n')
        base=[conf['driver'],'--endpoint',f"http://{source['host']}:{source['port']}",
            '--user-file',str(out/'reader.user'),'--password-file',source['password_file'],
            '--row-cap',str(conf['row_cap']),'--byte-cap',str(conf['byte_cap']),'--timeout-ms',str(conf['timeout_ms'])]
        def read(name,sql,arrow=False):
            cmd=[base[0],'arrow' if arrow else 'read',*base[1:],'--sql',sql,
                '--query-id',conf['query_prefix']+str(job['pin'])+'-'+name,'--out',str(out/(name+'.json'))]
            cmd+=['--ipc-out',str(out/'full.arrow')] if arrow else ['--body-out',str(out/(name+'.body'))]
            run(name,cmd);return json.loads((out/(name+'.json')).read_text())
        db,table=source['database'],source['table'];where=f"database='{db}' AND table='{table}'"
        metadata={'columns':f'SELECT name,type FROM system.columns WHERE {where} ORDER BY position FORMAT JSONEachRow',
            'parts':f'SELECT partition,name,rows,bytes_on_disk,modification_time FROM system.parts WHERE {where} AND active ORDER BY partition,name FORMAT JSONEachRow',
            'mutations':f'SELECT count() FROM system.mutations WHERE {where} AND is_done=0 FORMAT TSV',
            'uuid':f"SELECT toString(uuid) FROM system.tables WHERE database='{db}' AND name='{table}' FORMAT TSV",
            'grants':f"SELECT access_type,database,table,column,is_partial_revoke,grant_option FROM system.grants WHERE user_name='{source['user']}' ORDER BY access_type,database,table,column FORMAT JSONEachRow",
            'settings':"SELECT name,value FROM system.settings WHERE name IN ('readonly','max_execution_time') ORDER BY name FORMAT JSONEachRow"}
        for name,sql in metadata.items():read(name,sql)
        actual=[json.loads(x) for x in (out/'columns.body').read_text().splitlines()]
        if actual!=conf['schema_columns']:raise ValueError('SCHEMA_CHANGED; no read or ACK')
        if (out/'mutations.body').read_text().strip()!='0':raise ValueError('DELETES_LAGGED; no ACK')
        epoch=(out/'uuid.body').read_text().strip()
        if not epoch or checkpoint['table_epoch'] not in (None,epoch):raise ValueError('POSITION_EXPIRED; explicit resnapshot required')
        current=signatures([json.loads(x) for x in (out/'parts.body').read_text().splitlines()])
        known_ids=sorted(set(current)|set(checkpoint['partitions']))
        changed=[key for key in known_ids if key not in checkpoint['partitions'] or current.get(key)!=checkpoint['partitions'][key]]
        if not changed:raise ValueError('no changed partition; pause completed owned window')
        partition=changed[0];names=[item['name'] for item in actual];part_where=f"_partition_id='{partition}'"
        result=read('capture',f"SELECT {','.join(names)} FROM {db}.{table} FINAL WHERE {part_where} ORDER BY reading_id FORMAT ArrowStream",True)
        rows=int(result['rows']);read('keys',f"SELECT toString(reading_id) FROM {db}.{table} FINAL WHERE {part_where} ORDER BY reading_id FORMAT TSV")
        keys=(out/'keys.body').read_text().splitlines()
        if len(keys)!=rows or len(set(keys))!=rows:raise ValueError('source key set changed')
        for name,sql in metadata.items():
            read(name+'-after',sql)
            if (out/(name+'.body')).read_bytes()!=(out/(name+'-after.body')).read_bytes():raise ValueError('source metadata changed during FINAL read')
        columns=[]
        for field in result['fields']:
            token,precision,scale=TOKENS[field['data_type']]
            col={'name':field['name'],'logical_name':field['name'],'arrow_type':token,'nullable':field['nullable']}
            if precision is not None:col['precision']=precision
            if scale is not None:col['scale']=scale
            if col['name']=='reading_id':col['key']=True
            columns.append(col)
        save(out/'schema-preimage.json',[{'table':table,'columns':columns}])
        save(out/'type-preimage.json',[{'table':table,**{k:v for k,v in col.items() if k in ('name','arrow_type','precision','scale')}} for col in columns])
        scope={'source_id':source_id,'source_generation':job['pin'],'logical_schema':db,'logical_table':table,
            'partition_id':partition,'observed_partition_ids':known_ids,'complete':True,'table_epoch':epoch,
            'schema_fingerprint':'\n'.join(item['name']+'\t'+item['type'] for item in actual),
            'schema_digest':sha(out/'schema-preimage.json'),'type_digest':sha(out/'type-preimage.json'),
            'arrow_sha256':sha(out/'full.arrow'),'row_count':rows,'current_signature':current.get(partition),'mutations_pending':False}
        snapshot=f'ch-{conf["binding"]}-pin-{job["pin"]}';snap={'kind':'token','value':snapshot}
        capturer={'protocol':'oo.clickhouse.capture-tick/v1','source_engine':'clickhouse','source_id':source_id,
            'source_generation':job['pin'],'label':snapshot,'snapshot':snap,'database':db,'logical_schema':db,'logical_table':table,
            'table_epoch':epoch,'cross_table_consistency':'B','product_reader_write_attempts':0,'snapshot_token':False,'partition_scope':scope,
            'tables':[{'name':table,'arrow':'full.arrow','cursor_safety':'no_cursor','consistency':'S0','route':'C4','delete_mode':'partition_replace','tombstones':[],'row_count':rows}]}
        witness={'protocol':'oo.source-admission.capture-witness/v1','source_engine':'clickhouse','source_id':source_id,
            'source_generation':job['pin'],'snapshot':snap,'run_id':snapshot,'logical_schema':db,'logical_table':table,
            'capturer_path':str(out/'capturer.json'),'arrow_path':str(out/'full.arrow'),'driver_path':conf['driver'],
            'schema_preimage_path':str(out/'schema-preimage.json'),'type_preimage_path':str(out/'type-preimage.json')}
        report=json.loads(Path(conf['report_template']).read_text());config=json.loads(Path(conf['admission_config']).read_text())
        permissions=[]
        for grant in map(json.loads,(out/'grants.body').read_text().splitlines()):
            if grant['is_partial_revoke'] or grant['grant_option'] or grant['column']:raise ValueError('reader grant shape unsupported')
            permission={'name':grant['access_type'],'status':'known','granted':True};grant_scope={k:grant[k] for k in ('database','table') if grant[k]}
            if grant_scope:permission['scope']=grant_scope
            permissions.append(permission)
        settings={x['name']:x['value'] for x in map(json.loads,(out/'settings.body').read_text().splitlines())}
        if settings!={'readonly':'1','max_execution_time':'8'}:raise ValueError('actual source profile refused')
        for document in (report,config):
            document.update(source_id=source_id,secret_ref=conf['secret_ref'],permissions=permissions)
            document['native']['clickhouse'].update(settings)
        report['driver']['digest']=conf['driver_sha256'];report['capture_witness']=str(out/'witness.json')
        for name,body in [('capturer',capturer),('witness',witness),('report',report),('config',config)]:save(out/(name+'.json'),body)
        run('seal',[conf['sealer'],'seal-native','--report',str(out/'report.json'),'--config',str(out/'config.json'),'--out',str(out/'sealed')])
        binding=json.loads((out/'sealed/registry.json').read_text())['bindings'][0]
        request={'protocol':'oo.pipeline.native-partition/v1','source_engine':'clickhouse','source_id':source_id,'source_generation':job['pin'],
            'logical_schema':db,'logical_table':table,'batch_id':snapshot,'capture_method':'c4_partition','delete_detection':'partition_replace',
            'consistency_grade':'S0','cross_table_consistent':False,'position_codec':'partition-signature/v1','position_lineage':'clickhouse.c4.partition',
            'snapshot':snap,'columns':columns,'arrow_path':str(out/'full.arrow'),'tombstones':[],'row_identity':'unique','key_column':'reading_id',
            'full_snapshot':False,'mirror':False,'admission_path':binding['admission_receipt_path'],
            'admission_expect':{'declared_receipt_sha256':binding['admission_receipt_sha256']},'target':conf['target'],
            'audit_digest':binding['capture_receipt_sha256'],'previous_ack_exists':partition in checkpoint['partitions'],
            'previous_signature':checkpoint['partitions'].get(partition)}
        for name in ('config_digest','schema_digest','type_digest','input_digest'):request[name]=binding[name]
        if job['pin']>1:request['parent_source_generation']=job['pin']-1
        save(request_file,request);sys.stdout.write(request_file.read_text())

if __name__=='__main__':
    try:main()
    except Exception as exc:
        print('ClickHouse producer refused: '+str(exc),file=sys.stderr);raise SystemExit(2)
