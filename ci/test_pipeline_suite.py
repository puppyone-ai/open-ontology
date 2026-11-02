#!/usr/bin/env python3
"""Offline runner checks; the real capability suites are run separately."""

import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location(
    "pipeline_suite", Path(__file__).with_name("run_pipeline_suite.py"))
suite = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(suite)


class PipelineSuiteTests(unittest.TestCase):
    def test_help_is_offline(self):
        with patch.object(suite, "discover_pg", side_effect=AssertionError("runtime accessed")):
            with contextlib.redirect_stdout(io.StringIO()), self.assertRaises(SystemExit) as error:
                suite.main(["--help"])
        self.assertEqual(error.exception.code, 0)

    def test_unknown_suite_rejected(self):
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as error:
            suite.main(["--suite", "typo", "--output", "/unused"])
        self.assertEqual(error.exception.code, 2)

    def test_dispatch_is_exact_and_scoped(self):
        self.assertEqual(set(suite.SUITES), {"integration-a", "nightly-a-m"})
        self.assertTrue({spec[0] for spec in suite.SUITES["integration-a"]} <=
                        {spec[0] for spec in suite.SUITES["nightly-a-m"]})
        for specs in suite.SUITES.values():
            self.assertTrue(specs)
            for target, exact, prefix, coverage in specs:
                command = suite.cargo_command(target, exact)
                self.assertIn("--locked", command)
                self.assertEqual(command[command.index("--test") + 1], target)
                self.assertEqual(command[command.index("--exact") + 1], exact)
                self.assertNotIn("--workspace", command)
                self.assertTrue(prefix.startswith("OO_C5"))
                self.assertTrue(coverage)
                source = (suite.ROOT / "crates/oo-pipeline/tests" / f"{target}.rs").read_text()
                self.assertIn(f"async fn {exact}()", source)
                self.assertIn(f'"{prefix}_PG_PORT"', source)

    def test_no_zero_ignored_or_wrong_test_pass(self):
        suite.verify_test_output(
            "test real_test ... ok\n"
            "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured;", "real_test")
        for text in (
            "test result: ok. 0 passed; 0 failed; 0 ignored;",
            "test real_test ... ignored\ntest result: ok. 0 passed; 0 failed; 1 ignored;",
            "test another ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored;",
            "test real_test ... ok\ntest result: ok. 2 passed; 0 failed; 0 ignored;",
        ):
            with self.assertRaises(RuntimeError):
                suite.verify_test_output(text, "real_test")

    def test_credentials_and_inherited_fixture_ports_removed(self):
        with patch.dict(os.environ, {"PGHOST": "customer", "PGPASSWORD": "secret",
                                    "DATABASE_URL": "secret", "OO_C5_PG_PORT": "5432",
                                    "OO_C5_INITIALIZER_OWNED": "1", "RUST_LOG": "trace",
                                    "CARGO_TARGET_DIR": "/tmp/shared-cargo-target"}):
            env = suite.isolated_env()
        self.assertEqual(env["CARGO_TARGET_DIR"], "/tmp/shared-cargo-target")
        for key in ("PGHOST", "PGPASSWORD", "DATABASE_URL", "OO_C5_PG_PORT",
                    "OO_C5_INITIALIZER_OWNED", "RUST_LOG"):
            self.assertNotIn(key, env)

    def test_missing_runtime_is_failure_receipt(self):
        with tempfile.TemporaryDirectory() as tmp:
            output = Path(tmp) / "receipt.json"
            with patch.object(suite, "source_info", return_value={"head": "test"}), \
                 patch.object(suite, "discover_pg", side_effect=RuntimeError("secret DSN")), \
                 contextlib.redirect_stdout(io.StringIO()) as stdout:
                code = suite.main(["--suite", "integration-a", "--output", str(output)])
            receipt = json.loads(output.read_text())
            self.assertEqual(code, 1)
            self.assertEqual(receipt["status"], "failed")
            self.assertEqual(receipt["results"], [])
            self.assertEqual(receipt["protocol"], "oo.ci.capability-suite/v1")
            self.assertFalse(receipt["acceptance_closed"])
            self.assertEqual(receipt["tests_executed"], 0)
            self.assertTrue(receipt["cleanup_complete"])
            self.assertNotIn("secret", output.read_text() + stdout.getvalue())

    def test_missing_pg_override_does_not_fall_back(self):
        with patch.dict(os.environ, {"OO_CI_PG_BIN": "/nonexistent-ci-pg17"}):
            with self.assertRaises(RuntimeError):
                suite.discover_pg(suite.isolated_env())

    def test_timeout_recorded(self):
        with tempfile.TemporaryDirectory() as tmp:
            receipt = {"commands": []}
            runner = suite.Runner(receipt, Path(tmp))
            with self.assertRaises(RuntimeError):
                runner.run([sys.executable, "-c", "import time; time.sleep(60)"], timeout=0.1)
            self.assertTrue(receipt["commands"][0]["timeout"])
            self.assertEqual(receipt["commands"][0]["argv"][0], sys.executable)
            self.assertNotEqual(receipt["commands"][0]["exit_code"], 0)

    def test_empty_results_cannot_pass(self):
        with tempfile.TemporaryDirectory() as tmp:
            output = Path(tmp) / "receipt.json"
            with patch.object(suite, "source_info", return_value={}), \
                 patch.object(suite, "discover_pg", return_value=(Path("/fake"), "17.0")), \
                 patch.object(suite.Runner, "run"), patch.object(suite.Runner, "scenario"), \
                 contextlib.redirect_stdout(io.StringIO()):
                code = suite.main(["--suite", "integration-a", "--output", str(output)])
            self.assertEqual(code, 1)
            self.assertEqual(json.loads(output.read_text())["status"], "failed")

    def test_common_receipt_contract(self):
        with tempfile.TemporaryDirectory() as tmp:
            output = Path(tmp) / "receipt.json"

            def scenario(runner, pg, spec):
                runner.receipt["results"].append({
                    "test": spec[1], "tests_passed": 1, "tests_executed": 1,
                    "status": "passed", "cleanup": True})

            with patch.object(suite, "source_info", return_value={"head": "test"}), \
                 patch.object(suite, "discover_pg", return_value=(Path("/fake"), "17.0")), \
                 patch.object(suite.Runner, "run"), \
                 patch.object(suite.Runner, "scenario", scenario), \
                 contextlib.redirect_stdout(io.StringIO()):
                code = suite.main(["--suite", "nightly-a-m", "--output", str(output)])
            receipt = json.loads(output.read_text())
            self.assertEqual(code, 0)
            self.assertEqual(receipt["protocol"], "oo.ci.capability-suite/v1")
            self.assertEqual(receipt["tests_executed"], 5)
            self.assertTrue(receipt["cleanup_complete"])
            self.assertFalse(receipt["acceptance_closed"])
            self.assertEqual(receipt["status"], "passed")

    def test_failure_still_stops_only_owned_cluster(self):
        with tempfile.TemporaryDirectory() as tmp:
            receipt = {"commands": [], "results": []}
            runner = suite.Runner(receipt, Path(tmp))
            calls = []

            def fake_run(command, **kwargs):
                command = list(map(str, command))
                calls.append(command)
                if command[0].endswith("/initdb"):
                    data = Path(command[command.index("-D") + 1])
                    data.mkdir()
                elif command[-1] == "start":
                    data = Path(command[command.index("-D") + 1])
                    (data / "postmaster.pid").write_text("fake")
                elif command[0] == "cargo":
                    raise RuntimeError("test failed")

            with patch.object(runner, "run", side_effect=fake_run):
                with self.assertRaises(RuntimeError):
                    runner.scenario(Path("/fake"), suite.LIFECYCLE)
            stop = calls[-1]
            self.assertEqual(stop[-1], "stop")
            self.assertTrue(stop[stop.index("-D") + 1].startswith("/tmp/oo-ci-pg-"))
            self.assertTrue(receipt["results"][0]["cleanup"])
            self.assertFalse(Path(receipt["results"][0]["cluster"]).exists())
            self.assertEqual(receipt["results"][0]["status"], "failed")


if __name__ == "__main__":
    unittest.main()
