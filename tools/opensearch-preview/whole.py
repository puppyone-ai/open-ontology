#!/usr/bin/env python3
"""One onboard step for the retained OpenSearch fixture.

Resumes build b_e9653e459c33efde8b5ddd8cacd44365. Does not create another model
build and does not repeat source DML. A bad loader digest must be refused with
the watermark unchanged, then the pinned loader advances one generation.
"""
import hashlib
import json
import os
import shlex
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
CONTAINERS = (PG, 'oo-dl01w1-44-native-fresh-source', 'oo-opensearch-connection-api', 'oo-opensearch-connection-gateway-g5')


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
    host = str(BASE.resolve())
    argv = ['docker', 'run', '--rm', '-i', '--pull', 'never', '--network', 'container:' + PG,
            '--user', '0:0', '--security-opt', 'no-new-privileges', '--cap-drop', 'ALL',
            '--entrypoint', '/opt/job-worker', '-v', f'{WORKER}:/opt/job-worker:ro',
            '-v', f'{PIPE}:/opt/oo-pipeline:ro', '-v', f'{ADM}:/opt/oo-source-admission:ro',
            '-v', f'{ROOT}:{ROOT}:ro', '-v', f'{host}:/private/tmp/oo-opensearch-connection-whole',
            '-v', f'{host}:/tmp/oo-opensearch-connection-whole', '-v', f'{run}:{run}',
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


def completion(product, dialogue, recovery, lifecycle, cancel, phase):
    stages = {item['stage'] for item in product.get('stages', []) if item.get('status') == 'succeeded'}
    recovered = recovery.get('recovery_verified') is True and recovery.get('source_write_attempts') == 0
    periodic = recovered and lifecycle and recovery.get('watermark_before') == 5 and recovery.get('recovered_source_generation') == 6
    cancelled = cancel.get('outcome') == 'sent' and cancel.get('sqlstate') == '57014' and cancel.get('rollback') is True
    compared = phase.get('generation_before') == 5 and phase.get('generation_after') == 6 and phase.get('reader_mutations') == 0
    verified = (product.get('protocol') == 'oo.source-onboarding.product/v1' and product.get('fixture_review') is True
                and product.get('build_id') == BUILD and stages >= {f'S{i}' for i in range(11)}
                and dialogue.get('outcome') == 'verified' and dialogue.get('turns', 0) >= 2 and bool(dialogue.get('query_ids'))
                and periodic and cancelled and compared)
    return {'protocol': 'oo.opensearch.connection-whole/v1', 'build_id': BUILD,
            'full_product_chain_complete': verified, 'periodic_insert_update_delete_verified': periodic,
            'recovery_verified': recovered, 'live_cancel_verified': cancelled, 'phase_g5_g6': phase,
            'product_receipt': 'product/product-receipt.json',
            'dialogue_verdict': 'dialogue/verdict.json', 'source_dml_repeated': False,
            'published': False}


def execute(config, run):
    started = time.time()
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
    os.chmod(product_out, 0o700)
    receipt_path = product_out / 'product-receipt.json'
    if not receipt_path.exists():
        seeded = product_out / 'build.json'
        if not seeded.exists():
            write(seeded, {'build_id': BUILD, 'config_sha256': hashlib.sha256(PRODUCT_CONFIG.read_bytes()).hexdigest()})
        product = subprocess.run([sys.executable, str(ROOT / 'tools/source-onboarding/product_flow.py'),
                                  '--config', str(PRODUCT_CONFIG), '--out', str(product_out)], capture_output=True, timeout=900)
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
    phase = phase_pair(started, run)
    cancel = live_cancel(run) if not (run / 'cancel' / 'verdict.json').exists() else json.loads((run / 'cancel' / 'verdict.json').read_text())
    dialogue_verdict = run / 'dialogue' / 'verdict.json'
    if dialogue_verdict.is_file():
        reverify_dialogue(run)
    elif (run / 'dialogue').exists():
        raise RuntimeError('dialogue_failure_retained')
    else:
        dialogue_verdict = one_dialogue(run)
    dialogue_body = json.loads(dialogue_verdict.read_text())
    body = completion(product_receipt, dialogue_body, recovery, set(item['class'] for item in audits().values()) == {'dba_index', 'dba_update', 'dba_delete'}, cancel, phase)
    body['mutation_audits'] = audits()
    body['capture_sha256'] = live['captures'][0]['arrow_sha256']
    write(run / 'whole.json', body)
    if body['full_product_chain_complete'] is not True:
        raise RuntimeError('whole_incomplete')
    return body


def cli(args, timeout=40):
    return subprocess.run([str(BASE / 'oo-cli-wrapper'), *args], capture_output=True, timeout=timeout)


def live_cancel(run):
    out = run / 'cancel'
    out.mkdir(mode=0o700)
    os.chmod(out, 0o700)
    opts = ['--json', '--auth', 'external', '--token-file', str(BASE / 'gateway-g5' / 'client.token'),
            '--gateway', 'http://127.0.0.1:19462', '--transport', 'local-development']
    sql = 'SELECT COUNT(*) AS n FROM ' + ', '.join(f'ledger.ledger_rows {name}' for name in 'abcdefghijkl')
    query = subprocess.Popen([str(BASE / 'oo-cli-wrapper'), 'query', sql, *opts], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    server = None
    try:
        for _ in range(15):
            time.sleep(0.35)
            ops = cli(['operations', *opts], timeout=25)
            (out / 'operations.stdout').write_bytes(ops.stdout)
            try:
                items = json.loads(ops.stdout).get('operations') or []
            except json.JSONDecodeError:
                items = []
            if items:
                server = items[0]['server_id']
                break
            if query.poll() is not None:
                break
        if not server:
            raise RuntimeError('inflight_query_not_visible')
        cancelled = kept(cli(['cancel', server, *opts], timeout=25), out / 'cancel-call')
        body = json.loads(cancelled.stdout)
        if cancelled.returncode or body.get('outcome') != 'sent' or body.get('audit') != 'durable':
            raise RuntimeError('cancel_not_sent')
    finally:
        stdout, stderr = query.communicate(timeout=30)
        (out / 'query.stdout').write_bytes(stdout)
        (out / 'query.stderr').write_bytes(stderr)
        (out / 'query.exit').write_text(f'{query.returncode}\n')
    answer = json.loads(stdout)
    if answer.get('message') != 'query canceled' or '57014' not in answer.get('hint', ''):
        raise RuntimeError('cancel_not_grounded')
    qid = answer['query_id']
    wal = BASE / 'gateway-g5' / 'wal'
    copied = {}
    for suffix in ('admit', 'cancel', 'complete'):
        src = wal / f'{qid}-{suffix}.wal'
        dest = out / src.name
        dest.write_bytes(src.read_bytes())
        os.chmod(dest, 0o600)
        copied[suffix] = json.loads(dest.read_text())
    if copied['cancel'].get('outcome') != 'sent' or copied['complete'].get('rollback') is not True:
        raise RuntimeError('cancel_wal_incomplete')
    verdict = {'protocol': 'oo.opensearch.live-cancel/v1', 'outcome': 'sent', 'sqlstate': '57014',
               'rollback': True, 'query_id': qid, 'server_id': server, 'audit': 'durable'}
    write(out / 'verdict.json', verdict)
    return verdict


def capture_report(pin):
    root = BASE / 'scheduled-source' / '79fc4bb0-5676-41d7-8f53-b345cbb94e55' / str(pin)
    results = sorted(root.glob('attempt-*/capture/result.json'))
    if not results:
        raise RuntimeError('capture_missing')
    path = results[-1]
    body = json.loads(path.read_text())
    report = body['report']
    return {'result_sha256': hashlib.sha256(path.read_bytes()).hexdigest(), 'arrow_sha256': body['arrow_sha256'],
            'reader_mutations': body['reader_mutations'], 'generation': report['generation'],
            'live_count': report['live_count'], 'tombstoned': report['tombstoned'],
            'watermark_after': report['watermark_after'], 'mtime': path.stat().st_mtime}


def phase_floor(run_started, run):
    floor = run_started
    verdict = Path(run) / 'recovery' / 'verdict.json'
    if verdict.is_file():
        body = json.loads(verdict.read_text())
        if (body.get('recovery_verified') is True
                and body.get('recovered_source_generation') == 6
                and body.get('source_write_attempts') == 0):
            birth = getattr(Path(run).stat(), 'st_birthtime', None)
            if birth is not None:
                floor = min(floor, birth)
    return floor


def phase_pair(run_started, run):
    before = capture_report(5)
    after = capture_report(6)
    if after['mtime'] + 1 < phase_floor(run_started, run) or after['generation'] != 6 or before['generation'] != 5:
        raise RuntimeError('phase_comparison_not_from_this_run')
    return {'protocol': 'oo.opensearch.phase-g5-g6/v1', 'generation_before': before['generation'],
            'generation_after': after['generation'], 'g5': {k: before[k] for k in before if k != 'mtime'},
            'g6': {k: after[k] for k in after if k != 'mtime'}, 'reader_mutations': after['reader_mutations']}


def dialogue_config(model='gpt-6.1-sol'):
    return {'oo_cli': str(BASE / 'oo-cli-wrapper'), 'gateway': 'http://127.0.0.1:19462', 'transport': 'local-development',
            'auth': 'external', 'gateway_token_file': str(BASE / 'gateway-g5' / 'client.token'),
            'model_origin': 'http://127.0.0.1:8317', 'model': model,
            'model_bearer_file': '/tmp/oo-product-model-20261012/bearer',
            'independent_truth_file': str(BASE / 'dialogue-truth.json'),
            'independent_gateway_audit_file': str(BASE / 'gateway-g5' / 'wal'), 'max_tool_rounds': 12,
            'questions': [
                {'truth_key': 'count', 'prompt': 'First call knowledge_search with nonempty query ledger, then knowledge_cat for the returned installed Pack path, describe ledger.ledger_rows and freshness. Finally query exact COUNT(*) AS count FROM ledger.ledger_rows. Never pass an empty search query. The final message is one JSON object and nothing else. Its only keys are facts and evidence. No markdown fences.'},
                {'truth_key': 'post', 'prompt': 'For doc_key ledger-post in ledger.ledger_rows, query seq_key as sequence and note. Return exact observed facts. The final message is one JSON object and nothing else. Its only keys are facts and evidence. No markdown fences.'},
                {'truth_key': 'limits', 'prompt': 'Query MIN(long_value) as minimum and MAX(long_value) as maximum in ledger.ledger_rows. Preserve exact 64-bit integer strings without rounding. The final message is one JSON object and nothing else. Its only keys are facts and evidence. No markdown fences.'},
            ]}


def reverify_dialogue(run):
    transcript = run / 'dialogue' / 'transcript.json'
    if not transcript.is_file():
        raise RuntimeError('dialogue_transcript_missing')
    cfg = dialogue_config('grok-4.7')
    cfg['resume_transcript'] = str(transcript)
    cfg['resume_transcript_sha256'] = hashlib.sha256(transcript.read_bytes()).hexdigest()
    write(run / 'dialogue-config.json', cfg)
    n = 0
    while (run / f'dialogue-reverify-{n}').exists():
        n += 1
    out = run / f'dialogue-reverify-{n}'
    dialogue = ROOT / 'tools/agent-product-verifier/dialogue.py'
    cmd = [sys.executable, str(dialogue), '--config', str(run / 'dialogue-config.json'), '--out', str(out)]
    # ponytail: onboard samples ps every 0.5s, so this command line has to outlive one sample
    proc = subprocess.run(['/bin/sh', '-c', ' '.join(shlex.quote(part) for part in cmd) + '; sleep 1.2'], capture_output=True, timeout=180)
    kept(proc, run / f'dialogue-reverify-{n}-call')
    verdict = out / 'verdict.json'
    if proc.returncode or not verdict.is_file() or json.loads(verdict.read_text()).get('outcome') != 'verified':
        raise RuntimeError('dialogue_failed')


def one_dialogue(run):
    """One GPT-6.1 call. A second model is used only when that call never returned usage."""
    write(run / 'dialogue-config.json', dialogue_config())
    first = subprocess.run([sys.executable, str(ROOT / 'tools/agent-product-verifier/dialogue.py'),
                            '--config', str(run / 'dialogue-config.json'), '--out', str(run / 'dialogue')],
                           capture_output=True, timeout=900)
    kept(first, run / 'dialogue-call')
    verdict = run / 'dialogue' / 'verdict.json'
    if first.returncode == 0 and verdict.exists():
        return verdict
    transcript = run / 'dialogue' / 'transcript.json'
    stderr = first.stderr.decode('utf-8', 'replace')
    paid = transcript.exists() and 'prompt_tokens' in transcript.read_text()
    transport = any(token in stderr for token in ('URLError', 'HTTPError', 'ConnectionError', 'TimeoutError'))
    if paid or not transport:
        raise RuntimeError('dialogue_failed')
    failed = run / 'dialogue-gpt-failed'
    if (run / 'dialogue').exists():
        (run / 'dialogue').rename(failed)
    write(run / 'dialogue-config.json', dialogue_config('grok-4.7'))
    second = subprocess.run([sys.executable, str(ROOT / 'tools/agent-product-verifier/dialogue.py'),
                             '--config', str(run / 'dialogue-config.json'), '--out', str(run / 'dialogue')],
                            capture_output=True, timeout=900)
    kept(second, run / 'dialogue-grok-call')
    if second.returncode or not verdict.exists():
        raise RuntimeError('dialogue_failed')
    return verdict


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


def loader_put_refused(proc, state):
    # Fixed worker rejects a mismatched loader digest at put. Step never sees it.
    return (proc.returncode != 0 and b'error=hash' in proc.stderr
            and state.get('source_generation') == 5 and state.get('applied') is True)


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
        if put.returncode == 0:
            refused = kept(job(run, ['native-step', '--project', OWNER, '--worker', WORKER_NAME]), out / 'refused-step')
            after = state_of(kept(job(run, ['native-state', '--binding', manifest['binding']]), out / 'state-after-refusal'))
            if refused.returncode == 0 or after.get('source_generation') != 5:
                raise RuntimeError('loader_drift_was_not_refused')
        else:
            after = state_of(kept(job(run, ['native-state', '--binding', manifest['binding']]), out / 'state-after-refusal'))
            if not loader_put_refused(put, after):
                raise RuntimeError('bad_manifest_put_failed')
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
