import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import unittest
from unittest.mock import patch
import uuid

from whole import poll_native_state, running_worker


class PollLifecycle(unittest.TestCase):
    def test_only_connection_slot_exhaustion_retries_and_retains_raw_error(self):
        with tempfile.TemporaryDirectory() as out:
            failed = subprocess.CompletedProcess(['state'], 1, b'', b'job-worker: sql sqlstate=53300 remaining connection slots are reserved')
            good = subprocess.CompletedProcess(['state'], 0, b'{"source_generation":4}', b'')
            with patch('whole.subprocess.run', side_effect=[failed, good]) as run, patch('whole.time.sleep') as sleep:
                self.assertEqual(poll_native_state(['state'], out), {'source_generation': 4})
                self.assertEqual(run.call_count, 2)
                sleep.assert_called_once_with(1)
            record = json.loads(next(Path(out).glob('*.json')).read_bytes())
            self.assertEqual(record['exit'], 1)
            self.assertEqual(record['argv'], ['state'])
            self.assertEqual(next(Path(out).glob('*.stderr')).read_bytes(), failed.stderr)

    def test_deterministic_failure_stops_immediately(self):
        with tempfile.TemporaryDirectory() as out:
            failed = subprocess.CompletedProcess(['state'], 1, b'', b'job-worker: binding')
            with patch('whole.subprocess.run', return_value=failed) as run, patch('whole.time.sleep') as sleep:
                with self.assertRaisesRegex(RuntimeError, 'native_state_failed'):
                    poll_native_state(['state'], out)
                self.assertEqual(run.call_count, 1)
                sleep.assert_not_called()

    def test_slot_retry_budget_is_bounded(self):
        with tempfile.TemporaryDirectory() as out:
            failed = subprocess.CompletedProcess(['state'], 1, b'', b'job-worker: sql sqlstate=53300')
            with patch('whole.subprocess.run', return_value=failed) as run, patch('whole.time.sleep'):
                with self.assertRaisesRegex(RuntimeError, 'native_state_failed'):
                    poll_native_state(['state'], out)
                self.assertEqual(run.call_count, 3)
                self.assertEqual(len(list(Path(out).glob('*.stderr'))), 3)

    @unittest.skipUnless(os.environ.get('OO_MYSQL_LIFECYCLE_CONTAINER'), 'explicit owned fixture container required')
    def test_real_container_child_is_not_left_alive_after_loop_failure(self):
        container = os.environ['OO_MYSQL_LIFECYCLE_CONTAINER']
        argv = ['python3', '-c', 'import time; time.sleep(30)', 'mysql-lifecycle-'+uuid.uuid4().hex]
        with tempfile.TemporaryDirectory() as out:
            started = time.monotonic()
            with self.assertRaisesRegex(RuntimeError, 'synthetic state failure'):
                with running_worker(container, argv, out) as proc:
                    time.sleep(.4)
                    raise RuntimeError('synthetic state failure')
            self.assertIsNotNone(proc.poll())
            self.assertLess(time.monotonic() - started, 12)
            stopped = json.loads((Path(out)/'worker-stop-15.stdout').read_bytes())
            self.assertEqual(len(stopped['owned_pids']), 1)
            pid = str(stopped['owned_pids'][0])
            result = subprocess.run(['docker', 'exec', container, 'test', '-d', '/proc/'+pid])
            self.assertNotEqual(result.returncode, 0)


if __name__ == '__main__':
    unittest.main()
