#!/usr/bin/env python3
"""Capture a DBA-published actual SCN using only the approved fixture reader.

This adapter does not read the observer/DBA password. It does not import into
PG or mint leases. Captures and native admission inputs are source-owned.
"""
import argparse, hashlib, json, os, subprocess
from pathlib import Path
import c5_live as live
import c5_capture as capture
import c5_witness as witness

IMAGE='rust:1.94.0'
ENCODER_SHA='9b06acb3d873057efd0d2f2b431f129ab53061e7b19978f336930d1aad935d75'
SQLPLUS_SHA='aa962e50bc3f1bb2bf5d3b558c3e3f00d3826bb214c897b6024156aa2a1028a0'

def run(scn, out, generation=1, previous=None):
    if generation < 1 or (generation != 1 and previous is None): raise ValueError('actual acknowledged parent required')
    if not str(scn).isdigit(): raise ValueError('actual published SCN required')
    if out.exists(): raise ValueError('preserve existing occurrence; use durable saved request')
    out.mkdir(parents=True)
    sql=out/'run/sql'; sql.mkdir(parents=True)
    loaded=live.customer_input.load_customer(live.CUSTOMER,live.REGISTRY)
    password=Path(loaded['_password_file']).read_text().strip('\r\n')
    secrets=[password.encode()]
    dictionary=live.reader_dictionary(password,sql,secrets,'dictionary')
    approved={table:[c['name'] for c in cols] for table,cols in dictionary.items()}
    expected={**live.BASE_COLUMNS,'T_BATCH':live.BASE_COLUMNS['T_BATCH']+['C5_NOTE']}
    if previous is not None and dictionary!=json.loads((previous/'run/producer-profile.json').read_text())['columns']:
        raise ValueError('source type/schema changed; pause before applying data')
    if approved!=expected: raise ValueError('schema changed; pause and require maintained review')
    count_sql=live.HEAD+''.join("SELECT 'COUNT|"+table+"|' || COUNT(*) FROM (SELECT 1 FROM OO04_FIX."+table+" AS OF SCN "+str(scn)+" WHERE ROWNUM <= 9);\n" for table in dictionary)+'EXIT\n'
    count_code,count_text=live.sqlplus('OO04_READER',password,count_sql,'counts',sql,secrets)
    if count_code: raise ValueError('source count failed')
    counts={line.split('|')[0]:int(line.split('|')[1]) for line in live.lines(count_text,'COUNT|')}
    if set(counts)!=set(dictionary) or any(value>8 for value in counts.values()): raise ValueError('source row budget before data read')
    code,text=live.sqlplus('OO04_READER',password,live.pull_sql(scn,True).replace('ORDER BY id;', 'ORDER BY id FETCH FIRST 8 ROWS ONLY;'),'pull',sql,secrets)
    if code: raise ValueError('source read failed; see raw exit')
    rows=live.parse_pull(text,True)
    for table in rows:
        if len(rows[table])!=counts[table] or len(rows[table])>loaded['budget']['hard_limit']: raise ValueError('source row budget')
    arrow=out/'run/arrow'; arrow.mkdir()
    specs=[]
    for table,table_rows in rows.items():
        old=None if previous is None else capture.load_generation(previous/'run/shadow'/table,generation-1)
        result=capture.apply_snapshot(out/'run/shadow'/table,old,generation,'oo-dl01w1-04-scn-'+str(scn),approved[table],approved[table],table_rows,8)
        if not result['committed']: raise ValueError(result['state'])
        req=live.write_arrow_request(arrow,table,generation,approved[table],table_rows)
        filename=table.lower()+'-gen-'+str(generation)+'.arrow'
        args=['docker','run','--rm','--network','none','--user','501:20','-v','oo-dl01w1-04-c5-target:/t:ro','-v',str(out)+':'+str(out),'--entrypoint','/bin/sh',IMAGE,'-c','test "$(sha256sum /t/r194/debug/oo-c5-arrow | cut -d" " -f1)" = '+ENCODER_SHA+' && exec /t/r194/debug/oo-c5-arrow encode "$1" "$2"','sh',req,str(arrow/filename)]
        p=subprocess.run(args,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        (arrow/(filename+'.stdout')).write_bytes(p.stdout);(arrow/(filename+'.stderr')).write_bytes(p.stderr);(arrow/(filename+'.exit')).write_text(str(p.returncode)+'\n')
        if p.returncode: raise ValueError('real encoder failed')
        specs.append({'table':table,'generation':generation,'arrow':filename,'source_id':'oo04_native_'+table.removeprefix('T_').lower()})
    profile=json.loads(live.RUN.joinpath('producer-profile.json').read_text()) if hasattr(live,'RUN') else json.loads((witness.PROFILE_PATH).read_text())
    profile['columns']=dictionary
    (out/'run/producer-profile.json').write_text(json.dumps(profile,indent=2)+'\n')
    witness.CAPTURE=out; witness.ARROW_DIR=arrow; witness.PROFILE_PATH=out/'run/producer-profile.json'; witness.TABLES=tuple(specs)
    driver=Path('/tmp/oo-dl01w1-04-native-source-cli/sqlplus').resolve()
    if hashlib.sha256(driver.read_bytes()).hexdigest()!=SQLPLUS_SHA: raise ValueError('actual SQLPlus driver changed')
    desc={'kind':'command-line','name':'sqlplus','version':'23.26.3.0.0','digest':SQLPLUS_SHA,'bytes':driver.stat().st_size,'path':str(driver),'digest_is_file':True}
    witness.write_tree(out/'witness',driver,desc)
    (out/'capture.json').write_text(json.dumps({'actual_scn':scn,'generation':generation,'observer_password_accessed':False,'source_reader':'OO04_READER','source_dml_ddl':0,'tables':{t:len(r) for t,r in rows.items()},'encoder_sha256':ENCODER_SHA,'driver_sha256':SQLPLUS_SHA,'native_imported':False},indent=2)+'\n')
    return out

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--scn',required=True);p.add_argument('--out',type=Path,required=True);p.add_argument('--generation',type=int,default=1);p.add_argument('--previous',type=Path)
    a=p.parse_args();run(a.scn,a.out.resolve(),a.generation,a.previous.resolve() if a.previous else None)
