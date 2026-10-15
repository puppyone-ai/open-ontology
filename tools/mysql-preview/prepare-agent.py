#!/usr/bin/env python3
"""Freeze independent owned-source truth and new random Agent inputs after readiness."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'agent-product-verifier'))
from dialogue import validate_bank


def load(p):
    return json.loads(Path(p).read_bytes())


def save(p,d):
    with Path(p).open('x') as f:
        os.chmod(p,0o600)
        json.dump(d,f,sort_keys=True,separators=(',',':'));f.write('\n')


def run(config,out):
    c=load(config);out=Path(out);out.mkdir(mode=0o700,parents=True,exist_ok=False)
    ready=load(c['gateway_ready'])
    if not ready['ready'] or ready['source_generation']!=c['identity']['source_generation'] or ready['role']!=c['identity']['role']:
        raise ValueError('current_ready_identity')
    if hashlib.sha256(Path(c['role_pack']).read_bytes()).hexdigest()!=c['identity']['pack_sha256']:
        raise ValueError('current_pack')
    base=['docker','exec',c['source_container'],'mysql','--defaults-extra-file='+c['source_client_file'],'--batch','--raw','--skip-column-names','-e']
    def sql(label,text):
        p=subprocess.run(base+[text],capture_output=True)
        for ext,b in [('stdout',p.stdout),('stderr',p.stderr),('exit',(str(p.returncode)+'\n').encode())]:
            f=out/(label+'.'+ext);f.write_bytes(b);f.chmod(0o600)
        if p.returncode:
            raise RuntimeError('source_gold_query')
        return p.stdout.decode().strip()
    before=sql('source-before','SELECT @@GLOBAL.gtid_executed')
    if before!=c['gtid']:
        raise ValueError('source_gtid')
    bank=[];gold={}
    for index,q in enumerate(c['questions']):
        row=json.loads(sql('gold-'+str(index),q['source_sql']))
        if not isinstance(row,dict) or any(not isinstance(v,str) for v in row.values()):
            raise ValueError('exact_named_source_facts')
        bank.append({k:q[k] for k in ['semantic_id','query_class','prompt','truth_key']})
        gold[q['truth_key']]={'expected_named_facts':row}
    if sql('source-after','SELECT @@GLOBAL.gtid_executed')!=before:
        raise ValueError('source_changed_during_gold')
    validate_bank(bank,gold)
    save(out/'bank.json',bank);save(out/'gold.json',gold)
    if c['dialogue'].get('model','gpt-6.1-sol') not in ['gpt-6.1-sol','grok-4.7']:raise ValueError('approved_model')
    c['dialogue'].setdefault('model','gpt-6.1-sol')
    c['dialogue'].update(question_bank=str(out/'bank.json'),independent_truth_file=str(out/'gold.json'),identity=c['identity'])
    c['dialogue'].pop('questions',None);c['dialogue'].pop('seed',None)
    save(out/'dialogue-config.json',c['dialogue'])
    save(out/'source-gold-receipt.json',{'protocol':'oo.mysql.independent-random-gold/v1','source_id':c['identity']['source_id'],'gtid':before,'source_connection_reference':{'container':c['source_container'],'private_client_file':c['source_client_file']},'source_generation':c['identity']['source_generation'],'pack_sha256':c['identity']['pack_sha256'],'gateway_ready_sha256':hashlib.sha256(Path(c['gateway_ready']).read_bytes()).hexdigest(),'gold_sha256':hashlib.sha256((out/'gold.json').read_bytes()).hexdigest(),'bank_sha256':hashlib.sha256((out/'bank.json').read_bytes()).hexdigest(),'source_sql_sha256':hashlib.sha256(json.dumps([q['source_sql'] for q in c['questions']],separators=(',',':')).encode()).hexdigest(),'gold_values_sent_to_model':False})


if __name__=='__main__':
    run(*sys.argv[1:])
