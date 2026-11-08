"""Single-owner retained cycles, queued and timed by the control database."""
import argparse
import fcntl
import hashlib
import json
import re
import os
from pathlib import Path
import ssl
import subprocess
import sys
import threading
import time
import urllib.request
import uuid

# Closed refusal codes the loader prints for a candidate (publication_*).
PUBLICATION_REFUSAL = re.compile(r'publication_[a-z0-9_]{1,64}')

KIND = 'lite_retained'


def save(path, value):
    path = Path(path)
    temp = path.with_name('.' + path.name + '-' + uuid.uuid4().hex)
    with temp.open('x') as stream:
        os.chmod(temp, 0o600)
        json.dump(value, stream, separators=(',', ':'), sort_keys=True)
        stream.flush()
        os.fsync(stream.fileno())
    temp.replace(path)


def sql(config, text):
    result = subprocess.run(config['control_psql_command'] + ['-X', '-qAt', '-v', 'ON_ERROR_STOP=1'],
                            input='SET ROLE oo_jobs_runtime;\n' + text, text=True,
                            capture_output=True, timeout=15)
    if result.returncode:
        raise ValueError('control_database_failed')
    return result.stdout.strip()


def project(config):
    value = config['project']
    if str(uuid.UUID(value)) != value:
        raise ValueError('schedule_project')
    return value


def fire(config):
    owner = project(config)
    interval = config['interval_seconds']
    if type(interval) is not int or not 1 <= interval <= 86400:
        raise ValueError('schedule_interval')
    # Lock the occurrence and advance its deadline in the enqueue transaction.
    # A backlog is coalesced, never run concurrently against the same target.
    sql(config, f"""
BEGIN;
DO $body$
DECLARE s record; task uuid;
BEGIN
 FOR s IN SELECT * FROM jobs.schedules WHERE project_id='{owner}'::uuid
  AND kind='{KIND}' AND enabled AND next_run_at<=clock_timestamp()
  ORDER BY next_run_at FOR UPDATE SKIP LOCKED
 LOOP
  IF EXISTS(SELECT 1 FROM jobs.tasks WHERE project_id=s.project_id
    AND kind='{KIND}' AND state IN ('queued','running')) THEN CONTINUE; END IF;
  SELECT task_id INTO task FROM jobs.enqueue(s.project_id,s.kind,1,'{{}}',
   s.config_version,s.artifact_digest,s.id::text||':'||s.next_run_at::text,s.next_run_at,1);
  INSERT INTO jobs.schedule_occurrences(schedule_id,scheduled_at,project_id,task_id)
   VALUES(s.id,s.next_run_at,s.project_id,task);
  UPDATE jobs.schedules SET next_run_at=clock_timestamp()+interval '{interval} seconds',
   updated_at=clock_timestamp() WHERE id=s.id;
 END LOOP;
END $body$;
COMMIT;
""")


def command(argv, output, label, data=None):
    result = subprocess.run([str(a) for a in argv], input=data, capture_output=True, timeout=1800)
    # Product commands return sanitized structured reports, not credentials.
    (output / (label + '.stdout')).write_bytes(result.stdout)
    (output / (label + '.stderr')).write_bytes(result.stderr)
    return result


