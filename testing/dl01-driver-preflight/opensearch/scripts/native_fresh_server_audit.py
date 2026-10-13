#!/usr/bin/env python3
"""Independent complete server REST audit read; credentials remain in-process."""
import base64
import argparse
import json
from pathlib import Path
import ssl
import urllib.request

ROOT = Path(__file__).resolve().parents[4]
OUT = ROOT / 'evidence/bc6cf794-2d14-4e98-bcb6-f7a9b3533a88/DL01/opensearch-driver-preflight/native-fresh-20261012-r2/server-audit'
PRIVATE = Path('/tmp/oo-dl01w1-44-native-fresh-private-r2')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, default=OUT)
    args = parser.parse_args()
    out = args.out.resolve()
    owned = (ROOT / 'evidence/bc6cf794-2d14-4e98-bcb6-f7a9b3533a88/DL01/opensearch-driver-preflight').resolve()
    if not out.is_relative_to(owned):
        raise SystemExit('audit output outside owned evidence')
    out.mkdir(parents=True, exist_ok=False)
    pairs = dict(line.split('=', 1) for line in (PRIVATE / 'identities.env').read_text().splitlines() if '=' in line)
    ctx = ssl.create_default_context(cafile=str(PRIVATE / 'certs/ca.pem'))
    auth = base64.b64encode((pairs['dba_user'] + ':' + pairs['dba_password']).encode()).decode()
    body = json.dumps({'size': 10000, 'track_total_hits': True,
        'query': {'term': {'audit_request_effective_user': pairs['reader_user']}}}).encode()
    req = urllib.request.Request('https://127.0.0.1:19344/security-auditlog-*/_search',
        data=body, headers={'Content-Type': 'application/json', 'Authorization': 'Basic ' + auth}, method='POST')
    with urllib.request.urlopen(req, context=ctx, timeout=30) as response:
        raw = response.read()
        status = response.status
    # Bytes are retained before interpreting the result.
    (out / 'response.json').write_bytes(raw)
    (out / 'request.json').write_bytes(body)
    doc = json.loads(raw)
    hits = doc['hits']['hits']
    total = doc['hits']['total']
    assert total['relation'] == 'eq' and total['value'] == len(hits), 'audit truncated'
    events = [h['_source'] for h in hits]
    methods = {}
    forbidden = []
    for e in events:
        method = e.get('audit_rest_request_method', '')
        methods[method] = methods.get(method, 0) + 1
        path = e.get('audit_rest_request_path', '')
        # POST is legal search/PIT open/close, so distinguish actual data mutations.
        if method in ('PUT', 'PATCH') or any(x in path for x in ('/_update', '/_bulk', '/_doc/')) or (method == 'DELETE' and '/_search/point_in_time' not in path):
            forbidden.append(e)
    assert not forbidden, 'reader server data-mutation event'
    report = {'status': status, 'total_reader_server_events': len(hits), 'complete': True,
        'methods': methods, 'reader_mutation_events': len(forbidden), 'source_acceptance': 'not_claimed'}
    (out / 'result.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
