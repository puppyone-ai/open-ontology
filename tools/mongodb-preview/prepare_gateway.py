#!/usr/bin/env python3
"""Prepare the MongoDB native preview's isolated Gateway, token DB, and read-only roles."""
import hashlib, json, os, pathlib, secrets, shutil, subprocess
B=pathlib.Path(os.environ.get('OO_W41_MAINTENANCE_DIR', '/Users/redux/.cache/oo-dl01w1-71/native-generic/mongo41/work/builds/b_df0eae229850ceb71331c908dc082e86/native-maintenance/1344fc63b8bbdebc454943899a50c9837c37cb159c8df1673349fdad22874c9e'))
P=pathlib.Path('/tmp/oo-dl01w1-41-gateway-preview'); P.mkdir(mode=0o700,exist_ok=True)
load=lambda n:json.loads((B/n).read_text())
command=load('command.json'); reviewed=load('reviewed.json'); visibility=load('visibility.json'); pack=load('role-pack.json')
cat=command['catalog']; vector=cat['binding_vector']; entry=cat['entries'][0]; table=entry['tables'][0]; visible=visibility['datasets'][0]['columns']
assert len(vector)==1 and len(visible)==20 and visibility['role']=='oo_r_mongodb_full'
assert vector[0]['source_id']=='oo_w141_native_c5' and vector[0]['target_generation']==4 and vector[0]['dataset']=='oo_w141.shipments'
assert reviewed['provenance']['collection_sha256']==cat['collection_sha256']
assert all(d['pii']=='none' for d in command['decisions'])
# A model suggestion cannot replace a declared source-side semantic unit.
source_rules=json.loads((pathlib.Path(__file__).resolve().parents[2]/'evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/native-c5-20261012/cfc-selfbuild-bundle-unit-76b/rules.json').read_text())
for rule in source_rules['columns']:
 if 'unit' not in rule:
  continue
 dataset,column=rule['column'].rsplit('.',1)
 effective=next(c for d in reviewed['effective_oos']['datasets'] if d['dataset']==dataset for c in d['columns'] if c['name']==column)
 if effective['unit']!=rule['unit']:
  raise ValueError(f'source semantic unit mismatch: {dataset}.{column}; expected {rule["unit"]}, got {effective["unit"]}')
