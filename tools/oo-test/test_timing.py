import contextlib
import io
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import runner
import timing
import t3t4


class TimingTests(unittest.TestCase):
    def setUp(self):
        root = runner.ROOT / "evidence"
        root.mkdir(exist_ok=True)
        self.temp = tempfile.TemporaryDirectory(dir=root)
        self.addCleanup(self.temp.cleanup)
        self.run = SimpleNamespace(path=Path(self.temp.name), receipt={"run_id": "test"})
        self.now = 0.0
        self.run.timing = timing.Timing(self.run, clock=lambda: self.now)

    def test_nested_stages_are_exclusive_and_absent_is_null(self):
        with self.run.timing.stage("product_run"):
            self.now = 2
            with self.run.timing.stage("compare"):
                self.now = 5
            self.now = 7
        stages = self.run.timing.snapshot()["stages"]
        self.assertEqual(stages["product_run"]["seconds"], 4)
        self.assertEqual(stages["compare"]["seconds"], 3)
        self.assertIsNone(stages["seed"]["seconds"])

    def test_exception_and_interrupt_are_retained_without_error_text(self):
        for exception in (RuntimeError("private material"), KeyboardInterrupt()):
            with self.assertRaises(type(exception)):
                with self.run.timing.stage("seed"):
                    self.now += 2
                    raise exception
        raw = (self.run.path / "timing.json").read_text()
        self.assertNotIn("private material", raw)
        self.assertEqual(json.loads(raw)["stages"]["seed"]["raised"], 2)

    def test_receipt_fields_preserved(self):
        receipt = self.run.path / "resource-receipt.json"
        receipt.write_text('{"run_id":"test","cleanup_failed":["owned"]}')
        with self.run.timing.stage("cleanup"):
            self.now = 1
        data = json.loads(receipt.read_text())
        self.assertEqual(data["cleanup_failed"], ["owned"])
        self.assertEqual(data["timing"], self.run.receipt["timing"])

    def test_subclass_override_is_measured_without_super_call(self):
        class CustomRun(runner.Run):
            def seed(self):
                raise RuntimeError("fixture failure")
        run = CustomRun(self.run.path, "postgres", "17", SimpleNamespace())
        with self.assertRaises(RuntimeError):
            run.seed()
        self.assertEqual(run.timing.snapshot()["stages"]["seed"]["raised"], 1)

    def test_same_stage_super_call_does_not_double_count(self):
        with self.run.timing.stage("seed"):
            self.now = 1
            with self.run.timing.stage("seed"):
                self.now = 4
            self.now = 5
        self.assertEqual(self.run.timing.snapshot()["stages"]["seed"]["seconds"], 5)

    def test_decorator_accepts_harness_and_test_double(self):
        @timing.timed("compare")
        def compare(harness):
            return 42
        self.assertEqual(compare(SimpleNamespace(run=self.run)), 42)
        self.assertEqual(compare(SimpleNamespace()), 42)
        self.assertEqual(self.run.timing.snapshot()["stages"]["compare"]["calls"], 1)

    def test_product_commands_only(self):
        @timing.product_command
        def command(run, argv):
            return 1, "not a verdict"
        self.assertEqual(command(self.run, ["target/debug/oo-load"]), (1, "not a verdict"))
        command(self.run, ["docker", "inspect"])
        self.assertEqual(self.run.timing.snapshot()["stages"]["product_run"]["calls"], 1)

    def test_summary_missing_is_failure_not_zero(self):
        (self.run.path / "resource-receipt.json").write_text("{}")
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            self.assertEqual(timing.main([str(self.run.path)]), 1)
        self.assertIn("unmeasured", output.getvalue())

    def test_summary_reports_one_copy_per_context(self):
        (self.run.path / "resource-receipt.json").write_text("{}")
        with self.run.timing.stage("seed"):
            self.now = 4
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            self.assertEqual(timing.main([str(self.run.path), "--json"]), 0)
        rows = json.loads(output.getvalue())["receipts"]
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows[0]["timing"]["stages"]["seed"]["seconds"], 4)

    def test_summary_empty_and_external_paths_fail(self):
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(timing.main([str(self.run.path)]), 1)
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
            timing.main([str(runner.ROOT)])

    def test_cleanup_failure_still_writes_timing_before_checksums(self):
        run = runner.Run(self.run.path, "postgres", "17", SimpleNamespace())
        with patch.object(run, "command", side_effect=RuntimeError("failed")):
            run.created_container = True
            with self.assertRaises(RuntimeError):
                run.cleanup()
        runner.checksums(run.path)
        self.assertIn("timing.json", (run.path / "SHA256SUMS").read_text())
        self.assertEqual(run.timing.snapshot()["stages"]["cleanup"]["raised"], 1)

    def test_background_lifetime_is_explicitly_overlapping(self):
        self.now = 3
        with self.run.timing.stage("compare"):
            self.now = 5
        self.now = 9
        self.run.timing.background("product_run", 1)
        value = self.run.timing.snapshot()
        self.assertEqual(value["stages"]["product_run"]["seconds"], 8)
        self.assertTrue(value["events"][-1]["background"])
        self.assertEqual(value["stages"]["compare"]["seconds"], 2)

    def test_background_finish_records_exception_once(self):
        harness = object.__new__(t3t4.Harness)
        harness.run = self.run
        proc = SimpleNamespace(started=0)
        self.now = 4
        with patch.object(harness, "_finish", side_effect=RuntimeError("diagnostic")):
            for _ in range(2):
                with self.assertRaises(RuntimeError):
                    harness.finish(proc)
        stage = self.run.timing.snapshot()["stages"]["product_run"]
        self.assertEqual(stage["calls"], 1)
        self.assertEqual(stage["raised"], 1)


if __name__ == "__main__":
    unittest.main()
