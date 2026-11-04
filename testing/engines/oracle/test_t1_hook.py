"""Regression coverage for the Oracle hook in the shared T1/T8 runner."""
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / "tools/oo-test"))

import t1
from test_oracle_t1 import Source, Target


class OracleHookTests(unittest.TestCase):
    def test_oracle_readonly_retry_recognizes_sqlplus_header(self):
        with tempfile.TemporaryDirectory(dir=t1.ROOT / "evidence") as temporary:
            run = t1.LaneRun.__new__(t1.LaneRun)
            run.path, run.seq = Path(temporary), 1
            refusal = "Cannot connect to the Docker daemon at fixture. Is the docker daemon running?"
            header = ("whenever sqlerror exit failure rollback\n"
                      "whenever oserror exit failure rollback\n"
                      "set colsep '|'\nconnect / as sysdba\n"
                      "ALTER SESSION SET CONTAINER=XEPDB1;\n"
                      "ALTER SESSION SET CURRENT_SCHEMA=fixture;\n")
            for query, error, output, retry in [
                ("SELECT count(*) FROM v$session;", refusal, "", True),
                ("SET ARRAYSIZE 1000\nSET LINESIZE 1024\nSELECT * FROM FIXTURE.EVENTS;",
                 refusal, "", True),
                ("SELECT 1 FROM dual;", refusal, "partial result", False),
                ("SELECT 1 FROM dual;", "ORA-01031: insufficient privileges", "", False),
                ("INSERT INTO growth_events VALUES(1,1);", refusal, "", False),
                ("BEGIN NULL; END;\n/", refusal, "", False),
            ]:
                (run.path / "0001.stderr").write_text(error)
                (run.path / "0001.stdout").write_text(output)
                with patch.object(t1.runner.Run, "command",
                                  side_effect=[RuntimeError("failed"), (0, "1")]) as call:
                    with patch.object(t1.time, "sleep"):
                        if retry:
                            self.assertEqual(run.command(
                                ["docker", "exec", "owned", "sqlplus"],
                                data=header + query + "\nexit\n"), (0, "1"))
                        else:
                            with self.assertRaises(RuntimeError):
                                run.command(["docker", "exec", "owned", "sqlplus"],
                                            data=header + query + "\nexit\n")
                self.assertEqual(call.call_count, 2 if retry else 1)

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
