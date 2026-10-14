#!/usr/bin/env python3
"""Approved Oracle objects → dictionary-derived typed Arrow and C5 witness."""
import argparse
from decimal import Decimal
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys
import connection_capture as connection
import current_capture
import c5_capture
import c5_witness as witness

IDENT = connection.IDENT
ENCODER = '/t/r194/debug/oo-c5-arrow'
RUST_IMAGE='docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5'

def column_spec(col):
    kind=col['data_type']; precision=col['precision']; scale=col['scale']
    result={'name':col['name'],'nullable':col['nullable']=='Y'}
    if kind=='NUMBER' and precision is not None and scale is not None and 1<=precision<=38 and 0<=scale<=precision:
        result.update(type='decimal128',precision=precision,scale=scale)
    elif kind=='DATE': result['type']='timestamp_us'
    elif kind in ('CHAR','VARCHAR2','NCHAR','NVARCHAR2'): result['type']='utf8'
    elif kind=='NUMBER' and precision is None: result['type']='utf8'
    elif re.fullmatch(r'TIMESTAMP\([0-9]\)( WITH TIME ZONE)?',kind): result['type']='utf8'
    elif kind=='RAW': result['type']='binary'
    else: raise ValueError('unsupported exact Oracle type: '+kind)
    return result

def projection(col):
    name='"'+col['name']+'"';kind=col['data_type']
    if kind=='NUMBER': expression=f"TO_CHAR({name},'TM9','NLS_NUMERIC_CHARACTERS=''.,''')"
    elif kind=='DATE': expression=f"CASE WHEN TO_CHAR({name},'BC','NLS_DATE_LANGUAGE=American')='BC' THEN 'unsupported-date-era' ELSE TO_CHAR({name},'YYYY-MM-DD\"T\"HH24:MI:SS') END"
    elif kind=='RAW': expression=f'RAWTOHEX({name})'
    elif kind.startswith('TIMESTAMP('):
        fmt='YYYY-MM-DD\"T\"HH24:MI:SS.FF9'+(' TZH:TZM' if kind.endswith(' WITH TIME ZONE') else '')
        expression=f"TO_CHAR({name},'{fmt}')"
    else: expression=f'RAWTOHEX(UTL_RAW.CAST_TO_RAW({name}))'
    return "'"+col['name']+"' VALUE "+expression

def sql_run(cfg, body, out, name):
    connect,password=connection.prepare(cfg)
    # Retain only the already validated connection prelude, never auto-select a table.
    prelude=connect.split("SELECT 'CONNECTED|",1)[0]+'SET LONG 200000 LONGCHUNKSIZE 200000 TRIMSPOOL ON WRAP OFF\n'
    argv=['docker','run','--rm','--pull','never','--platform','linux/arm64','--label','oo.owner=oracle-connection-whole','--label','oo.kind=proof-runner','-i','--network',cfg['network'],'--read-only','--memory','256m','--cpus','0.5','--tmpfs','/tmp:rw,nosuid,nodev,size=32m','--cap-drop','ALL','--security-opt','no-new-privileges','--entrypoint','/opt/oracle/product/26ai/dbhomeFree/bin/sqlplus',connection.IMAGE,'-s','/nolog']
    proc=subprocess.run(argv,input=(prelude+body+'\nEXIT\n').encode(),capture_output=True,timeout=45)
    directory=out/'run/sql';directory.mkdir(parents=True,exist_ok=True)
    (directory/(name+'.sql')).write_text(body+'\nEXIT\n')
    for suffix,raw in [('stdout',proc.stdout),('stderr',proc.stderr)]: (directory/(name+'.'+suffix)).write_bytes(raw.replace(password.encode(),b'[redacted]'))
    (directory/(name+'.exit')).write_text(str(proc.returncode)+'\n')
    if proc.returncode: raise ValueError('source read failed; raw exit retained')
    return proc.stdout.decode()

def json_lines(text,prefix):
    return [json.loads(line.strip()[len(prefix):]) for line in text.splitlines() if line.strip().startswith(prefix)]

