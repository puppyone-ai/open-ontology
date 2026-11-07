import argparse
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import live_selfbuild_acceptance as live


class OwnedFixtureTests(unittest.TestCase):
    def test_runner_relabels_only_its_created_resources(self):
        run = object.__new__(live.LaneRun)
        with patch.object(live.fixture.Run, 'command', return_value=(0, '')) as command:
            run.command(['docker','create','--label','oo.lane=h','--label','oo.run_id=test'])
        self.assertEqual(command.call_args.args[0],
                         ['docker','create','--label','oo.lane=g','--label','oo.run_id=test'])

    def test_cleanup_refuses_another_lanes_resource(self):
        with tempfile.TemporaryDirectory() as directory:
            run = live.LaneRun(Path(directory),'postgres','17',argparse.Namespace())
            run.created_container = True
            calls = []
            def command(argv, **_):
                calls.append(argv)
                return 0,json.dumps({'oo.lane':'h','oo.run_id':run.id})
            run.command = command
            self.assertEqual(run.cleanup(),[run.name])
            self.assertFalse(any('rm' in argv for argv in calls))

    def test_generator_retains_full_table_parameter_and_wide_columns(self):
        sql = ''.join(live.fixture.generator.generate('postgres','17',1000,660))
        self.assertEqual(sql.count('CREATE TABLE small_'),660)
        self.assertIn('c299 BIGINT',sql)


if __name__ == '__main__':
    unittest.main()