def data_only(config, generation, output, stale_tables=()):
    # Reuse the retained Builder's observation, digest and review binding, but
    # never enter its model drafting branch from an unattended schedule.
    scripts = Path(__file__).resolve().parent / '../../tools/pg-builder/scripts'
    sys.path.insert(0, str(scripts if scripts.exists() else Path('/opt/oo/pg-builder')))
    import retained_build as builder
    build = dict(config['builder'], generation=generation)
    if build['library_id'] != config['library']:
        raise ValueError('builder_library_mismatch')
    origin = build['origin']
    old = json.loads(builder.pinned(origin['facts']))
    reviewed = builder.pinned(origin['reviewed_registry'])
    db = builder.lib.ReadOnlyPostgres(build['psql_command'], build.get('statement_timeout_ms', 30000))
    if config.get('source_catalog') == 'export':
        path = output / 'source-catalog.json'
        try:
            exported = command(['/usr/local/bin/oo', 'admin', 'source-catalog',
                                '--stack', config['stack'], '--source', config['source'],
                                '--secrets-dir', config['secrets_dir'], '--output', path],
                               output, 'source-catalog')
            if exported.returncode:
                raise ValueError('source_catalog_export_failed')
            pin = json.loads(exported.stdout)
            if (pin['path'] != str(path) or path.is_symlink()
                    or path.stat().st_mode & 0o777 != 0o600
                    or hashlib.sha256(path.read_bytes()).hexdigest() != pin['sha256']):
                raise ValueError('source_catalog_export_failed')
            build['source_catalog'] = pin
        except Exception:
            raise ValueError('source_catalog_export_failed') from None
    facts, provenance = builder.facts_for(db, build, previous=old, stale_tables=stale_tables)
    builder.save(output / 'facts.json', facts)
    digest = builder.semantics.data_only_digest(facts)
    builder.reviewed_shape(json.loads(reviewed), facts)
    pending = builder.semantics.table_review_pending(old, facts)
    # Table-scoped changes keep the rest of the library publishing; changes in
    # the library-wide remainder (inferred edges, reports, ...) stop the cycle.
    library = builder.semantics.library_review_pending(old, facts, config['library'])
    return {'stage': 'review_required' if library else 'data_only', 'model_calls': 0,
            'published': False, 'review_pending': library + pending,
            'table_data_only_digests': builder.semantics.table_data_only_digests(facts),
            'library_data_only_digest': builder.semantics.library_data_only_digest(facts),
            'receipt_sha256': provenance['receipt_sha256'], 'data_only_digest': digest}


def gateway_probe(config, receipt):
    client = config['gateway_probe']
    context = ssl.create_default_context(cafile=client['ca'])
    context.load_cert_chain(client['cert'], client['key'])
    token = Path(client['token_file']).read_text().strip()
    request = urllib.request.Request(client['url'].rstrip('/') + '/v1/freshness',
                                     headers={'Authorization': 'Bearer ' + token})
    with urllib.request.urlopen(request, context=context, timeout=10) as response:
        report = json.load(response)
    if report.get('publication') != 'live' or not any(
            row.get('receipt_sha256') == receipt['receipt_sha256']
            for row in report.get('library_publications', [])):
        raise ValueError('gateway_reload_pending')
    return {'receipt_sha256': receipt['receipt_sha256'], 'measured_at': timestamp()}


def cycle(config, generation, output, guard=lambda: None, active_generation=lambda: None):
    result = {'stage': 'failed', 'result': 'failed', 'reason': None,
              'stale_tables': [], 'review_pending': [], 'data_published_at': None,
              'data_verified_at': None, 'published_generation': None}
    try:
        return run_cycle(config, generation, output, guard, result, active_generation)
    except Exception as error:
        # Carry activation evidence even when a later Gateway probe/prune fails.
        allowed = {'sync_failed', 'all_tables_stale', 'candidate_failed', 'publish_failed',
                   'gateway_reload_failed', 'prune_failed', 'source_catalog_export_failed',
                   'retained_origin_shape_drift', 'lease_lost', 'mixed_retained_runs'}
        result['stage'] = result['result'] = 'failed'
        named = str(error).startswith('candidate_failed:') and PUBLICATION_REFUSAL.fullmatch(
            str(error).split(':', 1)[1])
        result['reason'] = str(error) if str(error) in allowed or named else 'retained_cycle_failed'
        save(output / 'result.json', result)
        raise

