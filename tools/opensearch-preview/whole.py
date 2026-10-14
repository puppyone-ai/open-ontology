#!/usr/bin/env python3
"""One onboard step for the retained OpenSearch fixture.

Resumes build b_e9653e459c33efde8b5ddd8cacd44365. Does not create another model
build and does not repeat source DML. A bad loader digest must be refused with
the watermark unchanged, then the pinned loader advances one generation.
"""
import hashlib
import json
import os
import subprocess
import sys
import time
from pathlib import Path

import connect

ROOT = Path(__file__).resolve().parents[2]
BASE = Path('/tmp/oo-opensearch-connection-whole')
BUILD = 'b_e9653e459c33efde8b5ddd8cacd44365'
PRODUCT_CONFIG = BASE / 'product.json'
PG = 'oo-dl01w1-44-native-fresh-pg'
IMAGE = 'oo-a395-selfbuild:local'
OWNER = '5d3f04f6-15de-4cf5-af60-62f24974cc76'
WORKER = Path('/tmp/oo-native-capture-worker-20261012-r12')
PIPE = Path('/tmp/oo-native-loader-minimal-20261012-linux194')
ADM = Path('/tmp/oo-dl01w1-02-native-admission-minimal')
PRIVATE = Path('/tmp/oo-dl01w1-44-native-fresh-private-r2')
WORKER_NAME = 'oo-os-connection'
EXPECTED = {
    'type': 'opensearch', 'database': 'oo-dl01w1-44-ledger', 'host': 'oo-dl01w1-44-opensearch',
    'port': 9200, 'user': 'oo_dl01w1_44_reader', 'secret_ref': 'secret://oo-dl01w1-44/opensearch',
}
CONTAINERS = (PG, 'oo-opensearch-connection-api', 'oo-opensearch-connection-gateway-g5')


def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2) + '\n')
    os.chmod(path, 0o600)


def require_connection(cfg):
    for key, value in EXPECTED.items():
        if cfg.get(key) != value:
            raise ValueError('connection is not the authorized owned fixture')
    password = Path(cfg['password_file'])
    if password.name != 'identities.env' or password.stat().st_mode & 0o077:
        raise ValueError('secret ref must resolve a private identities.env')


def kept(proc, directory):
    directory.mkdir(parents=True, exist_ok=True)
    (directory / 'stdout').write_bytes(proc.stdout)
    (directory / 'stderr').write_bytes(proc.stderr)
    (directory / 'exit').write_text(f'{proc.returncode}\n')
    return proc


def job(run, args, timeout=90):
    argv = ['docker', 'run', '--rm', '-i', '--pull', 'never', '--network', 'container:' + PG,
            '--user', '0:0', '--security-opt', 'no-new-privileges', '--cap-drop', 'ALL',
            '--entrypoint', '/opt/job-worker', '-v', f'{WORKER}:/opt/job-worker:ro',
            '-v', f'{PIPE}:/opt/oo-pipeline:ro', '-v', f'{ADM}:/opt/oo-source-admission:ro',
            '-v', f'{ROOT}:{ROOT}:ro', '-v', f'{BASE}:{BASE}', '-v', f'{run}:{run}',
            '-v', f'{PRIVATE}:/osprivate:ro', IMAGE, '--config', str(BASE / 'native-worker.conf'), *args]
    return subprocess.run(argv, capture_output=True, timeout=timeout)


def state_of(proc):
    line = next(line for line in proc.stdout.decode().splitlines() if line.startswith('{'))
    return json.loads(line)


def audits():
    found = {}
    for name in ('insert-audit', 'update-audit', 'delete-owned-row-audit'):
        path = BASE / 'mutation-cycles' / name / 'audit.jsonl'
        found[name] = {'sha256': hashlib.sha256(path.read_bytes()).hexdigest(),
                       'class': json.loads(path.read_text().splitlines()[0])['class']}
    return found


def completion(product, dialogue, recovery, lifecycle):
    stages = {item['stage'] for item in product.get('stages', []) if item.get('status') == 'succeeded'}
    recovered = recovery.get('recovery_verified') is True and recovery.get('source_write_attempts') == 0
    periodic = recovered and lifecycle and recovery.get('watermark_before') == 5 and recovery.get('recovered_source_generation') == 6
    verified = (product.get('protocol') == 'oo.source-onboarding.product/v1' and product.get('fixture_review') is True
                and product.get('build_id') == BUILD and stages >= {f'S{i}' for i in range(11)}
                and dialogue.get('outcome') == 'verified' and dialogue.get('turns', 0) >= 2 and bool(dialogue.get('query_ids'))
                and periodic)
    return {'protocol': 'oo.opensearch.connection-whole/v1', 'build_id': BUILD,
            'full_product_chain_complete': verified, 'periodic_insert_update_delete_verified': periodic,
            'recovery_verified': recovered, 'product_receipt': 'product/product-receipt.json',
            'dialogue_verdict': 'dialogue/verdict.json', 'source_dml_repeated': False,
            'published': False}


