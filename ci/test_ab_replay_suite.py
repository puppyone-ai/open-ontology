#!/usr/bin/env python3
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("suite", Path(__file__).with_name("run_ab_replay_suite.py"))
suite = importlib.util.module_from_spec(spec)
spec.loader.exec_module(suite)


class AbReplaySuiteTest(unittest.TestCase):
    def test_zero_failed_ignored_and_skipped_tests_fail(self):
        for text in ("", "test result: ok. 0 passed; 0 failed; 0 ignored;",
                     "test result: ok. 2 passed; 0 failed; 1 ignored;"):
            with self.assertRaises(ValueError):
                suite.selected_tests(text, "rust")
        for text in ("Ran 0 tests in 0.1s\nOK", "Ran 3 tests in 0.1s\nOK (skipped=1)",
                     "Ran 3 tests in 0.1s\nFAILED (failures=1)"):
            with self.assertRaises(ValueError):
                suite.selected_tests(text, "python")
        # Empty lib/bin/doc targets beside the integration tests are not a selection.
        rust = ("test result: ok. 0 passed; 0 failed; 0 ignored;\n"
                "test result: ok. 2 passed; 0 failed; 0 ignored;\n")
        self.assertEqual(suite.selected_tests(rust, "rust"), 2)
        self.assertEqual(suite.selected_tests("Ran 39 tests in 4s\n\nOK\n", "python"), 39)

    def test_fixed_inventory_replays_recorded_demo_without_model_inputs(self):
        output = suite.ROOT / "evidence/ab-replay-gate-test"
        commands = suite.inventory(output)
        self.assertEqual([c[0] for c in commands],
                         ["build-" + t for t in suite.TOOLS] + ["oo-eval", "ab-python", "replay"])
        replay = commands[-1][1]
        self.assertEqual(replay[replay.index("--dataset") + 1], "demo")
        self.assertFalse(any("model" in arg or "profile" in arg for arg in replay))
        self.assertTrue(all("--locked" in c[1] for c in commands if c[1][0] == "cargo"))
        self.assertEqual(suite.SCOPE["acceptance"], [])
        self.assertTrue(any("REQ-EVAL-04" in item for item in suite.SCOPE["not_covered"]))

    def test_replay_must_be_byte_identical_and_recorded(self):
        with tempfile.TemporaryDirectory(dir=suite.ROOT / "ci") as directory:
            root = Path(directory)
            report = {"adapter": "recorded-responses", "model_measured": False, "tracks": {}}
            produced, expected = root / "report.json", root / "expected.json"
            produced.write_text(json.dumps(report) + "\n")
            expected.write_text(json.dumps(report) + "\n")
            self.assertTrue(suite.compare(produced, expected)["byte_identical"])
            expected.write_text(json.dumps(report, indent=1) + "\n")  # same JSON, other bytes
            section = suite.compare(produced, expected)
            self.assertFalse(section["byte_identical"])
            self.assertEqual(section["produced_report"], produced.read_text())
            expected.unlink()
            self.assertFalse(suite.compare(produced, expected)["byte_identical"])
            produced.write_text(json.dumps(report | {"model_measured": True}))
            with self.assertRaisesRegex(ValueError, "recorded-response"):
                suite.compare(produced, expected)

    def test_failure_receipt_claims_nothing(self):
        with tempfile.TemporaryDirectory() as directory, \
             patch.object(suite.shutil, "which", return_value=None):
            output = Path(directory) / "receipt.json"
            self.assertEqual(suite.main(["--suite", "ab-replay", "--output", str(output)]), 1)
            receipt = json.loads(output.read_text())
            self.assertEqual(receipt["protocol"], "oo.ci.capability-suite/v1")
            self.assertEqual(receipt["status"], "failed")
            self.assertFalse(receipt["acceptance_closed"])
            self.assertEqual(receipt["tests_executed"], 0)
            self.assertEqual(receipt["model_calls"], 0)
            self.assertTrue(receipt["cleanup_complete"])


if __name__ == "__main__":
    unittest.main()