catsha=hashlib.sha256(json.dumps(cat,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode()).hexdigest()
R=P/'reviewed/mongodb_full';R.mkdir(parents=True,exist_ok=True)
(R/'native-input.json').write_text(json.dumps(command,sort_keys=True,separators=(',',':'))+'\n')
(R/'command.json').write_text(json.dumps(reviewed,sort_keys=True,separators=(',',':'))+'\n')
bind={'allow':[{'column':c,'dataset':'oo_w141.shipments','explicit_allow':False} for c in visible], 'artifact_sha256':catsha,'binding_vector':vector,'catalog_version':cat['collection_sha256'],'config_digest':vector[0]['config_digest'],'generation':4,'knowledge_id':command['settings']['knowledge']['id'],'knowledge_version':'1','login':'oo_w141_gateway_login','ontology_version':command['settings']['ontology_version'],'policy_id':command['settings']['policy']['id'],'policy_version':'1','project':'mongodb','role':'mongodb_full','schema':'oo_w141','schema_hash':cat['collection_sha256'],'source_id':'oo_w141_native_c5'}
(R/'binding.json').write_text(json.dumps(bind,sort_keys=True,separators=(',',':'))+'\n')
(P/'packs').mkdir(exist_ok=True);(P/'packs/mongodb_full.json').write_text(json.dumps(pack,sort_keys=True,separators=(',',':'))+'\n')
(P/'wal').mkdir(exist_ok=True)
for f in ['token-admin.pass','token-runtime.pass','data.pass']:
 p=P/f
 if not p.exists():
  p.write_text(secrets.token_hex(32)+'\n');p.chmod(0o600)
admin=(P/'token-admin.pass').read_text().strip(); runtime=(P/'token-runtime.pass').read_text().strip(); data=(P/'data.pass').read_text().strip()
q=lambda s:'"'+s.replace('"','""')+'"'
execsql=lambda db,sql:subprocess.run(['docker','exec','-i','-u','postgres','oo-dl01w1-41-pg-native','psql','-v','ON_ERROR_STOP=1','-d',db],input=sql,text=True,capture_output=True,check=True)
roles=[('oo_token_admin',admin),('oo_token_runtime',runtime),('oo_w141_gateway_login',data)]
for role,pw in roles:
 execsql('postgres',f"DO $$ BEGIN IF NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='{role}') THEN CREATE ROLE {q(role)} LOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS PASSWORD '{pw}'; END IF; END $$;\n")
execsql('postgres',"SELECT 'CREATE DATABASE oo_w141_gateway_token OWNER oo_token_admin' WHERE NOT EXISTS(SELECT 1 FROM pg_database WHERE datname='oo_w141_gateway_token')\\gexec\n")
execsql('postgres',"REVOKE CONNECT ON DATABASE oo_w141_gateway_token FROM PUBLIC; GRANT CONNECT ON DATABASE oo_w141_gateway_token TO oo_token_admin, oo_token_runtime;\n")
cols={c['logical_name']:c['stored_physical'] for c in table['columns']}; assert set(visible)==set(cols)
physical=vector[0]['stored_physical']; column_list=', '.join(q(cols[c]) for c in visible)
sql=f"""DO $$ BEGIN IF NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='oo_r_mongodb_full') THEN CREATE ROLE oo_r_mongodb_full NOLOGIN NOINHERIT; END IF; END $$;
ALTER ROLE oo_r_mongodb_full NOLOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
ALTER ROLE oo_w141_gateway_login NOINHERIT PASSWORD '{data}';
GRANT CONNECT ON DATABASE oo_w141_native20261013 TO oo_w141_gateway_login;
GRANT USAGE ON SCHEMA _oo TO oo_w141_gateway_login, oo_r_mongodb_full;
GRANT SELECT ON _oo.physical_mappings,_oo.table_freshness,_oo.source_watermark,_oo.table_quality,_oo.apply_receipts,_oo.full_import_run,_oo.full_import_table,_oo.full_import_quality TO oo_w141_gateway_login;
GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA _oo TO oo_w141_gateway_login;
GRANT SELECT ({column_list}) ON _oo.{q(physical)} TO oo_r_mongodb_full;
GRANT oo_r_mongodb_full TO oo_w141_gateway_login WITH INHERIT FALSE, SET TRUE;
"""
execsql('oo_w141_native20261013',sql)
endpoint={'host':'oo-dl01w1-41-pg-native','port':5432,'database':'oo_w141_gateway_token'}
(P/'token-endpoint.json').write_text(json.dumps(endpoint)+'\n');(P/'endpoint.json').write_text(json.dumps(endpoint)+'\n')
(P/'data-endpoint.json').write_text(json.dumps({'host':'oo-dl01w1-41-pg-native','port':5432,'database':'oo_w141_native20261013'})+'\n')
shutil.copy2('/tmp/oo-dl01w1-41-secrets/ca.crt',P/'ca.crt');shutil.copy2(P/'ca.crt',P/'data-ca.crt');shutil.copy2(P/'token-runtime.pass',P/'runtime.pass')
config={'listen':'127.0.0.1:0','audience':'https://127.0.0.1:9/','runtime_user':'oo_token_runtime','password_file':'runtime.pass','ca_file':'ca.crt','endpoint_file':'endpoint.json','pack_dir':'packs','body_limit':1048576,'request_timeout_ms':15000,'global_concurrency':4,'data_endpoint_file':'data-endpoint.json','data_password_file':'data.pass','data_ca_file':'data-ca.crt','data_user':'oo_w141_gateway_login','wal_dir':'wal','reviewed_dir':'reviewed'}
(P/'config.json').write_text(json.dumps(config,sort_keys=True,separators=(',',':'))+'\n')
grant={'kind':'client','project_id':'mongodb','principal_id':'fixture-admin','role':'oo_r_mongodb_full','policy_version':'1','knowledge_version':'1','audience_url':config['audience'],'claims':[],'client_ttl_ms':3600000}
(P/'grant.json').write_text(json.dumps(grant,sort_keys=True,separators=(',',':'))+'\n')
print(json.dumps({'catalog_sha256':catsha,'schema_hash':cat['collection_sha256'],'physical':physical,'visible_count':len(visible),'role':'oo_r_mongodb_full','login':'oo_w141_gateway_login','token_db':'oo_w141_gateway_token'}))
