"""Enterprise 19c resolver and fixture startup contract."""
import argparse
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch

import runner
import t1


class Oracle19Tests(unittest.TestCase):
    def setUp(self):
        (runner.ROOT / "evidence").mkdir(exist_ok=True)

    def test_pin_and_aliases(self):
        item = runner.resolve("oracle", "19")
        self.assertEqual(item["id"], "oracle-enterprise-19.19.0.0")
        for version in ("19c", "19.19.0.0"):
            self.assertEqual(runner.resolve("oracle", version), item)
        self.assertEqual(set(item["platforms"]), {"linux/arm64"})
        pin = f'{item["id"]} linux/arm64 {item["platforms"]["linux/arm64"]["digest"]}'
        self.assertIn(pin, (runner.ROOT / "testing/infra/images.pins").read_text().splitlines())
        self.assertIn("E365", item["license_note"])

    def test_pdb_routing_keeps_other_versions(self):
        for version, service in (("19", "FREEPDB1"), ("19c", "FREEPDB1"),
                                 ("21", "XEPDB1"), ("23ai", "FREEPDB1")):
            with tempfile.TemporaryDirectory(dir=runner.ROOT / "evidence") as directory:
                run = runner.Run(Path(directory), "oracle", version, argparse.Namespace())
                with patch.object(run, "command", return_value=(0, "")) as command:
                    run.sql("SELECT 1 FROM dual;")
                    self.assertIn("@localhost/" + service, command.call_args.kwargs["data"])
                    run.sql("SELECT 1 FROM dual;", admin=True)
                    self.assertIn("SET CONTAINER=" + service, command.call_args.kwargs["data"])

    def test_enterprise_startup_keeps_password_out_of_argv(self):
        calls = []

        def command(run, argv, **kwargs):
            calls.append(argv)
            if argv[:2] == ["docker", "info"]:
                return 0, "aarch64"
            if argv[:2] == ["docker", "logs"]:
                return 0, "DATABASE IS READY TO USE!"
            return 0, ""

        with tempfile.TemporaryDirectory(dir=runner.ROOT / "evidence") as directory:
            run = t1.LaneRun(Path(directory), "oracle", "19", argparse.Namespace(
                platform=None, health_timeout=1, scale=2_000_000, tables=200))
            with patch("runner.disk_gate"), patch.object(runner.Run, "command", command), \
                    patch.object(t1, "LANE", "E19"):
                run.setup()
            create = next(argv for argv in calls if argv[:2] == ["docker", "create"])
            self.assertIn("oo.lane=E19", create)
            self.assertIn("ORACLE_PDB=FREEPDB1", create)
            self.assertIn("ORACLE_SID=FREE", create)
            self.assertIn("--no-healthcheck", create)
            self.assertNotIn(run.secret, json.dumps(calls))
            self.assertIn('export ORACLE_PWD="$(cat /run/oo/password)"', create[-1])
            self.assertIn(["docker", "exec", run.name, "/opt/oracle/checkDBStatus.sh"], calls)
            self.assertEqual(run.receipt["execution"], "native")

    def test_t3t4_runs_both_paths_for_19c(self):
        from types import SimpleNamespace
        import oracle_t3t4
        import t3t4
        self.assertIn("19", t3t4.ORACLE)
        with patch.object(t3t4.Harness, "__init__", return_value=None):
            harness = oracle_t3t4.OracleHarness(SimpleNamespace(version="19"), None)
        self.assertEqual(harness.service, "FREEPDB1")
        self.assertEqual(harness.paths, ("c5", "c1"))

    def test_retention_accepts_only_closed_history_loss_refusals(self):
        from types import SimpleNamespace
        import oracle_t3t4
        import t3t4
        harness = SimpleNamespace(paths=("c5", "c1"),
                                  run=Mock(command=Mock(return_value=(0, ""))))
        for error, passed in (
            ("source error: catalog fact=logminer_resume_floor_rebaseline", True),
            ("logminer_log_gap_rebaseline", True),
            ("source error: catalog fact=logminer_no_retained_logs", False),
            ("source error: catalog fact=logminer_resume_floor_rebaseline extra", False),
            (None, False),
        ):
            with self.subTest(error=error):
                step = {"error": error, "passed": True, "outcome": "named_refusal"}
                with patch.object(t3t4, "refused_then_rebaseline", return_value=step):
                    result = oracle_t3t4.OracleHarness.retention_expiry(harness)
                self.assertEqual(result["status"], "passed" if passed else "failed")
                self.assertEqual(result["steps"][0]["error"], error)
        step = {"error": "logminer_log_gap_rebaseline", "passed": False,
                "outcome": "named_refusal"}
        with patch.object(t3t4, "refused_then_rebaseline", return_value=step):
            result = oracle_t3t4.OracleHarness.retention_expiry(harness)
        self.assertEqual(result["status"], "failed")


if __name__ == "__main__":
    unittest.main()