def run(cfg,out,generation=1,previous=None):
    # Validate complete authorization before any private secret or source access.
    selections=cfg['selections'];approved=set(cfg['approved_objects'])
    if not selections or not 1<=cfg['hard_limit']<=8: raise ValueError('authorization budget')
    owners=set()
    for selection in selections:
        obj=selection['object'];parts=obj.split('.')
        if obj not in approved or len(parts)!=2 or any(not IDENT.fullmatch(p) for p in parts): raise ValueError('approved object')
        if not IDENT.fullmatch(selection['key']) or not re.fullmatch('[a-z][a-z0-9_]{0,62}',selection['source_id']): raise ValueError('key/source identity')
        owners.add(parts[0])
    if len(owners)!=1: raise ValueError('one authorized schema per capture')
    scn=Path(cfg['scn_file']).read_text().strip()
    if not scn.isdigit() or generation<1 or (generation>1 and previous is None): raise ValueError('published SCN/parent')
    out.mkdir(mode=0o700,parents=True,exist_ok=False)
    charset=sql_run(cfg,"SELECT 'CHARSET|' || value FROM nls_database_parameters WHERE parameter='NLS_CHARACTERSET';\nSELECT 'USER|' || SYS_CONTEXT('USERENV','SESSION_USER') FROM dual;\nSELECT 'VERSION|' || banner FROM v$version WHERE banner LIKE 'Oracle%';",out,'charset')
    if not any(line.strip()=='USER|'+cfg['user'].upper() for line in charset.splitlines()): raise ValueError('actual reader identity mismatch')
    if not any(line.strip()=='CHARSET|AL32UTF8' for line in charset.splitlines()): raise ValueError('exact source charset unsupported')
    profile={'columns':{},'declared_primary_keys':{},'cdc_default':'off','source_charset':'AL32UTF8','source_versions':[line.strip()[8:] for line in charset.splitlines() if line.strip().startswith('VERSION|')],'periods':c5_capture.resolve_periods(cfg.get('periods'))}
    specs=[]
    for selection in selections:
        owner,table=selection['object'].split('.')
        dictionary_sql=f"SELECT 'COL|' || JSON_OBJECT('table' VALUE table_name,'position' VALUE column_id,'name' VALUE column_name,'data_type' VALUE data_type,'precision' VALUE data_precision,'scale' VALUE data_scale,'char_length' VALUE char_length,'nullable' VALUE nullable NULL ON NULL) FROM all_tab_columns WHERE owner='{owner}' AND table_name='{table}' ORDER BY column_id;\nSELECT 'KEY|' || cc.column_name FROM all_constraints c JOIN all_cons_columns cc ON c.owner=cc.owner AND c.constraint_name=cc.constraint_name WHERE c.owner='{owner}' AND c.table_name='{table}' AND c.constraint_type='P' ORDER BY cc.position;"
        text=sql_run(cfg,dictionary_sql,out,'dictionary-'+table)
        cols=json_lines(text,'COL|')
        if not cols or len(cols)>32 or [line.strip()[4:] for line in text.splitlines() if line.strip().startswith('KEY|')]!=[selection['key']]: raise ValueError('declared single key required')
        encoded=[column_spec(col) for col in cols]
        key=next(col for col in cols if col['name']==selection['key'])
        if key['data_type']!='NUMBER' or key['scale']!=0 or key['nullable']!='N': raise ValueError('integer C5 key required')
        for col in cols:
            col['flags']=['empty_string_is_null'] if col['data_type'] in ('CHAR','VARCHAR2','NCHAR','NVARCHAR2') else ['unconstrained_number'] if col['data_type']=='NUMBER' and col['precision'] is None else ['precision_loss'] if col['data_type'].startswith('TIMESTAMP(') else []
        if previous and cols!=json.loads((previous/'run/producer-profile.json').read_text())['columns'][table]: raise ValueError('schema changed; maintained approval required')
        profile['columns'][table]=cols
        profile['declared_primary_keys'][table]=[selection['key']]
        query="SELECT 'ROW|' || JSON_OBJECT("+','.join(projection(col) for col in cols)+f" NULL ON NULL RETURNING CLOB) FROM {owner}.{table} AS OF SCN {scn} ORDER BY \"{selection['key']}\" FETCH FIRST {cfg['hard_limit']+1} ROWS ONLY;"
        rows=json_lines(sql_run(cfg,query,out,'pull-'+table),'ROW|')
        for row in rows:
            for col in cols:
                if col['data_type']=='NUMBER' and col['precision'] is not None and row[col['name']] is not None:
                    row[col['name']]=format(Decimal(row[col['name']]),'f')
                if col['data_type'] in ('CHAR','VARCHAR2','NCHAR','NVARCHAR2') and row[col['name']] is not None:
                    row[col['name']]=bytes.fromhex(row[col['name']]).decode('utf-8')
        if len(rows)>cfg['hard_limit']: raise ValueError('full snapshot exceeds row budget; refuse truncation')
        cells={str(int(row[selection['key']])):row for row in rows}
        if len(cells)!=len(rows): raise ValueError('duplicate C5 key')
        arrow=out/'run/arrow';arrow.mkdir(parents=True,exist_ok=True)
        request=arrow/(table+'.json');request.write_text(json.dumps({'columns':encoded,'rows':rows}))
        filename=table.lower()+'-gen-'+str(generation)+'.arrow'
        command=['docker','run','--rm','--pull','never','--platform','linux/arm64','--label','oo.owner=oracle-connection-whole','--label','oo.kind=proof-runner','--network','none','--memory','256m','--cpus','0.5','--cap-drop','ALL','--security-opt','no-new-privileges','--user','501:20','-v','oo-dl01w1-04-c5-target:/t:ro','-v',str(out)+':'+str(out),'--entrypoint','/bin/sh',RUST_IMAGE,'-c','test "$(sha256sum /t/r194/debug/oo-c5-arrow | cut -d" " -f1)" = '+current_capture.ENCODER_SHA+' && exec '+ENCODER+' encode "$1" "$2"','sh',str(request),str(arrow/filename)]
        proc=subprocess.run(command,capture_output=True,timeout=60)
        for suffix,raw in [('stdout',proc.stdout),('stderr',proc.stderr)]: (arrow/(filename+'.'+suffix)).write_bytes(raw)
        (arrow/(filename+'.exit')).write_text(str(proc.returncode)+'\n')
        if proc.returncode: raise ValueError('fixed encoder failed')
        decoded=arrow/(filename+'.decoded.json')
        decode_command=command.copy()
        decode_command[-4]=decode_command[-4].replace(' encode ', ' decode ')
        decode_command[-2:]=[str(arrow/filename),str(decoded)]
        decoded_proc=subprocess.run(decode_command,capture_output=True,timeout=60)
        (arrow/(filename+'.decode.stdout')).write_bytes(decoded_proc.stdout)
        (arrow/(filename+'.decode.stderr')).write_bytes(decoded_proc.stderr)
        (arrow/(filename+'.decode.exit')).write_text(str(decoded_proc.returncode)+'\n')
        if decoded_proc.returncode: raise ValueError('native Arrow decode failed')
        got=json.loads(decoded.read_text())['rows']
        if len(got)!=len(rows): raise ValueError('native Arrow row count')
        for expected,actual in zip(rows,got):
            for col in cols:
                name=col['name']
                equal=(Decimal(expected[name])==Decimal(actual[name])) if col['data_type']=='NUMBER' and col['precision'] is not None and expected[name] is not None and actual[name] is not None else expected[name]==actual[name]
                if not equal: raise ValueError('native Arrow typed cell difference')
        old=None if previous is None else c5_capture.load_generation(previous/'run/shadow'/table,generation-1)
        result=c5_capture.apply_snapshot(out/'run/shadow'/table,old,generation,'configured-oracle-scn-'+scn,[c['name'] for c in cols],[c['name'] for c in cols],cells,cfg['hard_limit'])
        if not result['committed']: raise ValueError(result['state'])
        specs.append({'table':table,'generation':generation,'arrow':filename,'source_id':selection['source_id']})
    (out/'run/producer-profile.json').write_text(json.dumps(profile,indent=2)+'\n')
    return seal(cfg, out, generation, owners, specs, scn)

