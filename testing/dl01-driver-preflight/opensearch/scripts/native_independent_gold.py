#!/usr/bin/env python3
"""Observe the approved OpenSearch index directly for later OO CLI gold."""
import base64
import hashlib
import json
from pathlib import Path
import ssl
import urllib.request

ROOT = Path(__file__).resolve().parents[4]
OUT = ROOT / 'evidence/bc6cf794-2d14-4e98-bcb6-f7a9b3533a88/DL01/opensearch-product/legal-batch-gold'
PRIVATE = Path('/tmp/oo-dl01w1-44-native-fresh-private-r2')


def main():
    OUT.mkdir(parents=True, exist_ok=False)
    identities = dict(line.split('=',1) for line in (PRIVATE/'identities.env').read_text().splitlines() if '=' in line)
    auth = base64.b64encode((identities['dba_user']+':'+identities['dba_password']).encode()).decode()
    request_body = json.dumps({'size':40,'track_total_hits':True,'sort':['_id'],
        '_source':['doc_key','seq_key','scaled_value','event_ms','note']}).encode()
    req = urllib.request.Request('https://127.0.0.1:19344/oo-dl01w1-44-ledger/_search',
        data=request_body,headers={'Content-Type':'application/json','Authorization':'Basic '+auth},method='POST')
    ctx = ssl.create_default_context(cafile=str(PRIVATE/'certs/ca.pem'))
    with urllib.request.urlopen(req,context=ctx,timeout=30) as response:
        raw = response.read()
        assert response.status == 200
    (OUT/'source-request.json').write_bytes(request_body)
    (OUT/'source-response.json').write_bytes(raw)
    result = json.loads(raw)
    hits = result['hits']['hits']
    total = result['hits']['total']
    assert total['relation'] == 'eq' and total['value'] == len(hits) == 5
    docs = {hit['_source']['doc_key']:hit['_source'] for hit in hits}
    assert len(docs) == 5 and set(docs) == {'ledger-null','ledger-open','ledger-post','ledger-span','ledger-void'}
    gold = {'protocol':'oo.opensearch.independent-source-gold/v1','source':'direct OpenSearch DBA read',
        'response_sha256':hashlib.sha256(raw).hexdigest(),'rows':len(hits),'facts':{
            'ledger-post-seq':docs['ledger-post']['seq_key'],
            'ledger-post-note':docs['ledger-post']['note'],
            'ledger-null-seq':docs['ledger-null']['seq_key'],
            'ledger-span-seq':docs['ledger-span']['seq_key'],
        },'dialogue_truth':{
            'count':{'expected_exact_facts':['5']},
            'post':{'expected_exact_facts':['22','posted']},
            'null':{'expected_exact_facts':['33']},
            'span':{'expected_exact_facts':['44']},
        }}
    (OUT/'gold.json').write_text(json.dumps(gold,indent=2)+'\n')
    print(json.dumps({'rows':len(hits),'source_response_sha256':gold['response_sha256']}))


if __name__ == '__main__':
    main()
