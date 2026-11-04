import unittest
from unittest.mock import Mock, patch

import scheduler

PROJECT='00000000-0000-0000-0000-000000000001'

class Tests(unittest.TestCase):
    def test_worker_and_scheduler_share_native_project_without_claiming_success(self):
        for worker,command in [(False,'native-fire'),(True,'native-step')]:
            for code in (0,1):
                with patch.object(scheduler.subprocess,'run',return_value=Mock(returncode=code)) as run:
                    report=scheduler.dispatch(PROJECT,worker)
                args=run.call_args.args[0]
                self.assertEqual(args[3:6],[command,'--project',PROJECT])
                self.assertEqual(args[6:],['--worker','lite-worker'] if worker else [])
                self.assertEqual(report['dispatch_exit_code'],code)
                self.assertEqual(set(report),{'last_dispatch_at','dispatch_exit_code'})

    def test_bad_project_never_dispatches(self):
        with patch.object(scheduler.subprocess,'run') as run:
            with self.assertRaises(ValueError):
                scheduler.dispatch('not-a-uuid',True)
        run.assert_not_called()

if __name__=='__main__':
    unittest.main()