def seal(cfg, out, generation, owners, specs, scn):
    witness.CAPTURE=out;witness.ARROW_DIR=out/'run/arrow';witness.PROFILE_PATH=out/'run/producer-profile.json';witness.SCHEMA=next(iter(owners));witness.TARGET_ID=cfg['target_id'];witness.SECRET_REF=cfg['secret_ref'];witness.BUDGET_LIMIT=cfg['hard_limit']
    driver=Path('/tmp/oo-dl01w1-04-native-source-cli/sqlplus').resolve()
    if hashlib.sha256(driver.read_bytes()).hexdigest()!=current_capture.SQLPLUS_SHA: raise ValueError('SQLPlus driver changed')
    desc={'kind':'command-line','name':'sqlplus','version':'23.26.3.0.0','digest':current_capture.SQLPLUS_SHA,'bytes':driver.stat().st_size,'path':str(driver),'digest_is_file':True}
    # Read actual current session/object grants rather than borrowing historical site proof.
    grants=sql_run(cfg,"SELECT 'PRIV|' || privilege FROM session_privs;\nSELECT 'GRANT|' || table_schema || '.' || table_name || '|' || privilege FROM all_tab_privs WHERE grantee=USER;",out,'permissions')
    if not any(line.strip()=='PRIV|CREATE SESSION' for line in grants.splitlines()): raise ValueError('CREATE SESSION not observed')
    permissions=[{'name':'CREATE SESSION','status':'known','granted':True}]
    for sel in cfg['selections']:
        owner,table=sel['object'].split('.')
        for privilege in ('SELECT','FLASHBACK'):
            if not any(line.strip()=='GRANT|'+sel['object']+'|'+privilege for line in grants.splitlines()): raise ValueError('required scoped grant not observed')
            permissions.append({'name':privilege,'status':'known','granted':True,'scope':{'database':owner,'table':table}})
    for spec in specs:
        built=witness.build_inputs(spec,desc,permissions)
        if built['unsupported']: raise ValueError('unsupported native Arrow schema')
        witness.write_table(out/'witness'/spec['source_id'],built,driver)
    (out/'capture.json').write_text(json.dumps({'protocol':'oo.oracle.configured-capture/v1','actual_scn':scn,'generation':generation,'source_reader':cfg['user'],'source_dml_ddl':0,'tables':{s['table']:len(c5_capture.load_generation(out/'run/shadow'/s['table'],generation)['cells']) for s in specs},'reader_sql_files_sha256':{p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in (out/'run/sql').glob('*.sql')},'orchestration_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'cdc':'off','native_imported':False,'encoder_sha256':current_capture.ENCODER_SHA,'driver_sha256':current_capture.SQLPLUS_SHA},indent=2)+'\n')
    return out

def seal_existing(cfg, out, generation):
    """Finish witness for a capture that already committed its shadow. Does not re-read rows."""
    if (out/'capture.json').is_file():
        return out
    profile=json.loads((out/'run/producer-profile.json').read_text())
    owners=set(); specs=[]
    for selection in cfg['selections']:
        owner,table=selection['object'].split('.')
        owners.add(owner)
        shadow=c5_capture.load_generation(out/'run/shadow'/table, generation)
        if shadow['generation']!=generation: raise ValueError('shadow generation')
        specs.append({'table':table,'generation':generation,'arrow':table.lower()+'-gen-'+str(generation)+'.arrow','source_id':selection['source_id']})
    scn=Path(cfg['scn_file']).read_text().strip()
    return seal(cfg, out, generation, owners, specs, scn)

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--config',type=Path,required=True);p.add_argument('--out',type=Path,required=True);p.add_argument('--generation',type=int,default=1);p.add_argument('--previous',type=Path);a=p.parse_args()
    try:run(json.loads(a.config.read_text()),a.out.resolve(),a.generation,a.previous.resolve() if a.previous else None)
    except (ValueError,KeyError,OSError,subprocess.TimeoutExpired) as e: print('oracle capture refused: '+str(e),file=sys.stderr);sys.exit(2)
