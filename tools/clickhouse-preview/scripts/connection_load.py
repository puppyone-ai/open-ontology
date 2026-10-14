#!/usr/bin/env python3
"""Seal actual per-partition captures and run real claim→Loader→CAS→ACK."""
import argparse
import json
from pathlib import Path
import re
import uuid

import fresh_capture as f
from connection_target import Target, mount_path

def replay_ok(before, after):
    return after == before

def raw_json(run, label):
    path = run/'target-raw'/f'{label}.stdout'
    if not path.exists():
        return None
    exit_path = run/'target-raw'/f'{label}.exit'
    if exit_path.exists() and exit_path.read_text().strip() not in ('', '0'):
        return None
    text = path.read_text()
    if not text.strip():
        return None
    return json.loads(text)

def main():
    p=argparse.ArgumentParser();p.add_argument('--config',type=Path,required=True);p.add_argument('--run',type=Path,required=True);p.add_argument('--capture',type=Path,required=True);p.add_argument('--partitions',nargs='+');a=p.parse_args()
    run=Path(mount_path(a.run));capture=Path(mount_path(a.capture));config=json.loads(a.config.read_text());t=Target(config,run)
    observation=json.loads((capture/'source-observation.json').read_text());results=[];skipped=[]
    project=config.get('job_project','18181818-1818-4818-8818-181818181818')
    target=json.loads((run/'loader-target.json').read_text())
    admission=Path(config['admission_bin']);source_id=config.get('source_id','clickhouse_connection_whole')
    for part in observation['partitions']:
        if a.partitions and part['partition_id'] not in a.partitions:continue
        folder=capture/('admission-'+part['partition_id']);sealed=folder/'sealed'
        seen=mount_path(run)
        cmd=['docker','run','--rm','--pull','never',
            '--mount',f'type=bind,source={seen},target={seen}',
            '--mount','type=bind,source=/tmp/oo-dl01w1-40-gpt-producer,target=/tmp/oo-dl01w1-40-gpt-producer,readonly',
            '--mount',f'type=bind,source={admission},target=/bin/oo-source-admission,readonly',
            '--entrypoint','/bin/oo-source-admission',f.RUST_IMAGE,'seal-native',
            '--report',str(folder/'report.json'),'--config',str(folder/'config.json'),'--out',str(sealed)]
        if not sealed.exists():t.exec('seal-'+capture.name+'-'+part['partition_id'],cmd)
        binding=json.loads((sealed/'registry.json').read_text())['bindings'][0]
        generation=binding['source_generation'];label=f'g{generation}-{part["partition_id"]}'
        source_id=binding['source_id']
        prior=json.loads(t.psql('prior-'+label,f"SELECT row_to_json(r) FROM (SELECT current_signature,acknowledged FROM _oo.native_partition_state WHERE source='{binding['source_id']}' AND schema_name='{binding['partition_scope']['logical_schema']}' AND table_name='{binding['partition_scope']['logical_table']}' AND partition_id='{part['partition_id']}') r;").decode() or '{}')
        if prior.get('acknowledged') is True and prior.get('current_signature')==part.get('current_signature'):
            skipped.append({'partition':part['partition_id'],'generation':generation,'reason':'acked_signature'});continue
        if prior and prior.get('acknowledged') is False and not raw_json(run,'load-'+label):
            raise RuntimeError('prior partition not ACKed; recovery required')
        columns=json.loads((folder/'schema-preimage.json').read_text())[0]['columns']
        dedupe='ch-connection-'+str(uuid.uuid4());worker='ch-connection-whole'
        enq=raw_json(run,'enqueue-'+label)
        if enq is None:
            enq=json.loads(t.psql('enqueue-'+label,f"SET ROLE oo_jobs_runtime; SELECT row_to_json(r) FROM jobs.enqueue('{project}','snapshot_block',1,'{{\"source_id\":\"{binding['source_id']}\",\"generation\":{generation}}}','{binding['config_digest']}','{binding['input_digest']}','{dedupe}',clock_timestamp(),3) r;").decode())
        claimed=raw_json(run,'claim-'+label)
        if claimed is None:
            claimed=json.loads(t.psql('claim-'+label,f"SET ROLE oo_jobs_runtime; SELECT row_to_json(r) FROM jobs.claim('{project}',ARRAY['snapshot_block'],'{worker}') r;").decode())
            if claimed['task_id']!=enq['task_id'] or claimed['state']!='running':raise RuntimeError('real claim differs')
        request={'protocol':'oo.pipeline.native-partition/v1','source_engine':'clickhouse','source_id':binding['source_id'],
            'source_generation':generation,'parent_source_generation':generation-1 if generation>1 else None,
            'logical_schema':binding['partition_scope']['logical_schema'],'logical_table':binding['partition_scope']['logical_table'],
            'batch_id':'ch-connection-'+dedupe,'capture_method':'c4_partition','delete_detection':'partition_replace',
            'consistency_grade':'S0','cross_table_consistent':False,'position_codec':'partition-signature/v1',
            'position_lineage':'clickhouse.c4.partition','snapshot':binding['snapshot'],'columns':columns,
            'arrow_path':str(capture/(part['partition_id']+'.arrow')),'tombstones':[],'row_identity':'unique','key_column':'reading_id',
            'full_snapshot':False,'mirror':True,'admission_path':binding['admission_receipt_path'],
            'admission_expect':{'declared_receipt_sha256':binding['admission_receipt_sha256']},'target':target,
            'audit_digest':binding['capture_receipt_sha256'],'config_digest':binding['config_digest'],
            'schema_digest':binding['schema_digest'],'type_digest':binding['type_digest'],'input_digest':binding['input_digest'],
            'previous_ack_exists':bool(prior),'previous_signature':prior.get('current_signature'),'lease_id':claimed['task_id'],'lease_generation':claimed['generation']}
        loaded=raw_json(run,'load-'+label)
        if loaded is None:
            loaded=t.pipeline('load-'+label,'import-native-update',request,sealed/'registry.json')
            if part['partition_id']==config.get('replay_partition'):
                physical=loaded['physical_name']
                if not re.fullmatch(r'pt_[0-9a-f]{60}', physical):raise RuntimeError('bad physical table')
                before=int(t.psql('replay-before-'+label,f'SELECT count(*) FROM _oo.{physical}').decode().strip())
                replay={'before':before}
                try:
                    again=t.pipeline('replay-'+label,'import-native-update',request,sealed/'registry.json')
                    replay['after']=again.get('row_count');replay['second_import']='accepted'
                except RuntimeError:
                    replay['after']=int(t.psql('replay-after-'+label,f'SELECT count(*) FROM _oo.{physical}').decode().strip())
                    replay['second_import']='refused_count_unchanged'
                if not replay_ok(before, replay['after']):
                    raise RuntimeError('replay changed the physical row count')
                (capture/'replay.json').write_text(json.dumps(replay)+'\n')
        if loaded['load_state']!='awaiting_publish' or not loaded['source_bound']:raise RuntimeError('source-bound load missing')
        request['batch_id']=loaded.get('partition_target_receipt',{}).get('batch_id',request['batch_id'])
        completed=raw_json(run,'complete-'+label)
        if completed is None:
            completed=json.loads(t.psql('complete-'+label,f"SET ROLE oo_jobs_runtime; SELECT row_to_json(r) FROM jobs.complete_success('{project}','{claimed['task_id']}','{worker}',{claimed['generation']}) r;").decode())
        if completed['state']!='succeeded':raise RuntimeError('real CAS refused')
        acked=raw_json(run,'ack-'+label)
        if acked is None:
            ack={'protocol':'oo.pipeline.native-partition-ack/v1','source_id':binding['source_id'],
                'logical_schema':request['logical_schema'],'logical_table':request['logical_table'],'partition_id':part['partition_id'],
                'run_id':loaded['run_id'],'batch_id':request['batch_id'],'payload_digest':loaded['payload_digest'],
                'job_task_id':claimed['task_id'],'job_generation':claimed['generation']}
            acked=t.pipeline('ack-'+label,'ack-native-partition',ack)
        if not acked['acknowledged']:raise RuntimeError('ACK refused')
        results.append({'partition':part['partition_id'],'generation':generation,'run_id':loaded['run_id'],
            'load':loaded,'cas':completed,'ack':acked,'registry':str(sealed/'registry.json')})
    if not results and (capture/'load-manifest.json').exists():
        results=json.loads((capture/'load-manifest.json').read_text()).get('results',[])
    if not results:raise RuntimeError('no new partition load')
    (capture/'load-manifest.json').write_text(json.dumps({'protocol':'oo.clickhouse.fresh-native-load/v1','source_id':source_id,'source_uuid':observation['partitions'][0]['table_epoch'],'results':results,'skipped':skipped},indent=2)+'\n')

if __name__=='__main__':main()