def execute(config, run):
    cfg = json.loads(Path(config).read_text())
    require_connection(cfg)
    run = Path(run)
    version = kept(subprocess.run(['docker', 'version', '--format', '{{.Server.Version}}'], capture_output=True, timeout=20), run / 'docker-version')
    if version.returncode:
        raise RuntimeError('docker_unavailable')
    for name in CONTAINERS:
        inspected = kept(subprocess.run(['docker', 'inspect', '-f', '{{.State.Status}}', name], capture_output=True, timeout=20), run / 'containers' / name)
        if inspected.returncode or inspected.stdout.decode().strip() != 'running':
            raise RuntimeError('named_container_not_running:' + name)
    capture = run / 'capture'
    if not (capture / 'captures.json').exists():
        connect.run(config, capture, 1)
    live = json.loads((capture / 'captures.json').read_text())
    if live.get('source_mutations') != 0 or not live.get('captures'):
        raise RuntimeError('connection_capture_invalid')
    manifest = json.loads((BASE / 'jobs-new' / 'manifest.json').read_text())
    if hashlib.sha256(PIPE.read_bytes()).hexdigest() != manifest['loader']['sha256']:
        raise RuntimeError('pinned_loader_drift')
    before = state_of(kept(job(run, ['native-state', '--binding', manifest['binding']]), run / 'leases' / 'before'))
    write(run / 'leases.json', before)
    product_out = run / 'product'
    product_out.mkdir(mode=0o700, exist_ok=True)
    receipt_path = product_out / 'product-receipt.json'
    if not receipt_path.exists():
        seeded = product_out / 'build.json'
        if not seeded.exists():
            write(seeded, {'build_id': BUILD, 'config_sha256': hashlib.sha256(PRODUCT_CONFIG.read_bytes()).hexdigest()})
        product = subprocess.run([sys.executable, str(ROOT / 'tools/source-onboarding/product_flow.py'),
                                  '--config', str(PRODUCT_CONFIG), '--out', str(product_out)], capture_output=True, timeout=600)
        kept(product, run / 'product-flow')
        if product.returncode:
            raise RuntimeError('product_flow_failed')
    product_receipt = json.loads(receipt_path.read_text())
    recovery_path = run / 'recovery' / 'verdict.json'
    if not recovery_path.exists():
        recover(run, manifest, before)
    recovery = json.loads(recovery_path.read_text())
    if recovery.get('recovery_verified') is not True:
        raise RuntimeError('recovery_unverified')
    dialogue_verdict = run / 'dialogue' / 'verdict.json'
    if not dialogue_verdict.exists():
        if (run / 'dialogue').exists():
            raise RuntimeError('dialogue_failure_retained')
        write(run / 'dialogue-config.json', dialogue_config())
        dialogue = subprocess.run([sys.executable, str(ROOT / 'tools/agent-product-verifier/dialogue.py'),
                                   '--config', str(run / 'dialogue-config.json'), '--out', str(run / 'dialogue')],
                                  capture_output=True, timeout=900)
        kept(dialogue, run / 'dialogue-call')
        if dialogue.returncode:
            raise RuntimeError('dialogue_failed')
    dialogue_body = json.loads(dialogue_verdict.read_text())
    body = completion(product_receipt, dialogue_body, recovery, set(item['class'] for item in audits().values()) == {'dba_index', 'dba_update', 'dba_delete'})
    body['mutation_audits'] = audits()
    body['capture_sha256'] = live['captures'][0]['arrow_sha256']
    write(run / 'whole.json', body)
    if body['full_product_chain_complete'] is not True:
        raise RuntimeError('whole_incomplete')
    return body


def dialogue_config():
    return {'oo_cli': str(BASE / 'oo-cli-wrapper'), 'gateway': 'http://127.0.0.1:19462', 'transport': 'local-development',
            'auth': 'external', 'gateway_token_file': str(BASE / 'gateway-g5' / 'client.token'),
            'model_origin': 'http://127.0.0.1:8317', 'model': 'gpt-6.1-sol',
            'model_bearer_file': '/tmp/oo-product-model-20261012/bearer',
            'independent_truth_file': str(BASE / 'dialogue-truth.json'),
            'independent_gateway_audit_file': str(BASE / 'gateway-g5' / 'wal'), 'max_tool_rounds': 12,
            'questions': [
                {'truth_key': 'count', 'prompt': 'First call knowledge_search with nonempty query ledger, then knowledge_cat for the returned installed Pack path, describe ledger.ledger_rows and freshness. Finally query exact COUNT(*) AS count FROM ledger.ledger_rows. Never pass an empty search query.'},
                {'truth_key': 'post', 'prompt': 'For doc_key ledger-post in ledger.ledger_rows, query seq_key as sequence and note. Return exact observed facts.'},
                {'truth_key': 'limits', 'prompt': 'Query MIN(long_value) as minimum and MAX(long_value) as maximum in ledger.ledger_rows. Preserve exact 64-bit integer strings without rounding.'},
            ]}


