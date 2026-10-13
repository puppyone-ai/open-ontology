#!/usr/bin/env python3
"""Prepare source-specific reviewed Gateway runtime and least-privilege PG roles."""
import hashlib, json, os, pathlib, secrets, shutil, subprocess
ROOT=pathlib.Path(__file__).resolve().parents[2]
B=ROOT/'evidence/bc6cf794-2d14-4e98-bcb6-f7a9b3533a88/DL01/opensearch-product/legal-batch-selfbuild/native-maintenance'
P=pathlib.Path('/tmp/oo-dl01w1-44-gateway-preview')
P.mkdir(mode=0o700,exist_ok=True)
load=lambda n:json.loads((B/n).read_text())
command=load('command.json'); reviewed=load('reviewed.json'); visibility=load('visibility.json'); pack=load('role-pack.json')
vector=command['catalog']['binding_vector']; entry=command['catalog']['entries'][0]; table=entry['tables'][0]
assert len(vector)==1 and visibility['role']=='oo_r_ledger' and table['columns']
visible=visibility['datasets'][0]['columns']; decisions={(d['dataset'],d['column']):d for d in command['decisions']}
assert len(visible)==14 and all(('ledger.ledger_rows',c) in decisions for c in visible)
assert all(decisions['ledger.ledger_rows',c]['pii']=='none' for c in visible)
cat=command['catalog']; catsha=hashlib.sha256(json.dumps(cat,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode()).hexdigest()
assert reviewed['provenance']['collection_sha256']==cat['collection_sha256']
reviewdir=P/'reviewed/ledger'; reviewdir.mkdir(parents=True,exist_ok=True)
(reviewdir/'native-input.json').write_text(json.dumps(command,sort_keys=True,separators=(',',':'))+'\n')
(reviewdir/'command.json').write_text(json.dumps(reviewed,sort_keys=True,separators=(',',':'))+'\n')
bind={"allow":[{"column":c,"dataset":"ledger.ledger_rows","explicit_allow":False} for c in visible],"artifact_sha256":catsha,"binding_vector":vector,"catalog_version":cat['collection_sha256'],"config_digest":vector[0]['config_digest'],"generation":6,"knowledge_id":command['settings']['knowledge']['id'],"knowledge_version":"1","login":"oo_dl01w1_44_gateway_login","ontology_version":command['settings']['ontology_version'],"policy_id":command['settings']['policy']['id'],"policy_version":"1","project":"opensearch","role":"ledger","schema":"ledger","schema_hash":cat['collection_sha256'],"source_id":"oo_dl01w1_44"}
(reviewdir/'binding.json').write_text(json.dumps(bind,sort_keys=True,separators=(',',':'))+'\n')
(P/'packs').mkdir(exist_ok=True);(P/'packs/ledger.json').write_text(json.dumps(pack,sort_keys=True,separators=(',',':'))+'\n')
(P/'wal').mkdir(exist_ok=True)
(P/'endpoint.json').write_text((P/'token-endpoint.json').read_text())
(P/'data-endpoint.json').write_text(json.dumps({'host':'oo-dl01w1-44-opensearch','port':5432,'database':'oo_dl01w1_44_native_fresh'})+'\n')
shutil.copy2('/tmp/oo-dl01w1-44-native-fresh-private-r2/certs/ca.pem',P/'ca.crt')
shutil.copy2(P/'ca.crt',P/'data-ca.crt')
shutil.copy2(P/'token-runtime.pass',P/'runtime.pass')
config={"listen":"127.0.0.1:0","audience":"https://127.0.0.1:9/","runtime_user":"oo_token_runtime","password_file":"runtime.pass","ca_file":"ca.crt","endpoint_file":"endpoint.json","pack_dir":"packs","body_limit":1048576,"request_timeout_ms":15000,"global_concurrency":4,"data_endpoint_file":"data-endpoint.json","data_password_file":"data.pass","data_ca_file":"data-ca.crt","data_user":"oo_dl01w1_44_gateway_login","wal_dir":"wal","reviewed_dir":"reviewed"}
(P/'config.json').write_text(json.dumps(config,sort_keys=True,separators=(',',':'))+'\n')
grant={"kind":"client","project_id":"opensearch","principal_id":"fixture-admin","role":"oo_r_ledger","policy_version":"1","knowledge_version":"1","audience_url":config['audience'],"claims":[],"client_ttl_ms":3600000}
(P/'grant.json').write_text(json.dumps(grant,sort_keys=True,separators=(',',':'))+'\n')
if not (P/'data.pass').exists():
 pw=secrets.token_urlsafe(48);(P/'data.pass').write_text(pw+'\n');os.chmod(P/'data.pass',0o600)
else: pw=(P/'data.pass').read_text().strip()
cols={c['logical_name']:c['stored_physical'] for c in table['columns']}
assert set(visible)<=set(cols)
physical=vector[0]['stored_physical']; q=lambda s:'"'+s.replace('"','""')+'"'
column_list=', '.join(q(cols[c]) for c in visible)
role='oo_r_ledger';login='oo_dl01w1_44_gateway_login'
sql=f"""DO $$ BEGIN IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname='{role}') THEN CREATE ROLE {q(role)} NOLOGIN NOINHERIT; END IF; IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname='{login}') THEN CREATE ROLE {q(login)} LOGIN NOINHERIT PASSWORD '{pw}'; END IF; END $$;
ALTER ROLE {q(role)} NOLOGIN NOINHERIT;
ALTER ROLE {q(login)} NOINHERIT PASSWORD '{pw}';
GRANT CONNECT ON DATABASE oo_dl01w1_44_native_fresh TO {q(login)};
GRANT USAGE ON SCHEMA _oo TO {q(login)}, {q(role)};
GRANT SELECT ON _oo.physical_mappings,_oo.table_freshness,_oo.source_watermark,_oo.table_quality,_oo.apply_receipts,_oo.full_import_run,_oo.full_import_table,_oo.full_import_quality TO {q(login)};
GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA _oo TO {q(login)};
GRANT SELECT ({column_list}) ON _oo.{q(physical)} TO {q(role)};
GRANT {q(role)} TO {q(login)} WITH INHERIT FALSE, SET TRUE;
"""
run=subprocess.run(['docker','exec','-i','-u','postgres','oo-dl01w1-44-native-fresh-pg','psql','-v','ON_ERROR_STOP=1','-d','oo_dl01w1_44_native_fresh'],input=sql,text=True,capture_output=True)
if run.returncode: raise SystemExit(run.stderr)
print(json.dumps({'catalog_sha256':catsha,'schema_hash':cat['collection_sha256'],'physical':physical,'visible_columns':visible,'role':role,'login':login,'pg_ddl_exit':run.returncode}))
