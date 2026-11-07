#!/usr/bin/env python3
"""Real scheduled retained cycles on owned PG 17; initial model is explicit replay.

The initial draft uses the community runner's deterministic test transport and
real owner-review compiler. Subsequent cycles use only the production scheduler,
retained observations, publisher and authenticated Gateway. No model secret is read.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import sys
from unittest.mock import patch

os.environ['OO_TEST_LANE'] = 'm'
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tools/pg-builder/scripts'))
sys.path.insert(0, str(ROOT / 'tools/release-coordinator/tests'))
import retained_cycle_live as retained
import retained_build
import selfbuild_budget
from test_selfbuild_budget import Acceptance, Model
import scheduler
import lifecycle

PROJECT = '00000000-0000-0000-0000-000000000001'


class Scheduled(retained.Retained):
    def control_sql(self, text):
        result = retained.subprocess.run(
            ['docker', 'exec', '-i', self.db.name, 'psql', '-U', 'postgres', '-d', 'control',
             '-X', '-qAt', '-v', 'ON_ERROR_STOP=1'], input=text, text=True, capture_output=True)
        with (self.out / 'control.log').open('a') as log:
            log.write(result.stdout + result.stderr)
        assert result.returncode == 0, 'control SQL; see control.log'
        return result.stdout.strip()

    def builder(self, run, origin=None):
        config = {'protocol': retained_build.PROTOCOL, 'library_id': 'owned', 'generation': run,
                  'source_engine': 'postgres', 'project': 'owned',
                  'psql_command': ['docker', 'exec', '-i', self.db.name, 'psql', '-U', 'postgres']}
        path = self.root / ('build-' + run + '.json')
        scheduler.save(path, config)
        output = self.out / ('builder-' + run)
        model = Model()

        def draft(facts, config, output):
            tables = retained_build.inventory(facts)
            runner = selfbuild_budget.Runner(Acceptance().config(), output / 'model-replay', model)
            result = runner.run(tables, [])
            return result, len(model.requests)

        with patch.object(retained_build, 'draft', side_effect=draft):
            report = retained_build.run(path, output)
        # Parent's baseline assertion distinguishes draft generation from reuse.
        # This count means replay transport calls, never paid model calls.
        report['model_basis'] = 'deterministic_test_transport'
        scheduler.save(output / 'report.json', report)
        return report, output

    def install_jobs(self):
        self.db.sql('CREATE DATABASE control;')
        base = ROOT / 'crates/oo-jobs/migrations'
        names = ['0000_bootstrap_roles.sql', '0001_jobs_control_expand.sql',
                 '0002_event_project_binding_expand.sql', '0003_job_lease_operations_expand.sql']
        versions = ['jobs-bootstrap/1', 'jobs-control/1', 'jobs-event-project/1', 'jobs-lease-operations/1']
        sql = ['BEGIN; SELECT pg_advisory_xact_lock(6401202601);']
        for i, name in enumerate(names):
            if i == 1:
                sql.append('SET LOCAL ROLE oo_jobs_owner;')
            migration = (base / name).read_text()
            # The Agent fixture already created the cluster-wide NOLOGIN roles.
            # Install the unmodified control schema in its own database.
            sql.append('DO $$' + migration.split('DO $$', 1)[1] if i == 0 else migration)
        for version, name in zip(versions, names):
            digest = hashlib.sha256((base / name).read_bytes()).hexdigest()
            sql.append(f"INSERT INTO jobs.schema_migrations(version,checksum,phase,filename) VALUES('{version}','{digest}','expand','{name}');")
        sql.append('COMMIT;')
        self.control_sql('\n'.join(sql))
        self.control_sql(f"SET ROLE oo_jobs_runtime; INSERT INTO jobs.schedules(project_id,kind,config_version,artifact_digest,dedupe_prefix,next_run_at) VALUES('{PROJECT}','lite_retained','v1',repeat('a',64),'lite',clock_timestamp());")

    def config(self):
        origin = {name: {'path': str(path), 'sha256': hashlib.sha256(path.read_bytes()).hexdigest()}
                  for name, path in [('facts', self.origin_facts),
                                     ('reviewed_registry', self.out / 'builder-baseline/reviewed-registry.json')]}
        return {'project': PROJECT, 'interval_seconds': 3600,
                'control_psql_command': ['docker', 'exec', '-i', self.db.name, 'psql', '-U', 'postgres', '-d', 'control'],
                'work_dir': str(self.out / 'scheduled'), 'cycle': {
                    'stack': str(self.stack), 'source': 'owned', 'target': str(self.target),
                    'secrets_dir': str(self.db.directory), 'library': 'owned', 'login': retained.LOGIN,
                    'loader': str(retained.BIN / 'oo-load'), 'publisher': str(retained.BIN / 'release-coordinator'),
                    'publisher_config': str(self.root / 'publisher.json'), 'retain_seconds': 0,
                    'builder': {'protocol': retained_build.PROTOCOL, 'library_id': 'owned',
                        'source_engine': 'postgres', 'project': 'owned', 'origin': origin,
                        'psql_command': ['docker', 'exec', '-i', self.db.name, 'psql', '-U', 'postgres']},
                    'pack_path': str(self.runtime / 'packs/library.json'),
                    'document_path': str(self.runtime / 'reviewed/library/library.json'),
                    'gateway_probe': {'url': self.origin, 'ca': str(self.runtime / 'ca.crt'),
                        'cert': str(self.root / 'client.crt'), 'key': str(self.root / 'client.key'),
                        'token_file': str(self.root / 'client-token')}}}

    def cycles(self):
        self.install_jobs()
        config = self.config()
        scheduler.save(self.root / 'schedule.json', config)
        results = []
        for name, expected in [('noop', '10'), ('data-change', '11')]:
            if name == 'data-change':
                self.source_sql('UPDATE app.items SET amount=11;')
                self.control_sql('UPDATE jobs.schedules SET next_run_at=clock_timestamp();')
            scheduler.fire(config)
            result = scheduler.work(config)
            scheduler.save(self.out / (name + '.json'), result)
            assert result['stage'] == 'succeeded', result
            query = self.http()
            assert query['status'] == 200 and query['body']['rows'] == [[expected]], query
            results.append({'name': name, 'cycle': result, 'query': query})
        # Semantic change must stop scheduling and leave the published data alone.
        self.source_sql('DELETE FROM app.items;')
        self.control_sql('UPDATE jobs.schedules SET next_run_at=clock_timestamp();')
        scheduler.fire(config)
        changed = scheduler.work(config)
        assert changed['stage'] == 'review_required', changed
        assert self.control_sql('SELECT enabled FROM jobs.schedules').strip() == 'f'
        assert self.http()['body']['rows'] == [['11']]
        scheduler.save(self.out / 'scheduled-result.json', {
            'status': 'passed', 'version': self.db.sql('SHOW server_version').strip(),
            'initial_model_basis': 'deterministic_test_transport', 'cycles': results,
            'semantic_change': changed, 'seven_healthy': False,
            'scope': 'real scheduled control DB jobs; host executables, not compose acceptance'})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--run-id', required=True)
    args = parser.parse_args()
    assert args.run_id and all(c.isalnum() or c == '-' for c in args.run_id)
    os.umask(0o077)
    os.environ['OO_SELFBUILD_BUDGET_BIN'] = str(retained.BIN / 'selfbuild-budget')
    out = ROOT / 'evidence' / args.run_id
    out.mkdir(mode=0o700)
    live = Scheduled(out, 'postgres', None)
    try:
        live.start()
        live.cycles()
    finally:
        live.close()
        retained.cycles.checksums(out)


if __name__ == '__main__':
    main()