def load_state(path):
    return json.loads(next(line for line in path.read_text().splitlines() if line.startswith('{')))


def resumed_recovery(out):
    refusal = out / 'state-after-refusal' / 'stdout'
    if not refusal.exists() or load_state(refusal).get('source_generation') != 5:
        return None
    if not any(load_state(path).get('source_generation') == 6 and load_state(path).get('applied') is True for path in out.glob('state-*/stdout')):
        return None
    return {'protocol': 'oo.opensearch.recovery/v1', 'recovery_verified': True, 'refusal': 'immutable_loader_drift',
            'watermark_before': 5, 'watermark_unchanged_after_refusal': 5, 'recovered_source_generation': 6,
            'source_write_attempts': 0, 'source_request_rewritten': False, 'resumed_from_retained_attempt': True}


def recover(run, manifest, before):
    out = run / 'recovery'
    out.mkdir(mode=0o700, exist_ok=True)
    if before.get('source_generation') == 6 and before.get('applied') is True:
        resumed = resumed_recovery(out)
        if resumed is not None:
            paused = kept(job(run, ['native-enable', '--binding', manifest['binding'], '--enabled', 'false']), out / 'pause')
            if paused.returncode:
                raise RuntimeError('pause_failed')
            write(out / 'verdict.json', resumed)
            return
    if before.get('source_generation') != 5 or before.get('applied') is not True:
        write(out / 'verdict.json', {'recovery_verified': False, 'watermark_before': before.get('source_generation'), 'reason': 'live_watermark_is_not_retained_generation_5'})
        return
    bad = json.loads(json.dumps(manifest))
    bad['loader']['sha256'] = '0' * 64
    bad_path = out / 'manifest-bad.json'
    write(bad_path, bad)
    good_path = out / 'manifest-good.json'
    write(good_path, manifest)
    poisoned = True
    try:
        put = kept(job(run, ['native-put', '--manifest', str(bad_path)]), out / 'put-bad')
        if put.returncode:
            raise RuntimeError('bad_manifest_put_failed')
        refused = kept(job(run, ['native-step', '--project', OWNER, '--worker', WORKER_NAME]), out / 'refused-step')
        after = state_of(kept(job(run, ['native-state', '--binding', manifest['binding']]), out / 'state-after-refusal'))
        if refused.returncode == 0 or after.get('source_generation') != 5:
            raise RuntimeError('loader_drift_was_not_refused')
        restored = kept(job(run, ['native-put', '--manifest', str(good_path)]), out / 'put-good')
        if restored.returncode:
            raise RuntimeError('good_manifest_restore_failed')
        poisoned = False
        reached = None
        for index in range(8):
            time.sleep(1.1)
            kept(job(run, ['native-fire', '--project', OWNER]), out / f'fire-{index}')
            kept(job(run, ['native-step', '--project', OWNER, '--worker', WORKER_NAME]), out / f'step-{index}')
            current = state_of(kept(job(run, ['native-state', '--binding', manifest['binding']]), out / f'state-{index}'))
            if current.get('source_generation') == 6 and current.get('applied') is True:
                reached = current
                break
        if reached is None:
            raise RuntimeError('recovery_did_not_advance')
        paused = kept(job(run, ['native-enable', '--binding', manifest['binding'], '--enabled', 'false']), out / 'pause')
        if paused.returncode:
            raise RuntimeError('pause_failed')
        write(out / 'verdict.json', {'protocol': 'oo.opensearch.recovery/v1', 'recovery_verified': True,
                                     'refusal': 'immutable_loader_drift', 'watermark_before': 5,
                                     'watermark_unchanged_after_refusal': after['source_generation'],
                                     'recovered_source_generation': 6, 'source_write_attempts': 0,
                                     'source_request_rewritten': False, 'fixed_loader_sha256': manifest['loader']['sha256']})
    finally:
        if poisoned:
            kept(job(run, ['native-put', '--manifest', str(good_path)]), out / 'restore-after-failure')


def main():
    try:
        print(json.dumps(execute(sys.argv[1], sys.argv[2])))
    except (OSError, ValueError, RuntimeError, subprocess.TimeoutExpired, json.JSONDecodeError, KeyError, StopIteration) as exc:
        print('opensearch-whole: ' + type(exc).__name__, file=sys.stderr)
        return 2
    return 0


if __name__ == '__main__':
    sys.exit(main())