def run_cycle(config, generation, output, guard, result, active_generation=lambda: None):
    output.mkdir(mode=0o700)
    common = ['--stack', config['stack'], '--source', config['source'], '--target', config['target'],
              '--secrets-dir', config['secrets_dir']]
    loader = config.get('loader', 'oo-load')
    guard()
    c5 = output / 'c5.json'
    # Every cycle compares per-chunk source digests; only chunks whose digest
    # changed are re-read, plus a bounded periodic full read (default every
    # 24 cycles) that still catches a digest collision (plan D24: keep source
    # pressure low). No guessed CDC position.
    every = c5_full_read_every(config)
    save(c5, {'cycle': next_c5_cycle(output.parent, every), 'full_read_every': every})
    sync = command([loader, 'sync', *common, '--run-id', generation, '--c5', c5], output, 'sync')
    if sync.returncode not in (0, 3, 4):
        raise ValueError('sync_failed')
    reports = [json.loads(line) for line in sync.stdout.splitlines() if line.strip()]
    result['stale_tables'] = [
        {'table': (row['table']['schema'] + '.' + row['table']['name']
                   if isinstance(row['table'], dict) else row['table']),
         'reason': row.get('stale_reason', 'unknown'),
         'detail': row.get('stale_detail'), 'since': timestamp()}
        for row in reports if row.get('status') == 'stale']
    if sync.returncode == 4:
        raise ValueError('all_tables_stale')
    current = generation
    retained_runs = {row['retained_run'] for row in reports if row.get('status') != 'stale'}
    fresh_runs = {row['retained_run'] for row in reports if row.get('status') == 'fresh'}
    if len(fresh_runs) > 1:
        raise ValueError('mixed_retained_runs')
    if fresh_runs:
        # No-op and stale tables may keep older cuts in a partial candidate.
        anchor = fresh_runs.pop()
    elif len(retained_runs) == 1:
        # C5 no-ops retain the sealed parent, not a fictitious new run.
        anchor = retained_runs.pop()
    else:
        # Every table is unchanged or stale, on different retained runs (or none
        # healthy at all): re-confirm the active publication if there is one.
        anchor = active_generation() or current
    guard()
    # Per-table outcomes always come from this cycle. oo-load replays `anchor`
    # only when its recorded outcomes equal this cycle's; otherwise it mints
    # the cut at this cycle's run, which keeps each table's retained data.
    prepared = command([loader, 'publish', *common, '--generation', anchor,
                        '--outcome-run', current, '--library', config['library']], output, 'candidate')
    candidate = json.loads(prepared.stdout)
    if prepared.returncode:
        try:
            refused = json.loads(prepared.stderr).get('error')
        except (ValueError, AttributeError):
            refused = None
        if refused != 'publication_semantic_admission_required' or candidate.get('state') != 'pending':
            # Name the loader's closed refusal (e.g. a tampered published view)
            # so `oo status` shows the cause, not a generic failure.
            if isinstance(refused, str) and PUBLICATION_REFUSAL.fullmatch(refused):
                raise ValueError('candidate_failed:' + refused)
            raise ValueError('candidate_failed')
    generation = candidate.get('run', anchor)
    if generation not in (anchor, current):
        raise ValueError('candidate_failed')
    result['published_generation'] = generation
    guard()
    build = data_only(config, generation, output, [t['table'] for t in result['stale_tables']])
    result['review_pending'] = build.get('review_pending', [])
    save(output / 'builder.json', build)
    if build['stage'] == 'review_required':
        return dict(result, stage='review_required', result='review_required',
                    reason='semantic_review_required', model_calls=0)
    guard()
    # Preserve reviewed bytes: the publisher verifies the exact reviewed compiler output.
    import retained_build as builder
    reviewed = builder.pinned(config['builder']['origin']['reviewed_registry'])
    body = json.dumps({'protocol': 'oo.release-coordinator/publish-retained/v1',
                       'library_id': config['library'], 'generation': generation,
                       'login': config['login']}).encode()[:-1] + b',"reviewed_registry":' + reviewed + b'}'
    published = command([config.get('publisher', 'release-coordinator'), 'publish-retained',
                         '--config', config['publisher_config']], output, 'publisher', body)
    receipt = json.loads(published.stdout)
    if published.returncode or receipt.get('state') != 'ready' or receipt.get('outcome') not in ('activated', 'replay'):
        raise ValueError('publish_failed')
    if receipt['outcome'] == 'activated':
        result['data_published_at'] = timestamp()
    guard()
    document = receipt['gateway_document']
    # Data-only cycles have an identical Pack. Avoid needless replacements while
    # concurrent requests stamp the catalog; document replacement is atomic.
    pack = document['envelope']['producer']['artifact']
    path = Path(config['pack_path'])
    if not path.exists() or json.loads(path.read_bytes()) != pack:
        save(path, pack)
    save(config['document_path'], document)
    for attempt in range(30):
        guard()
        try:
            measured = gateway_probe(config, receipt)
            break
        except Exception:
            if attempt == 29:
                raise ValueError('gateway_reload_failed') from None
            time.sleep(1)
    save(output / 'gateway.json', measured)
    # The Gateway now serves this receipt. Its data is current as of this cycle
    # only if every table it calls fresh/noop was read and verified now.
    if receipt_matches_cycle(candidate, reports):
        result['data_verified_at'] = measured.get('measured_at') or timestamp()
    guard()
    pruned = command([loader, 'prune', *common, '--library', config['library'],
                      '--retain-seconds', str(config.get('retain_seconds', 86400))], output, 'prune')
    if pruned.returncode:
        raise ValueError('prune_failed')
    stage = 'partial' if sync.returncode == 3 else 'succeeded'
    return dict(result, stage=stage, result=stage, reason='stale_tables' if stage == 'partial' else None,
                model_calls=0, **measured)

