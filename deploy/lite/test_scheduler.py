import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch

import scheduler

PROJECT = '00000000-0000-0000-0000-000000000001'


class Tests(unittest.TestCase):
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
