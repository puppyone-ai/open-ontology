#!/usr/bin/env python3
"""Compare every current physical target cell with independently declared fixture truth."""
import argparse
import calendar
from datetime import datetime
import json
from pathlib import Path
import re
import subprocess

NAMES = ('reading_id','version','amount','note','code','code_or_null','ts','tags','labels','qty','label','wide_u64')
BINARY = {'note','code','code_or_null','label'}
ARRAY = {'tags','labels'}
TARGET_CONTAINER='oo-dl01w1-40-gpt-fresh-pg'
TARGET_DATABASE='oo_ch40'

def query(sql):
    r=subprocess.run(['docker','exec',TARGET_CONTAINER,'psql','-U','postgres','-d',TARGET_DATABASE,'-At','-v','ON_ERROR_STOP=1','-c',sql],capture_output=True,text=True)
    if r.returncode: raise RuntimeError(r.stderr)
    return r.stdout.strip()

def nanos(value):
    main,frac=value.split('.')
    return str(calendar.timegm(datetime.strptime(main,'%Y-%m-%d %H:%M:%S').timetuple())*1_000_000_000+int(frac))

def main():
    global TARGET_CONTAINER, TARGET_DATABASE
    p=argparse.ArgumentParser();p.add_argument('--gold',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--generation',type=int,default=4);p.add_argument('--connection-config',type=Path);a=p.parse_args()
    config=json.loads(a.connection_config.read_text()) if a.connection_config else {}
    TARGET_CONTAINER=config.get('target_container',TARGET_CONTAINER);TARGET_DATABASE=config.get('target_database',TARGET_DATABASE)
    source_id=config.get('source_id','clickhouse_268');table_name=config.get('source',{}).get('table','readings')
    if not all(re.fullmatch(r'[A-Za-z_][A-Za-z0-9_]*',x) for x in (source_id,table_name,TARGET_DATABASE)):
        raise RuntimeError('target identifier refused')
    if a.generation<1: raise RuntimeError('bad generation')
    mappings=query(f"SELECT logical_column||'|'||physical_name FROM _oo.physical_mappings WHERE source='{source_id}' AND run_generation={a.generation} AND level='c'")
    found=dict(line.split('|',1) for line in mappings.splitlines())
    if set(found)!=set(NAMES): raise RuntimeError('incomplete target physical mapping')
    table=query(f"SELECT physical_name FROM _oo.physical_mappings WHERE source='{source_id}' AND run_generation={a.generation} AND level='t' AND logical_table='{table_name}'")
    if not re.fullmatch(r'pt_[0-9a-f]{60}',table): raise RuntimeError('bad physical table')
    args=[]
    for name in NAMES:
        physical=found[name]
        if not re.fullmatch(r'pc_[0-9a-f]{60}',physical): raise RuntimeError('bad physical field')
        value=(f"CASE WHEN {physical} IS NULL THEN NULL ELSE encode({physical},'hex') END" if name in BINARY else
               f'{physical}::text')
        args.extend(["'"+name+"'",value])
    sql=f"SELECT row_to_json(x) FROM (SELECT json_build_object({','.join(args)}) AS cells FROM _oo.{table} ORDER BY {found['reading_id']}) x"
    target=[json.loads(json.loads(line)['cells']) if isinstance(json.loads(line)['cells'],str) else json.loads(line)['cells'] for line in query(sql).splitlines()]
    gold=json.loads(a.gold.read_text())
    def normalize_target(row):
        out={}
        for key,value in row.items():
            if key in BINARY: out[key]=None if value is None else bytes.fromhex(value).decode('utf-8')
            elif key in ARRAY: out[key]=json.loads(value)
            else: out[key]=value
        return out
    target=[normalize_target(row) for row in target]
    expected=[]
    for row in gold:
        expected.append({key:(nanos(value) if key=='ts' else value) for key,value in row.items()})
    target.sort(key=lambda row:int(row['reading_id']));expected.sort(key=lambda row:int(row['reading_id']))
    mismatches=[]
    if len(target)!=len(expected): mismatches.append('row_count')
    for i,(actual,want) in enumerate(zip(target,expected)):
        for key in NAMES:
            av=actual[key];wv=want[key]
            if key in ARRAY: av=[str(v) if v is not None else None for v in av]
            if av!=wv: mismatches.append(f'{i}:{key}:actual={av!r}:gold={wv!r}')
    result={'table':table,'row_count':len(target),'columns':list(NAMES),'cell_count':len(target)*len(NAMES),'mismatches':mismatches,'diff_zero':not mismatches}
    a.output.write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps(result))
    raise SystemExit(0 if not mismatches else 1)

if __name__=='__main__': main()
