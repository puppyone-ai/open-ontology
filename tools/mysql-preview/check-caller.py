#!/usr/bin/env python3
"""Prove current CLI readiness, permission refusal and accepted-query cancellation."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import time


def run(config,out):
    c=json.loads(Path(config).read_bytes());schema=c.get('schema','native_mysql_20261012');assert schema.replace('_','').isalnum();out=Path(out);out.mkdir(mode=0o700,parents=True,exist_ok=False)
    flags=['--json','--auth',c['auth'],'--token-file',c['gateway_token_file'],'--gateway',c['gateway'],'--transport',c['transport']]
    def record(name,p):
        for suffix,data in [('stdout',p.stdout),('stderr',p.stderr),('exit',(str(p.returncode)+'\n').encode())]:
            (out/(name+'.'+suffix)).write_bytes(data)
        return json.loads(p.stdout)
    def call(name,args):
        p=subprocess.run([c['oo_cli'],*args,*flags],capture_output=True,timeout=40)
        return p,record(name,p)
    p,ready=call('freshness',['freshness'])
    assert p.returncode==0 and ready['publication']=='live' and ready['sources'] and ready['freshness']
    assert not any(w['code']=='STALE_DATA' for w in ready['warnings'])
    for name,sql,code in [('internal','SELECT _oo_pos FROM '+schema+'.value_probe','COLUMN_NOT_ALLOWED'),('unselected','SELECT * FROM oo_c1.order_hdr','RELATION_NOT_FOUND')]:
        p,d=call('deny-'+name,['query',sql]);assert p.returncode!=0 and d['code']==code,d
    wal=Path(c['independent_gateway_audit_file']);before=set(wal.glob('*-admit.wal'))
    sql='SELECT COUNT(*) AS pairs FROM '+schema+'.tracking_events a CROSS JOIN '+schema+'.tracking_events b'
    query=subprocess.Popen([c['oo_cli'],'query',sql,'--timeout-ms','30000',*flags],stdout=subprocess.PIPE,stderr=subprocess.PIPE)
    deadline=time.monotonic()+10;found=None
    while time.monotonic()<deadline:
        for path in set(wal.glob('*-admit.wal'))-before:
            try:d=json.loads(path.read_text())
            except json.JSONDecodeError:continue
            if d['original_sha256']==hashlib.sha256(sql.encode()).hexdigest():
                found=d;break
        if found:break
        if query.poll() is not None:break
        time.sleep(.02)
    if not found:
        raw,err=query.communicate(timeout=35)
        record('long-query',subprocess.CompletedProcess([],query.returncode,raw,err))
        raise RuntimeError('accepted_query_not_observed')
    p,cancel=call('cancel',['cancel',found['query_id']]);assert p.returncode==0 and cancel.get('outcome')=='sent' and cancel.get('audit')=='durable',cancel
    raw,err=query.communicate(timeout=35);answer=record('long-query',subprocess.CompletedProcess([],query.returncode,raw,err))
    assert query.returncode!=0 and answer.get('query_id')==found['query_id'] and answer.get('code')=='TIMEOUT' and answer.get('message')=='query canceled' and answer.get('hint')=='sqlstate 57014',answer
    complete=wal/(found['query_id']+'-complete.wal');assert complete.is_file()
    closed=json.loads(complete.read_text());assert closed['status']=='failed' and closed['rollback'] is True,closed
    p,after=call('after-cancel',['query','SELECT COUNT(*) AS n FROM '+schema+'.hubs']);assert p.returncode==0 and after['rows']
    (out/'receipt.json').write_text(json.dumps({'protocol':'oo.mysql.current-cli-permissions-cancel/v1','query_id':found['query_id'],'cancel_response':cancel,'complete_wal_sha256':hashlib.sha256(complete.read_bytes()).hexdigest(),'refusals':['COLUMN_NOT_ALLOWED','RELATION_NOT_FOUND'],'subsequent_query_id':after['query_id'],'same_token_sha256':hashlib.sha256(Path(c['gateway_token_file']).read_bytes()).hexdigest()},sort_keys=True)+'\n')


if __name__=='__main__':
    run(*sys.argv[1:])
