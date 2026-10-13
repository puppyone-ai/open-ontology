#!/usr/bin/env python3
"""Read-only proof that the reviewed names resolve to the exact gen4 PG registry/columns."""
import hashlib, json, pathlib, subprocess
BASE=pathlib.Path('/tmp/oo-dl01w1-41-gateway-preview')
cmd=json.loads((BASE/'reviewed/mongodb_full/native-input.json').read_text())
entry=cmd['catalog']['entries'][0]; table=entry['tables'][0]; vector=cmd['catalog']['binding_vector'][0]
assert vector['target_generation']==4 and len(table['columns'])==20
sql="""SELECT jsonb_build_object('source',source,'config_digest',config_digest,'run_generation',run_generation,'level',level,'logical_namespace',logical_namespace,'logical_table',logical_table,'logical_column',logical_column,'physical_name',physical_name,'scope_namespace_physical',scope_namespace_physical,'scope_table_physical',scope_table_physical) FROM _oo.physical_mappings WHERE source='oo_w141_native_c5' AND config_digest='ab6dead498b56377f15766cfa802cba6bbdf075962bc6b1c18ccd9055ae33bf0' AND run_generation=4 AND logical_namespace='oo_w141' AND logical_table='shipments' ORDER BY level,logical_column"""
proc=subprocess.run(['docker','exec','-u','postgres','oo-dl01w1-41-pg-native','psql','-At','-d','oo_w141_native20261013','-c',sql],capture_output=True,text=True,check=True)
rows=[json.loads(line) for line in proc.stdout.splitlines()]
columns=[r for r in rows if r['level']=='c']; assert len(columns)==20
mapping={r['logical_column']:r for r in columns}
assert table['stored_physical'].startswith('pt_') and len(table['stored_physical'])==63
physical_sql=("SELECT jsonb_build_object('name',a.attname,'oid',a.atttypid,'typmod',a.atttypmod) "
              "FROM pg_attribute a JOIN pg_class t ON t.oid=a.attrelid JOIN pg_namespace n ON n.oid=t.relnamespace "
              "WHERE n.nspname='_oo' AND t.relname='"+table['stored_physical']+"' AND a.attnum>0 AND NOT a.attisdropped")
pg=subprocess.run(['docker','exec','-u','postgres','oo-dl01w1-41-pg-native','psql','-At','-d','oo_w141_native20261013','-c',physical_sql],capture_output=True,text=True,check=True)
physical_columns={r['name']:r for r in (json.loads(line) for line in pg.stdout.splitlines())}
actual=[]
for field in table['columns']:
 name=field['logical_name']; expected=name if name in mapping else 'q_'+name.encode().hex()
 row=mapping[expected]
 assert row['physical_name']==field['stored_physical']
 assert row['scope_table_physical']==table['stored_physical']
 assert row['scope_namespace_physical']==table['stored_namespace']
 physical=physical_columns[field['stored_physical']]
 assert int(physical['oid'])==field['oid'] and physical['typmod']==field['typmod'], (name,physical,field['oid'],field['typmod'])
 actual.append({'original':name,'registry_key':expected,'physical':row['physical_name'],'oid':int(physical['oid']),'typmod':physical['typmod']})
assert len({r['registry_key'] for r in actual})==20
assert next(r for r in actual if r['original']=='whCode')['registry_key']=='q_7768436f6465'
proof={'protocol':'oo.mongodb.gateway-mapping-proof/v1','source_id':vector['source_id'],'generation':vector['source_generation'],'target_generation':vector['target_generation'],'run_id':vector['run_id'],'config_digest':vector['config_digest'],'collection_sha256':cmd['catalog']['collection_sha256'],'columns':actual,'registry_row_count':len(rows),'exact_columns':20}
out=pathlib.Path(__file__).resolve().parents[3]/'evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/native-c5-20261012/cfc-gateway/mapping-proof.json'
out.parent.mkdir(parents=True,exist_ok=True);out.write_text(json.dumps(proof,sort_keys=True,separators=(',',':'))+'\n')
print(json.dumps({'proof':str(out),'sha256':hashlib.sha256(out.read_bytes()).hexdigest(),'columns':20,'registry_rows':len(rows),'whCode_registry_key':'q_7768436f6465'}))
