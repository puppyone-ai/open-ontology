import json
import copy
import hashlib
import os
import sys
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch

import scheduler

PROJECT = '00000000-0000-0000-0000-000000000001'
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'tools/pg-builder/scripts'))
import retained_build as builder
from test_retained_catalog import fixture, RelatedDb


class Tests(unittest.TestCase):
    def test_service_liveness_is_not_last_cycle_success(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            config = root / 'schedule.json'
            scheduler.save(config, {'work_dir': str(root)})
            status = root / 'worker-status.json'
            scheduler.save(status, {'stage': 'failed'})
            with patch.object(sys, 'argv', ['scheduler', '--worker', '--health', '--config', str(config)]):
                scheduler.main()
                os.utime(status, (1, 1))
                with self.assertRaises(SystemExit) as error:
                    scheduler.main()
                self.assertEqual(error.exception.code, 1)

    def test_loader_exit_contract_and_post_activation_failures(self):
        for code, failure, outcome in [(0, None, 'activated'), (3, None, 'activated'),
                                      (0, None, 'replay'), (4, 'all_tables_stale', None),
                                      (1, 'sync_failed', None), (2, 'sync_failed', None),
                                      (0, 'publish_failed', None), (0, 'prune_failed', 'activated'),
                                      (0, 'gateway_reload_failed', 'activated')]:
            with self.subTest(code=code, failure=failure), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                config = dict(stack='s', source='s', target='t', secrets_dir='p', library='owned',
                              login='owner', publisher_config='p',
                              builder={'origin': {'reviewed_registry': {}}},
                              pack_path=str(root / 'pack'), document_path=str(root / 'document'))
                commands = []
                def fake(argv, output, label, data=None):
                    commands.append(label)
                    if label == 'sync':
                        rows = [{'status': 'noop' if outcome == 'replay' else 'fresh',
                                 'retained_run': 'run', 'table': 'app.a'}]
                        if outcome != 'replay':
                            rows.append({'status': 'noop', 'retained_run': 'older', 'table': 'app.c'})
                        if code in (3, 4):
                            rows.append({'status': 'stale', 'table': {'schema': 'app', 'name': 'b'},
                                         'stale_reason': 'source_timeout', 'stale_detail': 'timeout'})
                        return Mock(returncode=code, stdout=b'\n'.join(json.dumps(r).encode() for r in rows))
                    if label == 'candidate':
                        return Mock(returncode=1, stdout=b'{"state":"pending"}',
                                    stderr=b'{"error":"publication_semantic_admission_required"}')
                    if label == 'publisher':
                        return Mock(returncode=int(failure == 'publish_failed'), stdout=json.dumps({
                            'state': 'ready', 'outcome': outcome, 'receipt_sha256': 'receipt',
                            'gateway_document': {'envelope': {'producer': {'artifact': {}}}}
                        }).encode())
                    return Mock(returncode=int(failure == 'prune_failed'))
                with patch.object(scheduler, 'command', side_effect=fake), patch.object(
                        scheduler, 'data_only', return_value={'stage': 'data_only', 'review_pending': [
                            {'table': 'app.a', 'changes': ['comment']}]}), patch.object(
                        builder, 'pinned', return_value=b'{}'), patch.object(
                        scheduler, 'gateway_probe', side_effect=ValueError('pending') if
                        failure == 'gateway_reload_failed' else None,
                        return_value={'receipt_sha256': 'receipt'}), patch.object(scheduler.time, 'sleep'):
                    out = root / 'cycle'
                    if failure:
                        with self.assertRaisesRegex(ValueError, failure):
                            scheduler.cycle(config, 'run', out)
                        result = json.loads((out / 'result.json').read_text())
                        self.assertEqual(result['result'], 'failed')
                    else:
                        result = scheduler.cycle(config, 'run', out)
                        self.assertEqual(result['result'], 'partial' if code == 3 else 'succeeded')
                        self.assertEqual(len(result['review_pending']), 1)
                    activated = outcome == 'activated'
                    self.assertEqual(result['data_published_at'] is not None, activated)
                    if code in (1, 2, 4):
                        self.assertEqual(commands, ['sync'])
                    if code in (3, 4):
                        self.assertEqual(result['stale_tables'][0]['table'], 'app.b')

    def test_all_noop_cycle_reconfirms_the_active_publication(self):
        for active in ('run_active', None):
            with self.subTest(active=active), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                config = dict(stack='s', source='s', target='t', secrets_dir='p', library='owned',
                              login='owner', publisher_config='p',
                              builder={'origin': {'reviewed_registry': {}}},
                              pack_path=str(root / 'pack'), document_path=str(root / 'document'))
                generations = []
                def fake(argv, output, label, data=None):
                    if label == 'sync':
                        rows = [{'status': 'noop', 'retained_run': 'run_a', 'table': 'app.a'},
                                {'status': 'noop', 'retained_run': 'run_b', 'table': 'app.b'}]
                        return Mock(returncode=0, stdout=b'\n'.join(json.dumps(r).encode() for r in rows))
                    if label == 'candidate':
                        generations.append(argv[argv.index('--generation') + 1])
                        return Mock(returncode=1, stdout=b'{"state":"pending"}',
                                    stderr=b'{"error":"publication_semantic_admission_required"}')
                    if label == 'publisher':
                        return Mock(returncode=0, stdout=json.dumps({
                            'state': 'ready', 'outcome': 'replay', 'receipt_sha256': 'receipt',
                            'gateway_document': {'envelope': {'producer': {'artifact': {}}}}}).encode())
                    return Mock(returncode=0)
                with patch.object(scheduler, 'command', side_effect=fake), patch.object(
                        scheduler, 'data_only', return_value={'stage': 'data_only', 'review_pending': []}), \
                        patch.object(builder, 'pinned', return_value=b'{}'), patch.object(
                        scheduler, 'gateway_probe', return_value={'receipt_sha256': 'receipt',
                                                                  'measured_at': '2026-11-08T00:00:00Z'}):
                    out = root / 'cycle'
                    if active is None:
                        with self.assertRaisesRegex(ValueError, 'mixed_retained_runs'):
                            scheduler.cycle(config, 'run_new', out, active_generation=lambda: active)
                        self.assertEqual(generations, [])
                    else:
                        result = scheduler.cycle(config, 'run_new', out, active_generation=lambda: active)
                        self.assertEqual(generations, ['run_active'])
                        self.assertEqual(result['result'], 'succeeded')
                        self.assertIsNone(result['data_published_at'])
                        self.assertEqual(result['data_verified_at'], '2026-11-08T00:00:00Z')
                        self.assertEqual(result['published_generation'], 'run_active')

    def test_active_generation_lookup_uses_the_schedule_config(self):
        # The worker passes the top-level schedule config (project and control
        # psql), which the nested cycle config does not carry.
        top = {'project': '11111111-2222-4333-8444-555555555555', 'control_psql_command': ['psql'],
               'cycle': {'library': 'owned'}}
        seen = []
        def fake_sql(config, text):
            seen.append((config, text))
            return 'run_active\n'
        with patch.object(scheduler, 'sql', side_effect=fake_sql):
            self.assertEqual(scheduler.last_published_generation(top), 'run_active')
        self.assertIs(seen[0][0], top)
        self.assertIn(top['project'], seen[0][1])
        with patch.object(scheduler, 'sql', return_value="bad'value"):
            self.assertIsNone(scheduler.last_published_generation(top))
        with self.assertRaises(KeyError):
            scheduler.last_published_generation(top['cycle'])

    def test_fresh_catalog_controls_data_only_admission(self):
        for change in ('unchanged', 'fk_removed', 'comment', 'column_comment', 'new_column', 'legacy',
                       'inferred_edge', 'report'):
            with self.subTest(change=change), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                metadata, source = fixture()
                config = dict(library='owned', stack='stack', source='owned', secrets_dir='secrets',
                              source_catalog='export',
                              builder=dict(library_id='owned', source_engine='postgres',
                                           generation='cycle-1', psql_command=['unused']))
                def pin(name, value):
                    path = root / name
                    scheduler.save(path, value)
                    return dict(path=str(path), sha256=hashlib.sha256(path.read_bytes()).hexdigest())
                initial = dict(config['builder'])
                if change != 'legacy':
                    initial['source_catalog'] = pin('original.json', source)
                else:
                    del config['source_catalog']
                facts, _ = builder.facts_for(RelatedDb(metadata), initial)
                reviewed = dict(protocol='oo.owner-reviewed-registry/v1', status='reviewed',
                                review_decision={'mode': 'self'},
                                compiled={'effective': {'datasets': [
                                    dict(dataset=t['dataset'], columns=t['columns']) for t in facts['tables']]}})
                origin = copy.deepcopy(facts)
                if change == 'report':
                    origin['report_definitions_observation'] = {'dataset': 'app.accounts', 'definitions': []}
                config['builder']['origin'] = dict(facts=pin('origin.json', origin),
                                                   reviewed_registry=pin('reviewed.json', reviewed))
                current = copy.deepcopy(source)
                if change == 'fk_removed':
                    current['tables'][1]['foreign_keys'] = []
                if change == 'comment':
                    current['tables'][0]['comment'] = 'Edited source comment'
                if change == 'column_comment':
                    current['tables'][1]['columns'][1]['comment'] = 'Edited column comment'
                if change == 'inferred_edge':
                    # The key comment drives inferred cross-table edges.
                    current['tables'][0]['columns'][0]['comment'] = 'Edited column comment'
                if change == 'new_column':
                    current['tables'][0]['columns'].append(dict(name='unreviewed', not_null=False, comment='new'))
                def export(argv, output, label):
                    self.assertEqual(argv[:3], ['/usr/local/bin/oo', 'admin', 'source-catalog'])
                    return Mock(returncode=0, stdout=json.dumps(pin('source-catalog.json', current)).encode())
                with patch.object(scheduler, 'command', side_effect=export) as command, patch.object(
                        builder.lib, 'ReadOnlyPostgres', return_value=RelatedDb(metadata)):
                    result = scheduler.data_only(config, 'cycle-1', root)
                library = change in ('inferred_edge', 'report')
                self.assertEqual(result['stage'], 'review_required' if library else 'data_only')
                self.assertEqual(result['library_data_only_digest'] ==
                                 builder.semantics.library_data_only_digest(origin), not library)
                expected = [] if change in ('unchanged', 'legacy', 'report') else [
                    'app.invoices' if change in ('fk_removed', 'column_comment') else 'app.accounts']
                if library:
                    head = result['review_pending'].pop(0)
                    self.assertEqual((head['table'], head['scope']), ('library:owned', 'library'))
                    self.assertEqual(head['changes'], ['relationship_candidates' if change == 'inferred_edge'
                                                       else 'reports'])
                self.assertEqual([r['table'] for r in result['review_pending']], expected)
                if not expected:
                    self.assertEqual(result['data_only_digest'], builder.semantics.data_only_digest(facts))
                self.assertEqual(command.call_count, int(change != 'legacy'))
                self.assertNotIn('source_catalog', config['builder'])

    def test_catalog_export_failure_never_observes_or_falls_back(self):
        for response in (Mock(returncode=1, stdout=b'{}'), Mock(returncode=0, stdout=b'{}')):
            with tempfile.TemporaryDirectory() as tmp, patch.object(
                    builder, 'pinned', return_value=b'{}'), patch.object(
                    builder.lib, 'ReadOnlyPostgres'), patch.object(builder, 'facts_for') as facts, patch.object(
                    scheduler, 'command', return_value=response):
                config = dict(source_catalog='export', library='owned', stack='s', source='s', secrets_dir='s',
                              builder=dict(library_id='owned', psql_command=[], origin=dict(facts={}, reviewed_registry={})))
                with self.assertRaisesRegex(ValueError, '^source_catalog_export_failed$'):
                    scheduler.data_only(config, 'cycle', Path(tmp))
                facts.assert_not_called()

    def test_worker_preserves_named_export_failure(self):
        with tempfile.TemporaryDirectory() as tmp:
            def cycle(config, generation, output, guard, active_generation=lambda: None):
                output.mkdir()
                raise ValueError('source_catalog_export_failed')
            responses = [json.dumps({'status': 'claimed', 'task_id': PROJECT, 'generation': 1}), 'failed']
            with patch.object(scheduler, 'sql', side_effect=responses), patch.object(
                    scheduler, 'cycle', side_effect=cycle):
                result = scheduler.work(dict(project=PROJECT, work_dir=tmp, cycle={}))
            self.assertEqual(result, {'stage': 'failed', 'reason': 'source_catalog_export_failed'})

    def test_fire_uses_control_occurrence_and_not_native_worker(self):
        with patch.object(scheduler, 'sql') as sql:
            scheduler.fire({'project': PROJECT, 'interval_seconds': 30})
        text = sql.call_args.args[1]
        for expected in ('FOR UPDATE SKIP LOCKED', 'schedule_occurrences', 'jobs.enqueue', "state IN ('queued','running')"):
            self.assertIn(expected, text)
        self.assertNotIn('native', text)

    def test_bad_schedule_never_dispatches(self):
        for config in ({'project': 'invalid', 'interval_seconds': 30},
                       {'project': PROJECT, 'interval_seconds': True}):
            with patch.object(scheduler, 'sql') as sql:
                with self.assertRaises(ValueError):
                    scheduler.fire(config)
                sql.assert_not_called()

    def test_control_errors_are_redacted(self):
        with patch.object(scheduler.subprocess, 'run', return_value=Mock(returncode=1, stderr='private')):
            with self.assertRaisesRegex(ValueError, '^control_database_failed$'):
                scheduler.sql({'control_psql_command': ['psql']}, 'SELECT 1')

    def test_review_required_stops_before_publisher_and_prune(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / 'cycle'
            library = {'table': 'library:owned', 'scope': 'library', 'changes': ['reports']}
            config = dict(stack='stack', source='source', target='target', secrets_dir='secrets', library='owned')
            responses = [Mock(returncode=0, stdout=b'{"retained_run":"baseline"}'), Mock(returncode=1, stdout=b'{"state":"pending"}',
                stderr=b'{"error":"publication_semantic_admission_required"}')]
            with patch.object(scheduler, 'command', side_effect=responses) as command, patch.object(
                    scheduler, 'data_only', return_value={'stage': 'review_required', 'model_calls': 0,
                                                          'review_pending': [library]}):
                result = scheduler.cycle(config, 'run', out)
                self.assertEqual(result['stage'], 'review_required')
                self.assertEqual(result['review_pending'], [library])
                self.assertEqual([c.args[0][1] for c in command.call_args_list], ['sync', 'publish'])

    def test_unexpected_candidate_failure_cannot_publish(self):
        with tempfile.TemporaryDirectory() as tmp:
            config = dict(stack='s', source='s', target='t', secrets_dir='p', library='owned')
            responses = [Mock(returncode=0, stdout=b'{"retained_run":"baseline"}'), Mock(returncode=1, stdout=b'{"state":"pending"}',
                stderr=b'{"error":"other_error"}')]
            with patch.object(scheduler, 'command', side_effect=responses), patch.object(scheduler, 'data_only') as build:
                with self.assertRaisesRegex(ValueError, 'candidate_failed'):
                    scheduler.cycle(config, 'run', Path(tmp) / 'cycle')
                build.assert_not_called()

    def test_lease_loss_prevents_any_subprocess(self):
        with tempfile.TemporaryDirectory() as tmp:
            config = dict(stack='s', source='s', target='t', secrets_dir='p', library='owned')
            with patch.object(scheduler, 'command') as command:
                with self.assertRaisesRegex(ValueError, 'lost'):
                    scheduler.cycle(config, 'run', Path(tmp) / 'cycle', Mock(side_effect=ValueError('lost')))
                command.assert_not_called()

    def test_worker_records_review_failure_without_disabling_schedule(self):
        with tempfile.TemporaryDirectory() as tmp:
            config = dict(project=PROJECT, work_dir=tmp, cycle={})
            def cycle(config, generation, output, guard, active_generation=lambda: None):
                output.mkdir()
                return {'stage': 'review_required', 'model_calls': 0}
            responses = [json.dumps({'status': 'claimed', 'task_id': PROJECT, 'generation': 1}), 'failed']
            with patch.object(scheduler, 'sql', side_effect=responses) as sql, patch.object(scheduler, 'cycle', side_effect=cycle):
                result = scheduler.work(config)
            self.assertEqual(result['stage'], 'review_required')
            statements = [call.args[1] for call in sql.call_args_list]
            self.assertNotIn('enabled=false', ' '.join(statements))
            self.assertIn('complete_lite_cycle', statements[1])
            self.assertIn('review_required', statements[1])
            self.assertTrue((Path(tmp) / PROJECT / 'result.json').exists())

    def test_expired_completion_never_claims_success(self):
        with tempfile.TemporaryDirectory() as tmp:
            def cycle(config, generation, output, guard, active_generation=lambda: None):
                output.mkdir()
                return {'stage': 'succeeded'}
            responses = [json.dumps({'status': 'claimed', 'task_id': PROJECT, 'generation': 1}), 'lost_lease', 'lost_lease']
            with patch.object(scheduler, 'sql', side_effect=responses), patch.object(scheduler, 'cycle', side_effect=cycle):
                result = scheduler.work(dict(project=PROJECT, work_dir=tmp, cycle={}))
            self.assertEqual(result['stage'], 'failed')


if __name__ == '__main__':
    unittest.main()
