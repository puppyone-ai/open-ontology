#!/usr/bin/env python3
"""Real source producer for owner74's durable native scheduler.

Executes original source driver and the pinned source-admission authority. It
never imports the target, claims a job, or writes a trusted certificate itself.
Only the derived native request is stdout. Configuration contains private file
references and fixed executable identities, never inline credentials.
"""
import argparse
import contextlib
import fcntl
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import shutil
import sys
import uuid


def execute(root, name, argv):
    out = root / name
    out.mkdir(exist_ok=False)
    result = subprocess.run(argv, capture_output=True)
    (out / 'stdout').write_bytes(result.stdout)
    (out / 'stderr').write_bytes(result.stderr)
    (out / 'exit').write_text(str(result.returncode) + '\n')
    if result.returncode:
        raise RuntimeError(f'{name}: exit{result.returncode}; raw retained at {out}')
    return result.stdout


def checked(path, expected):
    path = Path(path)
    if hashlib.sha256(path.read_bytes()).hexdigest() != expected:
        raise RuntimeError('fixed executable digest mismatch')
    return str(path)


def produce(cfg, incoming):
    if incoming['protocol'] != 'oo.jobs.native-producer/v1' or incoming['source_id'] != 'oo_dl01w1_44' or incoming['method'] != 'C5':
        raise RuntimeError('producer identity/method mismatch')
    if incoming['cycle'] not in ('pull', 'reconcile') or not 1000 <= incoming['period_ms'] <= 86400000:
        raise RuntimeError('unsupported cycle/period')
    reader = checked(cfg['reader_bin'], cfg['reader_sha256'])
    sealer = checked(cfg['admission_bin'], cfg['admission_sha256'])
    worker = checked(cfg['worker_bin'], cfg['worker_sha256'])
    uuid.UUID(incoming['binding'])
    if not isinstance(incoming['pin'], int) or incoming['pin'] < 1:
        raise RuntimeError('invalid durable pin')
    root = Path(cfg['capture_root'])
    root.mkdir(parents=True, exist_ok=True)
    with (root / 'producer.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        package = root / incoming['binding'] / str(incoming['pin'])
        package.mkdir(parents=True, exist_ok=True)
        complete = package / 'loader-input.json'
        if complete.exists():
            if json.loads((package / 'worker-input.json').read_text()) != incoming:
                raise RuntimeError('durable binding/pin input drift')
            return json.loads(complete.read_text())
        run = package / f'attempt-{uuid.uuid4()}'
        run.mkdir()
        acknowledged = json.loads(execute(run, 'acknowledged-state-call', [worker,
            '--config', cfg['worker_config'], 'native-state', '--binding', incoming['binding']]))
        if acknowledged['source_id'] != incoming['source_id'] or acknowledged['method'] != 'C5':
            raise RuntimeError('acknowledged source identity mismatch')
        generation = (acknowledged['source_generation'] or 0) + 1
        (run / 'worker-input.json').write_text(json.dumps(incoming) + '\n')
        schedule = json.loads(Path(cfg['schedule_path']).read_text())
        schedule['schedules'] = [{'name': 'worker', 'every_ms': incoming['period_ms'],
            'reconcile_every': 1 if incoming['cycle'] == 'reconcile' else 2}]
        schedule_path = run / 'effective-c5-schedule.json'
        schedule_path.write_text(json.dumps(schedule) + '\n')
        capture = run / 'capture'
        execute(run, 'capture-call', [reader, 'c5-cycle', '--schedule', str(schedule_path),
            '--private', cfg['private_path'], '--index', schedule['approved_index'],
            '--shadow', str(root / 'source-shadow.json'), '--schedule-name', 'worker',
            '--page-size', '2', '--out', str(capture)])
        result = json.loads((capture / 'result.json').read_text())
        if result.get('reader_mutations') != 0 or not result['report']['applied']:
            raise RuntimeError('source capture not applied/zero-write')
        receipt_dir = run / 'producer-request'
        receipt_dir.mkdir()
        target = cfg['target']
        args = [reader, 'c5-receipt', '--arrow', str(capture / 'batch.arrow'),
            '--audit', str(capture / 'audit.jsonl'), '--config', str(schedule_path),
            '--catalog', str(capture / 'live.json'), '--registry', cfg['approved_registry_path'],
            '--driver', reader, '--snapshot', str(capture / 'snapshot.json'),
            '--result', str(capture / 'result.json'), '--generation', str(generation),
            '--capture-watermark', str(result['report']['watermark_after']),
            '--batch-id', f't{generation}.b1', '--logical-schema', 'ledger',
            '--logical-table', 'ledger_rows', '--key-column', 'source_doc_id',
            '--host', target['host'], '--port', str(target['port']), '--database', target['database'],
            '--runtime-principal', target['runtime_principal'], '--password-file', target['password_file'],
            '--arrow-path', str(capture / 'batch.arrow'), '--admission-path', str(receipt_dir / 'receipt.json'),
            '--out-receipt', str(receipt_dir / 'receipt.json'), '--out-request', str(receipt_dir / 'request.json')]
        if generation > 1:
            prior_package = root / incoming['binding'] / str(acknowledged['pin'])
            prior_request = json.loads((prior_package / 'loader-input.json').read_text())
            prior_snapshot = str(Path(prior_request['arrow_path']).with_name('snapshot.json'))
            tombstoned = set(result['report']['tombstoned'])
            if tombstoned:
                for pin in range(acknowledged['pin'], 0, -1):
                    prior_request = json.loads((root / incoming['binding'] / str(pin) / 'loader-input.json').read_text())
                    candidate = Path(prior_request['arrow_path']).with_name('snapshot.json')
                    docs = json.loads(candidate.read_text())['docs']
                    keys = {json.loads(doc['raw'])['doc_key'] for doc in docs}
                    if tombstoned <= keys:
                        prior_snapshot = str(candidate)
                        break
                else:
                    raise RuntimeError('tombstone has no sealed historical source ID')
            args += ['--parent', str(generation - 1), '--prior-snapshot', prior_snapshot]
        execute(run, 'receipt-call', args)
        witness_path = checked(cfg['witness_module'], cfg['witness_sha256'])
        spec = importlib.util.spec_from_file_location('witness', witness_path)
        witness = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(witness)
        witness.OUT = run / 'witness'
        witness.ARROW = capture / 'batch.arrow'
        witness.AUDIT = capture / 'audit.jsonl'
        witness.PROOF = capture
        witness.DRIVER = Path(reader)
        witness.LOAD = receipt_dir / 'request.json'
        witness.SCHEDULE = schedule_path
        actual_receipt = json.loads((receipt_dir / 'receipt.json').read_text())
        with (run / 'witness.stdout').open('w') as diagnostics, contextlib.redirect_stdout(diagnostics):
            witness.main(generation=generation, run_id=actual_receipt['run_id'], snapshot_identity=actual_receipt['snapshot'])
        admission = run / 'admission'
        execute(run, 'admission-call', [sealer, 'seal-native', '--report', str(witness.OUT / 'report.json'),
            '--config', str(witness.OUT / 'config.json'), '--out', str(admission)])
        decision = json.loads((admission / 'decision.json').read_text())
        if not decision['admitted']:
            raise RuntimeError('source authority refused')
        request = json.loads((witness.OUT / 'import-request.json').read_text())
        authority = json.loads((admission / 'admission-receipt.json').read_text())
        for field in ('source_id', 'source_generation', 'snapshot', 'input_digest'):
            if request[field] != authority[field]:
                raise RuntimeError('authority/source identity mismatch')
        for field in ('config_digest', 'schema_digest', 'type_digest'):
            request[field] = authority[field]
        registry = json.loads((admission / 'registry.json').read_text())
        if len(registry['bindings']) != 1:
            raise RuntimeError('expected one original source index binding')
        request['audit_digest'] = registry['bindings'][0]['capture_receipt_sha256']
        request['target'] = target
        # Every C5 run has complete typed live rows and no ordered source cursor.
        # Full-snapshot replacement keeps the real snapshot-zero origin unchanged.
        request['full_snapshot'] = True
        request['mirror'] = True
        request['tombstones'] = []
        request['admission_path'] = str(admission / 'admission-receipt.json')
        request['admission_expect'] = {'declared_receipt_sha256': hashlib.sha256((admission / 'admission-receipt.json').read_bytes()).hexdigest()}
        (run / 'loader-input.json').write_text(json.dumps(request) + '\n')
        published = package / 'admission'
        published.mkdir(exist_ok=True)
        # Publish byte-identical authority output, retaining the authority's originals.
        for name in ('registry.json', 'catalog.json', 'admission-receipt.json', 'decision.json'):
            source = admission / name
            temporary = published / (name + '.next')
            shutil.copyfile(source, temporary)
            temporary.replace(published / name)
        (package / 'worker-input.json').write_text(json.dumps(incoming) + '\n')
        temporary = package / 'loader-input.next'
        temporary.write_text(json.dumps(request) + '\n')
        temporary.replace(complete)
        return request


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--config', required=True)
    parser.add_argument('--config-sha256', required=True)
    args = parser.parse_args()
    incoming = json.load(sys.stdin)
    config_bytes = Path(args.config).read_bytes()
    if hashlib.sha256(config_bytes).hexdigest() != args.config_sha256:
        raise SystemExit('fixed operator producer configuration drift')
    cfg = json.loads(config_bytes)
    try:
        print(json.dumps(produce(cfg, incoming)))
    except Exception as error:
        print(str(error), file=sys.stderr)
        raise SystemExit(1)


if __name__ == '__main__':
    main()
