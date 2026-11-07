#!/usr/bin/env python3
"""Explicit opt-in: two one-token probes, one short completion, one stream."""
import argparse
import http.client
import json
import os
from pathlib import Path
import threading
from model_egress import Egress, Refusal, server


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--config', required=True)
    parser.add_argument('--out', required=True)
    args = parser.parse_args()
    out = Path(args.out)
    out.mkdir(mode=0o700, exist_ok=True)
    config = json.loads(Path(args.config).read_text())
    egress = Egress(config['model'], out / 'usage.jsonl', timeout=90)
    httpd = None
    try:
        probe = egress.probe()
        (out / 'probe.json').write_bytes(egress.safe(probe) + b'\n')
        httpd = server(egress, '127.0.0.1', 0)
        thread = threading.Thread(target=httpd.serve_forever, daemon=True)
        thread.start()
        results = []
        for stream in (False, True):
            conn = http.client.HTTPConnection('127.0.0.1', httpd.server_port, timeout=100)
            request = {'model': config['model']['model'], 'messages': [{'role': 'user', 'content': 'Say OK.'}],
                       'max_tokens': 16, 'stream': stream}
            conn.request('POST', '/v1/chat/completions', json.dumps(request), {'Content-Type': 'application/json'})
            response = conn.getresponse()
            raw = response.read()
            conn.close()
            if egress.key.encode() in raw:
                raise Refusal('SECRET_ECHO')
            (out / ('stream.sse' if stream else 'completion.json')).write_bytes(raw)
            if response.status != 200:
                raise Refusal('LIVE_HTTP_FAILURE')
            if stream:
                if b'data: [DONE]' not in raw or b'"error"' in raw:
                    raise Refusal('LIVE_STREAM_FAILURE')
            elif not isinstance(json.loads(raw).get('choices'), list):
                raise Refusal('LIVE_COMPLETION_FAILURE')
            results.append({'stream': stream, 'http_status': response.status, 'bytes': len(raw)})
        (out / 'result.json').write_text(json.dumps({'status':'passed', 'calls':results})+'\n')
        print(json.dumps({'status':'passed', 'calls':results, 'probe':probe}))
        return 0
    except Refusal as error:
        print(json.dumps({'status':'failed', 'code':error.code}))
        return 2
    except (OSError, ValueError):
        print(json.dumps({'status':'failed', 'code':'LIVE_TRANSPORT'}))
        return 2
    finally:
        if httpd:
            httpd.shutdown()
            httpd.server_close()
            thread.join()
        egress.close()


if __name__ == '__main__':
    os.umask(0o077)
    raise SystemExit(main())
