#!/usr/bin/env python3
"""T9 status journal integration on owned PG 17; not full lite acceptance.

The --control-only probe validates real leases, atomic completion, event payloads,
stale continuity and publication-age health. Full T9-08/09 requires lane P's
loader and a real-model, isolated-HOME lite deployment; do not substitute this
probe for that acceptance.
"""
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import re
import secrets
import subprocess
import sys
import time
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'deploy/lite'))
import lifecycle
import scheduler

PROJECT = '00000000-0000-0000-0000-000000000001'
IMAGE = 'docker.io/library/postgres@sha256:839fbb5a4942a7758dd93e18b42ea5649807491a0545b5eeeb8f0b8bb11857a5'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--run-id', required=True)
    parser.add_argument('--control-only', action='store_true')
    args = parser.parse_args()
    if not re.fullmatch(r'[a-z0-9-]+', args.run_id):
        parser.error('invalid run ID')
    if not args.control_only:
        parser.error('full lite acceptance requires the lane P loader; use --control-only for the narrower probe')
    os.umask(0o077)
    out = ROOT / 'evidence' / args.run_id
    out.mkdir(mode=0o700)
    private = ROOT / 'target' / (args.run_id + '-private')
    private.mkdir(mode=0o700)
    password = secrets.token_hex(32).encode()
    (private / 'password').write_bytes(password)
    name = 'oo-fh-q-' + args.run_id
    created = False
    summary = {'scope': 'control DB only; not full T9-08/T9-09 lite acceptance',
               'T9-08': 'not_run', 'T9-09': 'partial', 'status': 'failed',
               'secret_scan_scope': 'generated fixture password; no model credentials accessed'}

    def command(argv, data=None):
        result = subprocess.run(argv, input=data, capture_output=True, timeout=90)
        with (out / 'commands.log').open('ab') as log:
            log.write(result.stdout + result.stderr)
        if result.returncode:
            raise RuntimeError('command_failed')
        return result.stdout.decode().strip()

    def sql(text):
        return command(['docker', 'exec', '-i', name, 'psql', '-X', '-qAt',
                        '-v', 'ON_ERROR_STOP=1', '-U', 'postgres'], text.encode())

    def complete(result, stamp, stale=None, pending=None, reason=None):
        lease = json.loads(sql(f"""SET ROLE oo_jobs_runtime;
SELECT * FROM jobs.enqueue('{PROJECT}','lite_retained',1,'{{}}','v1',repeat('a',64),
 '{secrets.token_hex(8)}',clock_timestamp(),1);
SELECT row_to_json(t) FROM jobs.claim('{PROJECT}',ARRAY['lite_retained'],'t9') t;
""").splitlines()[-1])
        assert lease['status'] == 'claimed'
        body = dict(result=result, reason=reason, stale_tables=stale or [],
                    review_pending=pending or [], data_published_at=stamp)
        binding = f"'{PROJECT}','{lease['task_id']}','t9',{lease['generation']}"
        payload = json.dumps(body).replace("'", "''")
        before = sql('SELECT count(*) FROM jobs.lite_cycle_events')
        wrong = f"'{PROJECT}','{lease['task_id']}','other-worker',{lease['generation']}"
        assert sql(f"SET ROLE oo_jobs_runtime; SELECT jobs.complete_lite_cycle({wrong},'{payload}');") == 'lost_lease'
        assert sql('SELECT count(*) FROM jobs.lite_cycle_events') == before
        done = sql(f"SET ROLE oo_jobs_runtime; SELECT jobs.complete_lite_cycle({binding},'{payload}');")
        assert done == ('succeeded' if result in ('succeeded', 'partial') else 'failed'), done
        # A repeated/expired lease cannot append a fabricated publication event.
        count = sql('SELECT count(*) FROM jobs.lite_cycle_events')
        assert sql(f"SET ROLE oo_jobs_runtime; SELECT jobs.complete_lite_cycle({binding},'{payload}');") == 'terminal'
        assert sql('SELECT count(*) FROM jobs.lite_cycle_events') == count
        event = json.loads(sql('SELECT payload FROM jobs.lite_cycle_events ORDER BY event_id DESC LIMIT 1'))
        assert event['result'] == result
        return event

    try:
        command(['docker', 'run', '-d', '--name', name, '--label', 'oo.lane=fh-q',
                 '--platform', 'linux/arm64', '--memory', '512m',
                 '-e', 'POSTGRES_PASSWORD_FILE=/run/private/password',
                 '-v', str(private) + ':/run/private:ro', '--tmpfs', '/var/lib/postgresql/data',
                 IMAGE])
        created = True
        for _ in range(90):
            ready = subprocess.run(['docker', 'exec', name, 'pg_isready', '-U', 'postgres'],
                                   capture_output=True)
            if ready.returncode == 0:
                break
            time.sleep(1)
        else:
            raise RuntimeError('postgres_start_timeout')
        summary['version'] = sql('SHOW server_version')
        assert summary['version'].startswith('17.')
        migrations = ROOT / 'crates/oo-jobs/migrations'
        files = ['0000_bootstrap_roles.sql', '0001_jobs_control_expand.sql',
                 '0002_event_project_binding_expand.sql', '0003_job_lease_operations_expand.sql']
        import hashlib
        versions = ['jobs-bootstrap/1', 'jobs-control/1', 'jobs-event-project/1', 'jobs-lease-operations/1']
        install = ['BEGIN;']
        for index, filename in enumerate(files):
            if index == 1:
                install.append('SET LOCAL ROLE oo_jobs_owner;')
            install.append((migrations / filename).read_text())
        for version, filename in zip(versions, files):
            digest = hashlib.sha256((migrations / filename).read_bytes()).hexdigest()
            install.append("INSERT INTO jobs.schema_migrations(version,checksum,phase,filename)"
                           f" VALUES('{version}','{digest}','expand','{filename}');")
        sql('\n'.join(install + ['COMMIT;']))
        extension = (ROOT / 'deploy/lite/cycle_events.sql').read_text()
        sql(extension)
        sql(extension)  # Repeated up is an additive, idempotent migration.
        stamp = scheduler.timestamp()
        complete('succeeded', stamp)
        stale = [{'table': 'app.items', 'reason': 'source_timeout', 'since': stamp}]
        pending = [{'table': 'app.accounts', 'changes': ['comment']}]
        partial = complete('partial', stamp, stale, pending, 'stale_tables')
        stale[0]['since'] = '2099-01-01T00:00:00Z'
        repeated = complete('partial', stamp, stale, pending, 'stale_tables')
        assert repeated['stale_tables'][0]['since'] == partial['stale_tables'][0]['since']
        failed = complete('failed', None, reason='sync_failed')
        assert failed['stale_tables'] == partial['stale_tables']
        assert failed['review_pending'] == pending
        assert failed['data_published_at'] is None
        publication = sql("SELECT payload->>'data_published_at' FROM jobs.lite_cycle_events "
                          "WHERE payload->>'data_published_at' IS NOT NULL ORDER BY event_id DESC LIMIT 1")
        assert publication == stamp
        history = {'data_published_at': publication, 'interval_seconds': 30,
                   'cycle': {'result': 'failed'}, 'tables': {'stale': [], 'review_pending': []}}
        assert lifecycle.publication_health(history, True)['status'] == 'healthy'
        history['tables']['review_pending'] = pending
        assert lifecycle.publication_health(history, True)['status'] == 'degraded'
        assert lifecycle.publication_health(history, True,
            datetime(2099, 1, 1, tzinfo=timezone.utc))['alert'] == 'data_publication_overdue'
        recovered = complete('succeeded', stamp)
        assert recovered['stale_tables'] == recovered['review_pending'] == []
        complete('review_required', None, pending=pending, reason='semantic_review_required')
        assert sql("SELECT has_table_privilege('oo_jobs_runtime','jobs.lite_cycle_events','INSERT')") == 'f'
        operator = private / 'work/operator'
        operator.mkdir(parents=True, mode=0o700)
        scheduler.save(operator / 'schedule.json', {
            'project': PROJECT, 'interval_seconds': 30, 'cycle': {'library': 'owned'}})
        def owned_docker(argv, env):
            # Only redirect the compose address to this owned container. The
            # production read-only status SQL executes unchanged on PostgreSQL.
            assert argv[:3] == ['exec', '-T', 'control-db']
            return command(['docker', 'exec', '-i', name, *argv[3:]]).encode()
        with patch.object(lifecycle, 'docker', side_effect=owned_docker):
            status = lifecycle.control_status(private, [], {})
        assert status['schedule_status'] == 'measured', status
        assert status['data_published_at'] == stamp
        assert status['cycle']['result'] == 'review_required'
        assert status['cycle']['at'] and status['tables']['review_pending'] == pending
        (out / 'status.json').write_text(json.dumps(status, indent=2) + '\n')
        (operator / 'schedule.json').unlink()
        operator.rmdir()
        operator.parent.rmdir()
        summary.update(status='passed', cycles=7, replay_refusals=7, wrong_owner_refusals=7, migration_runs=2)
    finally:
        if created:
            command(['docker', 'rm', '-f', '-v', name])
        (private / 'password').unlink()
        # No model/token files are ever created by this control-only probe.
        import shutil
        shutil.rmtree(private)
        summary['containers_left'] = int(bool(subprocess.check_output(
            ['docker', 'ps', '-aq', '--filter', 'name=^/' + name + '$']).strip()))
        summary['exact_byte_secret_matches'] = sum(
            password in path.read_bytes() for path in out.rglob('*') if path.is_file())
        (out / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
    assert summary['containers_left'] == summary['exact_byte_secret_matches'] == 0
    print(json.dumps(summary))


if __name__ == '__main__':
    main()
