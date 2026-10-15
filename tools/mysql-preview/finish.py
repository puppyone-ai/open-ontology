#!/usr/bin/env python3
"""Bind the ordinary entry's current product and new Agent evidence to the C1 run."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys


def load(p):return json.loads(Path(p).read_bytes())
def sha(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def save(p,v):
    import os
    with Path(p).open('x') as f:
        os.chmod(p,0o600);json.dump(v,f,sort_keys=True,separators=(',',':'));f.write('\n')


def run(config,out,phase):
    c=load(config);out=Path(out);p=c['onboarding']['product'];proof=p['final'];cut=Path(proof['cut']);ready=load(cut/'gateway-ready.json');product=out/'product'
    if phase=='dialogue-next':
        first=load(out/'dialogue1/verdict.json')
        if first['outcome']!='verified' or first['selection']!='random' or not first['counts_as_random']:
            raise ValueError('first_random_dialogue')
        d=load(proof['agent_config']);d['prior_manifest']=str(out/'dialogue1/selection.json');save(out/'dialogue2-config.json',d);return
    current=load(product/'maintenance-consumption.json')
    if current['source_generation']!=ready['source_generation'] or current['pack_digest']!=ready['pack_digest'] or current['role']!=ready['role']:
        raise ValueError('current_product_identity')
    gateway=Path(ready['private_root'])
    if sha(product/'current-role-pack.json')!=sha(gateway/'packs/mysql_full.json'):
        raise ValueError('current_pack_bytes')
    d=load(proof['agent_config'])
    if sha(d['gateway_token_file'])!=ready['token_sha256'] or d['identity']['pack_sha256']!=sha(product/'current-role-pack.json'):
        raise ValueError('caller_identity')
    state=subprocess.run(['docker','exec',c['onboarding']['container'],c['onboarding']['worker'],'--config',c['onboarding']['worker_config'],'native-state','--binding',load(Path(c['onboarding']['prepared_root'])/'prepared/identity.json')['binding_id']],capture_output=True,check=True)
    observed=json.loads(state.stdout)
    if observed['source_generation']!=ready['source_generation'] or observed['applied_pos']!=load(cut/'native/load.json')['applied_pos']:
        raise ValueError('source_advanced')
    flags=['--json','--gateway',d['gateway'],'--transport',d['transport'],'--auth',d['auth'],'--token-file',d['gateway_token_file']]
    preflight=subprocess.run([d['oo_cli'],'freshness',*flags],capture_output=True,check=True)
    fresh=json.loads(preflight.stdout)
    if fresh['publication']!='live' or not fresh['freshness'] or not fresh['sources'] or any(w['code']=='STALE_DATA' for w in fresh['warnings']):
        raise ValueError('caller_not_ready')
    gold_receipt=load(Path(d['independent_truth_file']).parent/'source-gold-receipt.json')
    ref=gold_receipt['source_connection_reference']
    source=subprocess.run(['docker','exec',ref['container'],'mysql','--defaults-extra-file='+ref['private_client_file'],'--batch','--raw','--skip-column-names','-e','SELECT @@GLOBAL.gtid_executed'],capture_output=True,check=True)
    if source.stdout.decode().strip()!=gold_receipt['gtid']:raise ValueError('source_changed_after_gold')
    if phase=='preflight':
        (out/'current-preflight.stdout').write_bytes(preflight.stdout);save(out/'current-ready.json',{'ready':True,'gateway_ready_sha256':sha(cut/'gateway-ready.json'),'maintenance_sha256':sha(product/'maintenance-consumption.json'),'token_sha256':ready['token_sha256'],'pack_sha256':sha(product/'current-role-pack.json')});return
    if phase!='complete':raise ValueError('phase')
    follow=load(Path(proof['follow_root'])/'follow-receipt.json')
    if follow['worker_exit']!=0 or follow['current_generation']!=ready['source_generation'] or not follow['cuts']:
        raise ValueError('autonomous_followup')
    for path in proof['zero_exits']:
        if Path(path).read_text().strip()!='0':raise ValueError('lifecycle_raw_exit')
    oracle=load(next(cut.glob('*-oracle.stdout')))
    if oracle['generation']!=ready['source_generation'] or len(oracle['tables'])!=5 or any(t['different_cells']!=0 for t in oracle['tables'].values()):
        raise ValueError('independent_oracle')
    for changed in proof['changed_cuts']:
        original=Path(changed);req=load(original/'request.json');loaded=load(original/'load.json');acked=load(original/'ack.json')
        if not loaded.get('source_committed_at') or acked['task_state']!='succeeded' or not acked['acknowledged'] or acked['applied_pos']!=loaded['applied_pos'] or loaded['source_epoch']!=load(cut/'native/load.json')['source_epoch']:
            raise ValueError('changed_cut_ACK')
        stream=Path(req['entries'][0]['snapshot_arrow']['path']).parent/'worker-stream.ndjson'
        txns=[json.loads(line) for line in stream.read_bytes().splitlines() if json.loads(line).get('kind')=='transaction']
        final=[t for t in txns if str(t['applied_pos'])==loaded['applied_pos']]
        if len(final)!=1 or not final[0]['source_commit_timestamp'] or {row['op'] for row in final[0]['rows']}!={'upsert','delete'}:
            raise ValueError('actual_IUD_frames')
    standard=load(product/'current-oos-receipt.json')
    if standard['source_generation']!=ready['source_generation'] or standard['current_effective_sha256']!=sha(product/'current-effective-oos.json') or any(sha(product/'current-oos'/name)!=digest for name,digest in standard['files'].items()):raise ValueError('current_YAML_Ontology')
    first,second=[load(out/name/'verdict.json') for name in ['dialogue1','dialogue2']]
    for v in [first,second]:
        if v['outcome']!='verified' or v['selection']!='random' or not v['counts_as_random'] or len(v['query_ids'])<1:
            raise ValueError('random_dialogue')
    if set(first['semantic_ids']) & set(second['semantic_ids']):raise ValueError('random_repeated_semantics')
    cancel=load(proof['caller_receipt'])
    if cancel['same_token_sha256']!=ready['token_sha256'] or cancel['cancel_response']['outcome']!='sent':raise ValueError('caller_cancel')
    save(out/'whole.json',{'protocol':'oo.mysql.full-reference/v1','full_product_chain_complete':True,'periodic_insert_update_delete_verified':True,'recovery_verified':True,'product_receipt':'product/product-receipt.json','dialogue_verdict':'dialogue2/verdict.json','fixture_only':True,'production_ready':False,'source_id':current['source_id'],'source_generation':current['source_generation'],'pack_digest':current['pack_digest'],'same_token_sha256':ready['token_sha256'],'evidence_sha256':{path:sha(path) for path in [*proof['zero_exits'],proof['caller_receipt'],str(Path(proof['follow_root'])/'follow-receipt.json'),str(cut/'gateway-ready.json'),str(cut/'release-status.json')]},'random_dialogues':[{'path':name+'/verdict.json','sha256':sha(out/name/'verdict.json')} for name in ['dialogue1','dialogue2']]})


if __name__=='__main__':run(*sys.argv[1:])
