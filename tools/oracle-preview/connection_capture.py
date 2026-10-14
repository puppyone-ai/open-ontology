#!/usr/bin/env python3
"""Configurable authorized Oracle metadata/sample capture; no typed import claim."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys

IMAGE = 'docker.io/gvenzl/oracle-free@sha256:cdc5b3a3685da4f1785daeee24f74de8a9768b7629192ea9abdaeeea343d12a8'
IDENT = re.compile(r'[A-Za-z][A-Za-z0-9_]{0,127}\Z')

def prepare(config):
    if config['type'] != 'oracle': raise ValueError('engine')
    for key in ('database', 'user'):
        if not IDENT.fullmatch(config[key]): raise ValueError('connection identifier')
    if not re.fullmatch(r'[a-zA-Z0-9][a-zA-Z0-9.-]{0,252}', config['host']): raise ValueError('host')
    if type(config['port']) is not int or not 1 <= config['port'] <= 65535: raise ValueError('port')
    secret = Path(config['password_file'])
    meta = secret.lstat()
    if not secret.is_absolute() or not stat.S_ISREG(meta.st_mode) or meta.st_uid != os.getuid() or meta.st_mode & 0o077:
        raise ValueError('private secret')
    value = secret.read_text().strip('\r\n')
    # Existing authorized customer reference contains username/password JSON.
    if value.startswith('{'):
        credentials = json.loads(value)
        if credentials['username'] != config['user']: raise ValueError('secret identity mismatch')
        value = credentials['password']
    if not value or any(c in value for c in '\r\n\0'): raise ValueError('password shape')
    descriptor = f"(DESCRIPTION=(CONNECT_TIMEOUT=10)(TRANSPORT_CONNECT_TIMEOUT=10)(ADDRESS=(PROTOCOL=TCP)(HOST={config['host']})(PORT={config['port']}))(CONNECT_DATA=(SERVICE_NAME={config['database']})))"
    sql = '''SET ECHO OFF VERIFY OFF FEEDBACK OFF HEADING OFF PAGESIZE 0 LINESIZE 32767
WHENEVER OSERROR EXIT FAILURE
WHENEVER SQLERROR EXIT SQL.SQLCODE
'''
    sql += 'CONNECT ' + config['user'] + '/"' + value.replace('"', '""') + '"@"' + descriptor + '"\n'
    sql += '''SELECT 'CONNECTED|' || SYS_CONTEXT('USERENV','SESSION_USER') FROM dual;
COLUMN oo_owner NEW_VALUE selected_owner NOPRINT
COLUMN oo_table NEW_VALUE selected_table NOPRINT
SELECT owner AS oo_owner, table_name AS oo_table FROM (SELECT owner,table_name FROM all_tables WHERE owner NOT IN ('SYS','SYSTEM','XDB','MDSYS','CTXSYS','WMSYS','DBSNMP','OUTLN') AND REGEXP_LIKE(owner,'^[A-Z][A-Z0-9_]*$') AND REGEXP_LIKE(table_name,'^[A-Z][A-Z0-9_]*$') ORDER BY CASE WHEN owner=USER THEN 0 ELSE 1 END,owner,table_name) WHERE ROWNUM <= 1;
SELECT 'COLUMN|' || column_name || '|' || data_type || '|' || data_precision || '|' || data_scale || '|' || nullable FROM all_tab_columns WHERE owner='&selected_owner' AND table_name='&selected_table' ORDER BY column_id;
SELECT * FROM &selected_owner..&selected_table WHERE ROWNUM <= 5;
SELECT 'SAMPLE_DONE' FROM dual;
EXIT
'''
    return sql, value

def run(config, out):
    sql, password = prepare(config)
    out.mkdir(mode=0o700, parents=True, exist_ok=False)
    cmd = ['docker','run','--rm','--pull','never','--platform','linux/arm64','--label','oo.owner=oracle-connection-whole','--label','oo.kind=proof-runner','-i','--network','bridge','--read-only','--memory','256m','--cpus','0.5','--tmpfs','/tmp:rw,nosuid,nodev,size=32m','--cap-drop','ALL','--security-opt','no-new-privileges','--entrypoint','/opt/oracle/product/26ai/dbhomeFree/bin/sqlplus',IMAGE,'-s','/nolog']
    try:
        result = subprocess.run(cmd, input=sql.encode(), capture_output=True, timeout=35)
        code, stdout, stderr = result.returncode, result.stdout, result.stderr
    except subprocess.TimeoutExpired as error:
        code, stdout, stderr = 124, error.stdout or b'', error.stderr or b''
    # Raw sample is private and password echoes are redacted even there.
    for name, raw in [('stdout',stdout),('stderr',stderr)]:
        p=out/name; p.write_bytes(raw.replace(password.encode(),b'[redacted]')); p.chmod(0o600)
    (out/'exit').write_text(str(code)+'\n'); (out/'exit').chmod(0o600)
    raw=stdout+stderr
    receipt={'protocol':'oo.oracle.connection-capture/v1','endpoint':{k:config[k] for k in ('type','database','host','port','user')},'raw_exit':code,'connected':b'CONNECTED|' in raw,'sample_completed':code==0 and b'SAMPLE_DONE' in raw,'sample_hard_limit':5,'source_dml_ddl':0,'account_enumeration':False,'cdc':'off','typed_native_import':'not_run','s0_s10':'not_run','publication':'not_ready','errors':[x.decode(errors='replace').replace(password,'[redacted]') for x in re.findall(rb'(?:ORA|TNS|SP2)-\d+[^\r\n]*',raw)],'artifacts':{name:hashlib.sha256((out/name).read_bytes()).hexdigest() for name in ('stdout','stderr','exit')}}
    p=out/'receipt.json'; p.write_text(json.dumps(receipt,indent=2)+'\n');p.chmod(0o600)
    return 0 if receipt['sample_completed'] else 2

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--config',type=Path,required=True);parser.add_argument('--out',type=Path,required=True);args=parser.parse_args()
    try: sys.exit(run(json.loads(args.config.read_text()),args.out))
    except (ValueError,KeyError,OSError): print('oracle connection capture refused',file=sys.stderr);sys.exit(2)
