#!/usr/bin/env python3
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import json
import os
import sys

spec = importlib.util.spec_from_file_location("suite", Path(__file__).with_name("run_gateway_builder_suite.py"))
suite = importlib.util.module_from_spec(spec)
spec.loader.exec_module(suite)


class SuiteTest(unittest.TestCase):
    def test_zero_ignored_and_skipped_tests_fail(self):
        for text in ("", "test result: ok. 0 passed; 0 failed; 0 ignored;",
                     "test result: ok. 1 passed; 0 failed; 1 ignored;"):
            with self.assertRaises(ValueError):
                suite.selected_tests(text, "rust")
        for text in ("Ran 0 tests in 0.1s\nOK", "Ran 1 test in 0.1s\nOK (skipped=1)"):
            with self.assertRaises(ValueError):
                suite.selected_tests(text, "python")
        self.assertEqual(suite.selected_tests("Ran 2 tests in 0.1s\nOK\n", "python"), 2)
        self.assertEqual(suite.selected_tests("test result: ok. 1 passed; 0 failed; 0 ignored;", "rust"), 1)

    def test_fixed_inventory_and_no_acceptance_claim(self):
        commands = suite.inventory("ontology-d", Path("/tmp/unused"))
        self.assertEqual(len(commands), len(suite.RUST_TARGETS) + 2)
        self.assertIn("test_library_selfbuild", commands[-1][1])
        self.assertIn("--bins", commands[0][1])
        for scope in suite.SCOPE.values():
            self.assertTrue(scope["not_covered"])

    def test_explicit_missing_pg_never_falls_back(self):
        with patch.dict(os.environ, {"OO_CI_PG_BIN": "/nonexistent-pg17"}):
            with self.assertRaises(ValueError):
                suite.pg_bin()

    def test_failure_receipt(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(suite.shutil, "which", return_value=None):
            output = Path(directory) / "receipt.json"
            self.assertEqual(suite.main(["--suite", "ontology-d", "--output", str(output)]), 1)
            report = json.loads(output.read_text())
            self.assertFalse(report["acceptance_closed"])
            self.assertEqual(report["status"], "failed")
            self.assertEqual(report["protocol"], "oo.ci.capability-suite/v1")
            self.assertEqual(report["tests_executed"], 0)
            self.assertTrue(report["cleanup_complete"])

    def test_redaction(self):
        self.assertEqual(suite.redact("secret postgresql://user:pass@host/db", ["secret"]),
                         "[REDACTED] postgresql://[REDACTED]")

    def test_success_receipt_counts_tests_not_commands(self):
        def fake_run(command, cwd, env, log, timeout):
            self.assertIn("OO_LIBRARY_BUILDER_BIN", env)
            log.write_text("Ran 7 tests in 0.1s\nOK\n")
            return 0, False

        with tempfile.TemporaryDirectory() as directory, \
                patch.object(suite, "run_command", side_effect=fake_run), \
                patch.object(suite.shutil, "which", return_value="/bin/cargo"), \
                patch.object(suite, "inventory", return_value=[
                    ("fixture", ["fixed-test-command"], "python", suite.ROOT)]):
            output = Path(directory) / "receipt.json"
            self.assertEqual(suite.main(["--suite", "ontology-d", "--output", str(output)]), 0)
            report = json.loads(output.read_text())
            self.assertEqual(report["tests_executed"], 7)
            self.assertEqual(report["commands"][0]["argv"], ["fixed-test-command"])
            self.assertEqual(report["commands"][0]["exit_code"], 0)
            self.assertTrue(report["cleanup_complete"])
            self.assertFalse(report["acceptance_closed"])

    def test_cleanup_refuses_other_cluster(self):
        with tempfile.TemporaryDirectory() as directory:
            scratch = Path(directory)
            data = scratch / "f22-owned-test/pg"
            data.mkdir(parents=True)
            (data / "postmaster.pid").write_text("123\n/unrelated/customer/pg\n")
            with patch.object(suite.subprocess, "run") as run:
                with self.assertRaises(ValueError):
                    suite.cleanup_owned(scratch, Path("/pg17/bin"))
                run.assert_not_called()

    def test_timeout_is_nonzero_and_recorded(self):
        with tempfile.TemporaryDirectory() as directory:
            code, expired = suite.run_command(
                [sys.executable, "-c", "import time; time.sleep(30)"], suite.ROOT,
                dict(os.environ), Path(directory) / "timeout.log", 0.1)
            self.assertTrue(expired)
            self.assertNotEqual(code, 0)


if __name__ == "__main__":
    unittest.main()
