"""Regression coverage for the Oracle hook in the shared T1/T8 runner."""
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / "tools/oo-test"))

import t1
from test_oracle_t1 import Source, Target


class OracleHookTests(unittest.TestCase):
    def test_shared_entry_compares_complete_lobs_and_empty_locators(self):
        source = Source()
        for corrupt in (False, True):
            result = t1.compare_oracle(
                source, Target(source, corrupt), {"target": "fixture"}, "TYPE_EDGES")
            self.assertEqual(result["equal"], not corrupt)

    def test_source_resources_use_explicit_lane_e(self):
        run = t1.LaneRun.__new__(t1.LaneRun)
        run.name, run.engine = "owned-source", "oracle"
        with patch.object(t1, "LANE", "e"):
            with patch.object(t1.runner.Run, "command", return_value=(0, "")) as command:
                run.command(["docker", "create", "--name", run.name,
                             "--label", "oo.lane=h", "image"])
                self.assertIn("oo.lane=e", command.call_args.args[0])
                self.assertIn("127.0.0.1::1521", command.call_args.args[0])


if __name__ == "__main__":
    unittest.main()
