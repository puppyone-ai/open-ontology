#!/usr/bin/env python3
"""Actual configured ClickHouse read-only capture, with retained private exits."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess

import fresh_capture as f

IDENT = re.compile(r'^[A-Za-z_][A-Za-z0-9_]*$')
HOST = re.compile(r'^[A-Za-z0-9_.-]+$')

def main():
    p = argparse.ArgumentParser()
    p.add_argument('--config', type=Path, required=True)
    p.add_argument('--run', type=Path, required=True)
    a = p.parse_args()
    config = json.loads(a.config.read_text())
    source = config['source']
    for key in ('database', 'table'):
        if not IDENT.fullmatch(source[key]): raise ValueError('source identifier refused')
    if not HOST.fullmatch(source['host']) or type(source['port']) is not int or not 1 <= source['port'] <= 65535:
        raise ValueError('source endpoint refused')
    secret = Path(source['password_file']).resolve()
    if not secret.is_file() or secret.stat().st_mode & 0o077: raise ValueError('private source file required')
    if hashlib.sha256(Path(config['driver']).read_bytes()).hexdigest() != f.DRIVER_SHA:
        raise ValueError('fixed producer changed')
    out = a.run.resolve() / 'capture'
    out.mkdir(mode=0o700, exist_ok=False)
    (out/'reader.user').write_text(source['user']+'\n')
    endpoint = f"http://{source['host']}:{source['port']}"
    base = ['docker','run','--rm','--pull','never','--network',config['network'],
        '--mount',f'type=bind,source={out},target=/evidence',
        '--mount',f'type=bind,source={secret},target=/private/reader.password,readonly',
        '--mount',f'type=bind,source={config["driver"]},target=/bin/ch-reader,readonly',
        '--entrypoint','/bin/ch-reader',f.RUST_IMAGE]
    def capture(name, sql, arrow=False):
        cmd = base + ['arrow' if arrow else 'read','--endpoint',endpoint,
            '--user-file','/evidence/reader.user','--password-file','/private/reader.password',
            '--sql',sql,'--query-id',config['query_prefix']+name,
            '--row-cap',str(config['row_cap']),'--byte-cap',str(config['byte_cap']),
            '--timeout-ms',str(config['timeout_ms']),'--out',f'/evidence/{name}.json']
        cmd += ['--ipc-out',f'/evidence/{name}.arrow'] if arrow else ['--body-out',f'/evidence/{name}.body']
        r=subprocess.run(cmd,capture_output=True,timeout=120)
        for suffix,body in [('stdout',r.stdout),('stderr',r.stderr),('exit',f'{r.returncode}\n'.encode())]:
            (out/f'{name}.{suffix}').write_bytes(body)
        if r.returncode: raise RuntimeError(f'{name} refused; private raw retained')
    db, table = source['database'],source['table']
    where = f"database='{db}' AND table='{table}'"
    metadata = {'columns':f'SELECT name,type FROM system.columns WHERE {where} ORDER BY position FORMAT JSONEachRow',
        'parts':f'SELECT partition,name,rows,bytes_on_disk,modification_time FROM system.parts WHERE {where} AND active ORDER BY partition,name FORMAT JSONEachRow',
        'mutations':f'SELECT count() FROM system.mutations WHERE {where} AND is_done=0 FORMAT TSV',
        'uuid':f"SELECT toString(uuid) FROM system.tables WHERE database='{db}' AND name='{table}' FORMAT TSV"}
    for name,sql in metadata.items():capture(name,sql)
    if (out/'mutations.body').read_text().strip()!='0':raise RuntimeError('DELETES_LAGGED; no publication')
    columns=[json.loads(x) for x in (out/'columns.body').read_text().splitlines()]
    names=[x['name'] for x in columns]
    if names != config['columns'] or not all(IDENT.fullmatch(x) for x in names):raise ValueError('approved source schema changed')
    capture('full',f"SELECT {','.join(names)} FROM {db}.{table} FINAL ORDER BY {config['key']} FORMAT ArrowStream",True)
    capture('source-gold',f"SELECT reading_id,version,toString(amount) AS amount,note,code,code_or_null,toString(ts) AS ts,tags,labels,qty,label,wide_u64 FROM {db}.{table} FINAL ORDER BY {config['key']} FORMAT JSONEachRow")
    for name,sql in metadata.items():
        capture(name+'-after',sql)
        if (out/f'{name}.body').read_bytes()!=(out/f'{name}-after.body').read_bytes():raise RuntimeError('source changed during capture')
    artifacts={x.name:hashlib.sha256(x.read_bytes()).hexdigest() for x in out.iterdir() if x.is_file()}
    manifest={'protocol':'oo.clickhouse.connection-capture/v1','source':{k:source[k] for k in ('host','port','database','table','user')},
        'table_uuid':(out/'uuid.body').read_text().strip(),'current_source_read':True,'product_write_attempts':0,
        'capture_family':config['capture']['sync'],'consistency':'S0','cross_table_consistency':'B',
        'producer_sha256':f.DRIVER_SHA,'artifacts':artifacts,'whole_chain_complete':False}
    (a.run/'capture-manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')

if __name__=='__main__':main()
