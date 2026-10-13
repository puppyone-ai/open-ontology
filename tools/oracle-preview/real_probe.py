#!/usr/bin/env python3
"""Single authorized customer connection; private metadata/sample never printed."""
import json, os, re, subprocess, time
from pathlib import Path

def main():
    cfg=json.loads(Path('/tmp/oo-customer-oracle-20261012/target.json').read_text())
    creds=json.loads(Path(cfg['credential_file']).read_text())
    user=creds['username']; password=creds['password']
    if user != 'aiagenttest': raise ValueError('only supplied test identity allowed')
    private=Path('/tmp/oo-customer-oracle-20261012/probe'); private.mkdir(mode=0o700,exist_ok=True)
    evidence=Path('evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/oracle-product/native-real-20261012'); evidence.mkdir(parents=True,exist_ok=True)
    descriptor=f"(DESCRIPTION=(CONNECT_TIMEOUT=10)(TRANSPORT_CONNECT_TIMEOUT=10)(ADDRESS=(PROTOCOL=TCP)(HOST={cfg['host']})(PORT={cfg['port']}))(CONNECT_DATA=(SERVICE_NAME={cfg['database_identifier']})))"
    sql=f'''SET ECHO OFF VERIFY OFF FEEDBACK OFF HEADING OFF PAGESIZE 0 LINESIZE 32767
WHENEVER OSERROR EXIT FAILURE
WHENEVER SQLERROR EXIT SQL.SQLCODE
CONNECT {user}/"{password.replace(chr(34),chr(34)*2)}"@"{descriptor}"
SELECT 'CONNECTED|' || SYS_CONTEXT('USERENV','SESSION_USER') || '|' || SYS_CONTEXT('USERENV','SERVICE_NAME') FROM dual;
COLUMN oo_owner NEW_VALUE selected_owner NOPRINT
COLUMN oo_table NEW_VALUE selected_table NOPRINT
SELECT owner AS oo_owner,table_name AS oo_table FROM (SELECT owner,table_name FROM all_tables WHERE owner NOT IN ('SYS','SYSTEM','XDB','MDSYS','CTXSYS','WMSYS','DBSNMP','OUTLN') AND REGEXP_LIKE(owner,'^[A-Z][A-Z0-9_]*$') AND REGEXP_LIKE(table_name,'^[A-Z][A-Z0-9_]*$') ORDER BY CASE WHEN owner=USER THEN 0 ELSE 1 END,owner,table_name) WHERE ROWNUM <= 1;
SELECT 'COLUMN|' || column_name || '|' || data_type || '|' || data_precision || '|' || data_scale || '|' || nullable FROM all_tab_columns WHERE owner='&selected_owner' AND table_name='&selected_table' ORDER BY column_id;
SELECT * FROM &selected_owner..&selected_table WHERE ROWNUM <= 5;
EXIT
'''
    started=time.time()
    try:
        p=subprocess.run(['docker','run','--rm','-i','--name','oo-dl01w1-04-real-probe','--network','bridge','--read-only','--tmpfs','/tmp:rw,nosuid,nodev,size=32m','--cap-drop','ALL','--security-opt','no-new-privileges','--entrypoint','/opt/oracle/product/26ai/dbhomeFree/bin/sqlplus','docker.io/gvenzl/oracle-free@sha256:cdc5b3a3685da4f1785daeee24f74de8a9768b7629192ea9abdaeeea343d12a8','-s','/nolog'],input=sql.encode(),stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=35)
        raw=p.stdout+p.stderr
        # Private raw includes any echoed connection error and is never committed.
        dest=private/('connection-'+str(time.time_ns())+'.raw'); dest.write_bytes(raw); dest.chmod(0o600)
        errors=re.findall(rb'(?:ORA|TNS|SP2)-\d+[^\r\n]*',raw)
        errors=[e.decode('utf-8','replace').replace(password,'[redacted]') for e in errors]
        connected=b'CONNECTED|' in raw
        result={'protocol':'oo.oracle.real-probe/v1','endpoint':f"{cfg['host']}:{cfg['port']}/{cfg['database_identifier']}",'identity':'aiagenttest','connect_data':'SERVICE_NAME','exit':p.returncode,'connected':connected,'errors':errors,'elapsed_seconds':round(time.time()-started,3),'prepared_readonly_statements':['SELECT dual session identity','SELECT all_tables metadata ROWNUM <= 1'],'metadata_attempted':connected,'sample_completed':connected and p.returncode==0,'source_dml_ddl':0,'source_sample_rows':None if connected else 0,'sample_hard_limit':5,'raw_customer_data':'private only','private_raw_path':str(dest)}
    except subprocess.TimeoutExpired:
        result={'endpoint':f"{cfg['host']}:{cfg['port']}/{cfg['database_identifier']}",'connected':False,'error':'client timeout after 35s','source_dml_ddl':0,'source_sample_rows':0}
    (evidence/('connection-'+str(time.time_ns())+'.json')).write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps(result))
if __name__=='__main__': main()
