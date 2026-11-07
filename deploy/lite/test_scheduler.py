import json
import copy
import hashlib
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
    def test_fresh_catalog_controls_data_only_admission(self):
        for change in ('unchanged', 'fk_removed', 'comment', 'column_comment', 'legacy'):
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
                config['builder']['origin'] = dict(facts=pin('origin.json', facts),
                                                   reviewed_registry=pin('reviewed.json', reviewed))
                current = copy.deepcopy(source)
                if change == 'fk_removed':
                    current['tables'][1]['foreign_keys'] = []
                if change == 'comment':
                    current['tables'][0]['comment'] = 'Edited source comment'
                if change == 'column_comment':
                    current['tables'][0]['columns'][0]['comment'] = 'Edited column comment'
                def export(argv, output, label):
                    self.assertEqual(argv[:3], ['/usr/local/bin/oo', 'admin', 'source-catalog'])
                    return Mock(returncode=0, stdout=json.dumps(pin('source-catalog.json', current)).encode())
                with patch.object(scheduler, 'command', side_effect=export) as command, patch.object(
                        builder.lib, 'ReadOnlyPostgres', return_value=RelatedDb(metadata)):
                    result = scheduler.data_only(config, 'cycle-1', root)
                expected = 'data_only' if change in ('unchanged', 'legacy') else 'review_required'
                self.assertEqual(result['stage'], expected)
                if expected == 'data_only':
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
            def cycle(config, generation, output, guard):
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
            config = dict(stack='stack', source='source', target='target', secrets_dir='secrets', library='owned')
            responses = [Mock(returncode=0, stdout=b'{"retained_run":"baseline"}'), Mock(returncode=1, stdout=b'{"state":"pending"}',
                stderr=b'{"error":"publication_semantic_admission_required"}')]
            with patch.object(scheduler, 'command', side_effect=responses) as command, patch.object(
                    scheduler, 'data_only', return_value={'stage': 'review_required', 'model_calls': 0}):
                self.assertEqual(scheduler.cycle(config, 'run', out)['stage'], 'review_required')
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

    def test_worker_records_review_failure_and_disables_schedule(self):
        with tempfile.TemporaryDirectory() as tmp:
            config = dict(project=PROJECT, work_dir=tmp, cycle={})
            def cycle(config, generation, output, guard):
                output.mkdir()
                return {'stage': 'review_required', 'model_calls': 0}
            responses = [json.dumps({'status': 'claimed', 'task_id': PROJECT, 'generation': 1}), '', 'failed']
            with patch.object(scheduler, 'sql', side_effect=responses) as sql, patch.object(scheduler, 'cycle', side_effect=cycle):
                result = scheduler.work(config)
            self.assertEqual(result['stage'], 'review_required')
            statements = [call.args[1] for call in sql.call_args_list]
            self.assertIn('enabled=false', statements[1])
            self.assertIn('complete_failure', statements[2])
            self.assertTrue((Path(tmp) / PROJECT / 'result.json').exists())

    def test_expired_completion_never_claims_success(self):
        with tempfile.TemporaryDirectory() as tmp:
            def cycle(config, generation, output, guard):
                output.mkdir()
                return {'stage': 'succeeded'}
            responses = [json.dumps({'status': 'claimed', 'task_id': PROJECT, 'generation': 1}), 'lost_lease', 'lost_lease']
            with patch.object(scheduler, 'sql', side_effect=responses), patch.object(scheduler, 'cycle', side_effect=cycle):
                result = scheduler.work(dict(project=PROJECT, work_dir=tmp, cycle={}))
            self.assertEqual(result['stage'], 'failed')


if __name__ == '__main__':
    unittest.main()