def table_name(table):
    return table['schema'] + '.' + table['name'] if isinstance(table, dict) else table


def receipt_matches_cycle(candidate, reports):
    """Whether the cut records exactly this cycle's per-table health."""
    try:
        shards = candidate['receipt']['shards']
        tables = shards[0]['receipt']['tables'] if len(shards) == 1 else None
        published = {t['schema'] + '.' + t['table']: t['status'] for t in tables}
    except (KeyError, TypeError, IndexError):
        return False
    observed = {table_name(row['table']): row.get('status') for row in reports if 'table' in row}
    healthy = ('fresh', 'noop')
    return (len(published) == len(tables) and published.keys() == observed.keys()
            and all((status in healthy and observed[name] in healthy)
                    or (status == 'stale' and observed[name] == 'stale')
                    for name, status in published.items()))


def complete(config, binding, result):
    payload = dict(reason=None, stale_tables=[], review_pending=[], data_published_at=None,
                   data_verified_at=None, published_generation=None)
    payload.update(result, result=result['stage'])
    encoded = json.dumps(payload, separators=(',', ':')).replace("'", "''")
    return sql(config, f"SELECT jobs.complete_lite_cycle({binding},'{encoded}'::jsonb);")


C5_FULL_READ_EVERY = 24


def c5_full_read_every(config):
    value = config.get('c5_full_read_every', C5_FULL_READ_EVERY)
    if not isinstance(value, int) or isinstance(value, bool) or not 1 <= value <= 10000:
        raise ValueError('schedule_c5_full_read_every')
    return value


def next_c5_cycle(root, every):
    """Monotonic per-deployment cycle number. oo-load fully re-reads when the
    number is a multiple of `every`; the first cycle is one of them."""
    path = root / 'c5-cycle.json'
    try:
        previous = int(json.loads(path.read_text())['cycle'])
    except (OSError, ValueError, KeyError, TypeError):
        previous = every - 1
    current = max(previous, 0) + 1
    save(path, {'cycle': current})
    return current


