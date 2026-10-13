#!/usr/bin/env python3
"""Prepare a private, source-gold-bound MongoDB OO dialogue run."""
import json, os, pathlib
ROOT=pathlib.Path(__file__).resolve().parents[3]
GOLD=ROOT/'evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/native-c5-20261012/cfc-selfbuild-fixed-76b/gold-target-aggregate.json'
PRIVATE=pathlib.Path('/tmp/oo-dl01w1-41-gateway-preview')
gold=json.loads(GOLD.read_text()); result=gold['result']
assert gold['protocol']=='oo.mongodb.gold-target-aggregate/v1' and gold['target_generation']==4
assert result['approved_rows']==14 and result['deleted_id_present']==0 and result['inserted_id_present']==1
addr=(PRIVATE/'listen.addr').read_text().strip()
assert addr.startswith('127.0.0.1:')
truth={
 'aggregate':{'expected_exact_facts':[str(result['approved_rows']),result['units_sum']]},
 'mutation':{'expected_exact_facts':['S41-001-U','S41-017-I']},
 'identity':{'expected_exact_facts':['oo_w141.shipments','whCode']},
}
truth_path=PRIVATE/'dialogue-truth.json';truth_path.write_text(json.dumps(truth,sort_keys=True)+'\n')
config={'model_origin':'http://127.0.0.1:8317','model':'grok-4.7','model_bearer_file':'/tmp/oo-product-model-20261012/bearer',
 'gateway_token_file':str(PRIVATE/'client-20261013-01.token'),'gateway':'http://'+addr,'transport':'local-development','auth':'external',
 'oo_cli':str(ROOT/'tools/mongodb-preview/scripts/oo-gateway-cli.py'),
 'independent_truth_file':str(truth_path),'independent_gateway_audit_file':str(PRIVATE/'wal'),
 'questions':[
  {'truth_key':'aggregate','prompt':'Inspect the role knowledge and schema through OO tools, then query the current reviewed MongoDB shipments. Report the exact row count and exact SUM(units), with query IDs and freshness caveats.'},
  {'truth_key':'mutation','prompt':'Using only OO tools and actual SQL, verify the update (code S41-001-U), physical delete of ObjectId c141d04141eef04100000002, and insertion (code S41-017-I). State each count and query ID; do not infer from the prompt.'},
  {'truth_key':'identity','prompt':'Using role knowledge, tables, describe, freshness, and read-only SQL as needed, identify the dataset oo_w141.shipments and explain the exact reviewed whCode column, ObjectId text, UUID and binary mappings, and whether the source is live or the target remains queryable independently.'},
 ],'max_tool_rounds':20}
path=PRIVATE/'dialogue-config.json';path.write_text(json.dumps(config,sort_keys=True,indent=2)+'\n');os.chmod(path,0o600)
print(json.dumps({'config':str(path),'truth':str(truth_path),'gold':str(GOLD),'gateway':config['gateway']}))
