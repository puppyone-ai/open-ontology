#!/usr/bin/env python3
"""Independent unsampled PostgreSQL cells versus the separate Arrow decoder."""
import argparse,json,pathlib,re,subprocess
from decimal import Decimal

def main():
    p=argparse.ArgumentParser();p.add_argument('--decoded',required=True);p.add_argument('--out',required=True);p.add_argument('--container',required=True);p.add_argument('--database',required=True);p.add_argument('--source',required=True);p.add_argument('--source-generation',type=int);a=p.parse_args()
    if not (a.container.startswith('oo-dl01w1-41-') or a.container.startswith('oo-mongodb-connection-whole-')) or not re.fullmatch('[a-zA-Z0-9_]+',a.source):raise ValueError('owned fixture identity')
    out=pathlib.Path(a.out);out.mkdir(parents=True,exist_ok=False)
    def sql(query,name):
        r=subprocess.run(['docker','exec','-i',a.container,'psql','-U','postgres','-d',a.database,'-At','-v','ON_ERROR_STOP=1'],input=query.encode(),capture_output=True)
        (out/(name+'.stdout')).write_bytes(r.stdout);(out/(name+'.stderr')).write_bytes(r.stderr);(out/(name+'.exit')).write_text(str(r.returncode)+'\n')
        if r.returncode:raise RuntimeError('target SELECT failed')
        return [json.loads(line) for line in r.stdout.splitlines()]
    if a.source_generation is not None and a.source_generation<1:raise ValueError('source generation')
    generation_filter='' if a.source_generation is None else ' and source_generation='+str(a.source_generation)
    batch=sql("select row_to_json(t)::text from _oo.update_batch t where source='"+a.source+"'"+generation_filter+" order by source_generation desc limit 1",'batch')[0]
    generation=batch['target_generation'];table=batch['physical_name']
    mappings=sql("select row_to_json(m)::text from _oo.physical_mappings m where source='"+a.source+"' and run_generation="+str(generation)+" and level='c' and logical_table='shipments'",'mapping')
    columns={}
    for mapping in mappings:
        name=mapping['logical_column']
        # Native mapping uses reversible q_<utf8hex> for mixed-case source
        # names; the expected source roster retains its original spelling.
        if name.startswith('q_') and re.fullmatch('[0-9a-f]+',name[2:]):name=bytes.fromhex(name[2:]).decode()
        columns[name]=mapping['physical_name']
    if len(columns)!=20 or not re.fullmatch('pt_[0-9a-f]{60}',table):raise RuntimeError('original20 mapping required')
    def q(name):
        physical=columns[name]
        if not re.fullmatch('pc_[0-9a-f]{60}',physical):raise RuntimeError('physical column')
        return '"'+physical+'"'
    fields=[]
    for name in columns:
        value=q(name)
        if name=='uid':value="replace("+value+"::text,'-','')"
        elif name=='blob':value="encode("+value+",'hex')"
        elif name=='created':value='(extract(epoch from '+value+')*1000)::bigint'
        elif name in ['units','price','weight']:value+='::text'
        fields.extend(["'"+name+"'",value])
    actual=sql('select json_build_object('+','.join(fields)+')::text from _oo."'+table+'" order by '+q('_id'),'cells')
    attrs=sql("select json_build_object('name',attname,'oid',atttypid::int,'typmod',atttypmod)::text from pg_attribute where attrelid='_oo.\""+table+"\"'::regclass and attnum>0 and not attisdropped",'types')
    attr={r['name']:r for r in attrs};oids={'_id':25,'uid':2950,'blob':17,'flag':23,'units':20,'price':1700,'created':1184,'weight':701,'lane':114,'marks':114,'pack':114,'doc':3802}
    differences=[]
    for name,physical in columns.items():
        wanted=oids.get(name,21 if name.endswith('_presence') else 1043)
        if attr[physical]['oid']!=wanted:differences.append({'column':name,'type_expected':wanted,'actual':attr[physical]})
    expected=[]
    for row in json.loads(pathlib.Path(a.decoded).read_text())['records']:
        e={name:row[name] for name in ['_id','code','flag','units','warehouse','whCode','memo','memo_presence','marks','marks_presence','pack_presence','blob_presence','doc']}
        e.update(uid=row['uid_hex'],blob=row['blob_hex'],created=row['created_ms'],price=str(Decimal(row['price']).scaleb(-7)),weight=str(row['weight']))
        e['lane']={'type_id':0 if row['lane_tag']=='utf8' else 1,'type':row['lane_tag'],'value':row['lane_text'] if row['lane_tag']=='utf8' else int(row['lane_num'])}
        e['pack']=None if row['pack_presence']<2 else {'kind':row['pack_kind'],'n':row['pack_n']};expected.append(e)
    expected.sort(key=lambda r:r['_id'])
    if len(actual)!=len(expected):differences.append({'rows_expected':len(expected),'rows_actual':len(actual)})
    for got,wanted in zip(actual,expected):
        for name in columns:
            equal=got[name]==wanted[name]
            if name in ['price','weight']:equal=Decimal(got[name])==Decimal(wanted[name])
            if not equal:differences.append({'key':wanted['_id'],'column':name,'expected':wanted[name],'actual':got[name]})
    result={'source':a.source,'source_generation':batch['source_generation'],'target_generation':generation,'actual_rows':len(actual),'expected_rows':len(expected),'full_unsampled':True,'columns':20,'typed_cells':len(expected)*20,'cells_differ':len(differences),'differences':differences,'source_acceptance':False}
    (out/'result.json').write_text(json.dumps(result,sort_keys=True)+'\n');print(json.dumps({k:v for k,v in result.items() if k!='differences'}));return bool(differences)

if __name__=='__main__':raise SystemExit(main())