def last_published_generation(config):
    """The anchor run of the newest cycle that activated or replayed a publication."""
    project_id = project(config)
    row = sql(config, "SELECT payload->>'published_generation' FROM jobs.lite_cycle_events "
                      f"WHERE project_id='{project_id}' AND payload->>'published_generation' IS NOT NULL "
                      "AND payload->>'data_verified_at' IS NOT NULL ORDER BY event_id DESC LIMIT 1;").strip()
    return row if row and all(c.isalnum() or c in '_-' for c in row) and len(row) <= 64 else None


def timestamp():
    return time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())


def work(config):
    owner = project(config)
    root = Path(config['work_dir'])
    root.mkdir(mode=0o700, parents=True, exist_ok=True)
    # The lite work volume is the single-owner exclusion boundary, including
    # after lease loss. No new task can overlap an old subprocess on this volume.
    with (root / 'worker.lock').open('a') as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            return {'stage': 'busy'}
        lease = json.loads(sql(config, f"SELECT row_to_json(t) FROM jobs.claim('{owner}',ARRAY['{KIND}'],'lite-worker') t;"))
        if lease['status'] != 'claimed':
            return {'stage': 'idle'}
        task = str(uuid.UUID(lease['task_id']))
        generation = int(lease['generation'])
        binding = f"'{owner}','{task}','lite-worker',{generation}"
        stop, lost = threading.Event(), threading.Event()

        def beat():
            while not stop.wait(5):
                try:
                    if sql(config, f'SELECT status FROM jobs.heartbeat({binding});') != 'extended':
                        lost.set()
                    save(root / 'worker-status.json', {'stage': 'running', 'measured_at': timestamp()})
                except Exception:
                    lost.set()

        def guard():
            if lost.is_set():
                raise ValueError('lease_lost')

        thread = threading.Thread(target=beat, daemon=True)
        thread.start()
        try:
            # The schedule config, not the nested cycle config, holds the
            # control-DB binding the active-publication lookup needs.
            result = cycle(config['cycle'], 'scheduled-' + task, root / task, guard,
                           lambda: last_published_generation(config))
            guard()
            done = complete(config, binding, result)
            if done not in ('succeeded', 'failed'):
                raise ValueError('completion_refused')
        except Exception as error:
            reason = ('source_catalog_export_failed' if str(error) == 'source_catalog_export_failed'
                      else 'retained_cycle_failed')
            result = {'stage': 'failed', 'reason': reason}
            recorded = root / task / 'result.json'
            if recorded.exists():
                result = json.loads(recorded.read_bytes())
            try:
                complete(config, binding, result)
            except Exception:
                pass  # Lease expiry records failure; never synthesize a success.
        finally:
            stop.set()
            thread.join(timeout=20)
        save(root / task / 'result.json', result)
        return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--worker', action='store_true')
    parser.add_argument('--once', action='store_true')
    parser.add_argument('--health', action='store_true')
    parser.add_argument('--gateway-health', action='store_true')
    parser.add_argument('--config', type=Path, default=Path('/work/operator/schedule.json'))
    args = parser.parse_args()
    os.umask(0o077)
    config = json.loads(args.config.read_text())
    if args.gateway_health:
        document = json.loads(Path(config['cycle']['document_path']).read_text())
        gateway_probe(config['cycle'], document['publication'])
        return
    path = Path(config['work_dir']) / ('worker-status.json' if args.worker else 'scheduler-status.json')
    if args.health:
        report = json.loads(path.read_text())
        if time.time() - path.stat().st_mtime > 45:
            raise SystemExit(1)
        return
    while True:
        try:
            result = work(config) if args.worker else (fire(config) or {'stage': 'dispatched'})
        except Exception:
            result = {'stage': 'failed', 'reason': 'retained_dispatch_failed'}
        path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        save(path, dict(result, measured_at=timestamp()))
        if args.once:
            print(json.dumps(result))
            raise SystemExit(1 if result['stage'] in ('failed', 'review_required') else 0)
        time.sleep(1 if args.worker else 10)


if __name__ == '__main__':
    main()
