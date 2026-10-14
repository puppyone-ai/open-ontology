#!/usr/bin/env python3
"""Configured Doris native capture plus independent full-cell source oracle.

The reviewed typed seven-table schema is required. No source writes or historical
capture bytes are used. Downstream import/S0-S10 are separate pinned stages.
"""
import argparse
import hashlib
import ipaddress
import json
import os
from pathlib import Path
import re
import subprocess

IMAGE = 'docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5'
TABLES = {
    'dup_events': 'event_id,kind,qty', 'agg_qty': 'sku,qty,note',
    'uniq_acct': 'id,balance,label', 'dec_exact': 'id,a18,b18,d38',
    'part_day': 'day,region,amount', 'str_key': 'k,note', 'nz_obs': 'id,qty',
}


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def save(path, value):
    with path.open('x') as f:
        os.chmod(path, 0o600)
        json.dump(value, f, indent=2)
        f.write('\n')


def require(ok):
    if not ok:
        raise ValueError('Doris capture contract refused')


def validate(c):
    require(c['type'] == 'doris')
    ipaddress.IPv4Address(c['host'])
    require(type(c['port']) is int and 1 <= c['port'] <= 65535)
    for key in ('database', 'user'):
        require(re.fullmatch(r'[A-Za-z0-9_]{1,64}', c[key]))
    require(re.fullmatch(r'secret://[A-Za-z0-9_./-]{1,128}', c['secret_ref']))
    require(Path(c['password_file']).is_absolute())
    require(Path(c['password_file']).is_file())
    require(Path(c['password_file']).stat().st_mode & 0o077 == 0)
    require(1000 <= c['period_ms'] <= 86400000)
    require(1 <= c['max_rows_per_table'] <= 1000000)
    require(digest(c['reader']) == c['reader_sha256'])
    require(Path(c['reader']).read_bytes()[:4] == b'\x7fELF')


def main():
    p = argparse.ArgumentParser()
    p.add_argument('--config', required=True)
    p.add_argument('--run', required=True)
    a = p.parse_args()
    os.umask(0o077)
    c = json.loads(Path(a.config).read_text())
    validate(c)
    root = Path(a.run).resolve()
    capture = root / 'capture'
    capture.mkdir(mode=0o700)
    command = ['docker', 'run', '--rm', '--network', c['network'], '--pull', 'never',
               '--platform', 'linux/arm64', '--memory', '512m',
               '--label', 'oo.owner=doris-connection-whole', '--label', 'oo.kind=source-reader',
               '-v', f"{c['reader']}:/opt/doris-reader:ro", '-v', f'{capture}:/evidence',
               '-v', f"{c['password_file']}:/secrets/reader.password:ro",
               '--entrypoint', '/opt/doris-reader', IMAGE,
               'capture-connection', '/evidence', '/secrets', c['host'], str(c['port']),
               c['database'], c['user'], str(c['period_ms']), 'current', str(c['max_rows_per_table'])]
    with (root / 'capture.stdout').open('xb') as out, (root / 'capture.stderr').open('xb') as err:
        try:
            result = subprocess.run(command, stdout=out, stderr=err, timeout=240)
        except subprocess.TimeoutExpired:
            (root / 'capture.exit').write_text('timeout\n')
            raise SystemExit('Doris capture deadline exceeded; cleanup requires Docker availability')
    (root / 'capture.exit').write_text(str(result.returncode) + '\n')
    if result.returncode:
        raise SystemExit('Doris native capture refused; raw exit retained')
    tick = json.loads((capture / 'current.json').read_text())
    require(tick['protocol'] == 'oo.doris.capture-tick/v1')
    require(tick['database'] == c['database'] and tick['product_reader_write_attempts'] == 0)
    require(tick['budget']['enforced'] is True and tick['budget']['max_rows_per_table'] == c['max_rows_per_table'])
    require({t['name'] for t in tick['tables']} == set(TABLES))
    # The independent wire client uses the actual configured endpoint, database,
    # user and secret file, and preserves SQL NULL versus numeric zero.
    client = root / 'oracle_client.py'
    client.write_text('''import json,os,pathlib,subprocess,sys
c=json.loads(pathlib.Path(sys.argv[1]).read_text())
env=os.environ.copy();env['MYSQL_PWD']=pathlib.Path('/secrets/password').read_text().strip()
for table,columns in c['oracle_tables'].items():
 print('@@'+table,flush=True)
 r=subprocess.run(['mysql','-h',c['host'],'-P',str(c['port']),'-u',c['user'],'--database',c['database'],'--connect-timeout=15','--default-character-set=utf8mb4','-N','-B','--raw','-e','SELECT '+columns+' FROM '+table+' ORDER BY '+columns+' LIMIT '+str(c['max_rows_per_table']+1)],env=env)
 if r.returncode:sys.exit(r.returncode)
''')
    os.chmod(client, 0o600)
    oracle_config = dict(c, oracle_tables=TABLES)
    save(root / 'oracle-config.json', oracle_config)
    oracle = ['docker', 'run', '--rm', '--network', c['network'], '--pull', 'never',
              '--platform', 'linux/arm64', '--memory', '512m',
              '--label', 'oo.owner=doris-connection-whole', '--label', 'oo.kind=source-oracle', '-v', f'{root}:/run:ro',
              '-v', f"{c['password_file']}:/secrets/password:ro", '--entrypoint', 'python3',
              c['client_image'], '/run/oracle_client.py', '/run/oracle-config.json']
    result = subprocess.run(oracle, capture_output=True, timeout=120)
    (root / 'oracle.stdout').write_bytes(result.stdout)
    (root / 'oracle.stderr').write_bytes(result.stderr)
    (root / 'oracle.exit').write_text(str(result.returncode) + '\n')
    require(result.returncode == 0)
    rows = {name: [] for name in TABLES}
    table = None
    for line in result.stdout.decode().splitlines():
        if line.startswith('@@'):
            table = line[2:]
        else:
            require(table in TABLES)
            rows[table].append('\x1f'.join('\\N' if cell == 'NULL' else cell for cell in line.split('\t')))
    actual = {t['name']: t['rows'] for t in tick['tables']}
    mismatches = [name for name in TABLES if sorted(rows[name]) != sorted(actual[name])]
    save(root / 'source-gold.json', {'sampled': False, 'rows': rows, 'mismatch_tables': mismatches,
                                   'source_capture_sha256': digest(capture / 'current.json')})
    require(not mismatches)
    artifacts = [p for p in capture.iterdir() if p.is_file()]
    save(root / 'capture-receipt.json', {
        'protocol': 'oo.doris.connection-capture/v1', 'fresh_source_capture': True,
        'connection': {k: c[k] for k in ('type', 'host', 'port', 'database', 'user', 'secret_ref')},
        'reader_sha256': c['reader_sha256'], 'source_typed_gold_equal': True,
        'reader_write_attempts': 0, 'capture': str(capture / 'current.json'),
        'artifacts': [{'path': str(p.relative_to(root)), 'sha256': digest(p)} for p in artifacts],
        'full_product_chain_complete': False})


if __name__ == '__main__':
    main()
