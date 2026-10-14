#!/usr/bin/env python3
"""Product and dialogue step. Never starts a model create without the fresh private config."""
import json, os, pathlib, socket, ssl, subprocess, sys

FLOW=pathlib.Path('/Users/redux/puppy-one/open-ontology/tools/source-onboarding/product_flow.py')
DIALOGUE=pathlib.Path('/Users/redux/puppy-one/open-ontology/tools/agent-product-verifier/dialogue.py')

def identity_status():
    raw=socket.create_connection(('127.0.0.1', 18441), 3)
    try:
        context=ssl.create_default_context(); context.check_hostname=False; context.verify_mode=ssl.CERT_NONE
        secured=context.wrap_socket(raw); secured.settimeout(3)
        secured.send(b'GET /control/v1/identity/current HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n')
        return secured.recv(80).split(b'\r\n', 1)[0].decode('ascii', 'replace')
    finally:
        raw.close()

def run(argv):
    result=subprocess.run(argv)
    if result.returncode: raise SystemExit(result.returncode)

def main():
    parser=__import__('argparse').ArgumentParser(); parser.add_argument('--config', required=True); parser.add_argument('--run', required=True)
    args=parser.parse_args(); root=pathlib.Path(args.run)
    whole=root/'whole.json'
    if whole.exists():
        print(whole.read_text()); return
    try: status=identity_status()
    except OSError:
        print('identity_not_serving', file=sys.stderr); raise SystemExit(2)
    product=root/'product'; receipt=product/'product-receipt.json'; config=root/'product-config.json'
    if not receipt.exists():
        if not config.is_file() or config.stat().st_mode & 0o077:
            print('product_config_absent identity_status='+status, file=sys.stderr); raise SystemExit(2)
        run([sys.executable, str(FLOW), '--config', str(config), '--out', str(product)])
    dialogue=root/'dialogue'; verdict=dialogue/'verdict.json'; dconfig=root/'dialogue-config.json'
    if not verdict.exists():
        if not dconfig.is_file() or dconfig.stat().st_mode & 0o077:
            print('dialogue_config_absent', file=sys.stderr); raise SystemExit(2)
        run([sys.executable, str(DIALOGUE), '--config', str(dconfig), '--out', str(dialogue)])
    product_body=json.loads(receipt.read_text()); dialogue_body=json.loads(verdict.read_text()); cycle=json.loads((root/'cycle-receipt.json').read_text())
    assert product_body.get('protocol')=='oo.source-onboarding.product/v1' and product_body.get('fixture_review') is True
    assert dialogue_body.get('outcome')=='verified' and dialogue_body.get('turns', 0)>=2
    assert cycle.get('insert_update_delete_verified') is True and cycle.get('recovery_verified') is True and cycle.get('typed_cell_diff')==0
    body={'full_product_chain_complete':True,'periodic_insert_update_delete_verified':True,'recovery_verified':True,'product_receipt':'product/product-receipt.json','dialogue_verdict':'dialogue/verdict.json'}
    whole.write_text(json.dumps(body, indent=2)+'\n'); os.chmod(whole, 0o600); print(json.dumps(body))

if __name__=='__main__':
    main()
