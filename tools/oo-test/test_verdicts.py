import hashlib
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import runner
import matrix
import t3t4
from verdicts import incremental, merge, write_rows


class VerdictTests(unittest.TestCase):
    def test_hook_matrix_uses_owned_loopback_source(self):
        import t1
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            hook = root / "hook"
            hook.write_text("#!/bin/sh\nexit 1\n")
            hook.chmod(0o700)
            with patch.object(runner, "ROOT", root), patch.object(t1, "LANE", "m"), \
                 patch.dict(runner.os.environ, {"OO_TEST_LANE": "h"}), \
                 patch("sys.argv", ["oo-test", "matrix", "--engine", "postgres",
                                   "--versions", "17", "--suites", "T1",
                                   "--hook", "T1=" + str(hook)]), \
                 patch.object(runner, "disk_gate"), \
                 patch.object(runner, "resolve", return_value={"id": "postgres-17.11"}), \
                 patch.object(t1.LaneRun, "setup", side_effect=RuntimeError("test stop")) as setup, \
                 patch.object(t1.LaneRun, "cleanup", return_value=[]):
                self.assertEqual(runner.main(), 1)
                setup.assert_called_once()
                self.assertEqual(t1.LANE, "h")

    def test_merge_only_preserves_verdict_without_starting_containers(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "evidence" / "source"
            source.mkdir(parents=True)
            raw = source / "result.json"
            raw.write_text("{}")
            run = SimpleNamespace(engine="postgres", version="17", id="r",
                                  receipt={"version_actual": "17.11"})
            expected = write_rows(run, raw, [("T1", "snapshot", "pass")])[0]
            with patch.object(runner, "ROOT", root), \
                 patch("sys.argv", ["oo-test", "matrix", "--merge-only",
                                   "--merge-evidence", str(source)]), \
                 patch.object(runner.Run, "setup") as setup:
                self.assertEqual(runner.main(), 0)
                setup.assert_not_called()
            outputs = list(root.glob("evidence/h-merged-*/evidence-index.json"))
            self.assertEqual(len(outputs), 1)
            actual = json.loads(outputs[0].read_text())
            expected["evidence"] = "evidence/source/result.json"
            self.assertEqual(actual, [expected])

    def test_rows_bind_raw_results_and_round_trip(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)
            raw = path / "result.json"
            raw.write_text('{"T1":"passed"}\n')
            run = SimpleNamespace(engine="postgres", version="17", id="test-run",
                                  receipt={"version_actual": "17.11", "execution": "native"})
            rows = write_rows(run, raw, [("T1", "snapshot", "pass")])
            self.assertEqual(rows[0]["sha256"], hashlib.sha256(raw.read_bytes()).hexdigest())
            self.assertTrue(rows[0]["observed_at"].endswith("Z"))
            self.assertEqual(merge(path), rows)
            raw.write_text('{"T1":"failed"}\n')
            with self.assertRaisesRegex(ValueError, "digest mismatch"):
                merge(path)

    def test_unmeasured_version_or_failed_cleanup_cannot_pass(self):
        with tempfile.TemporaryDirectory() as directory:
            raw = Path(directory) / "result.json"
            raw.write_text("{}")
            run = SimpleNamespace(engine="mysql", version="5.7", id="test-run")
            self.assertEqual(write_rows(run, raw, [("T1", "snapshot", "pass")])[0]["result"], "fail")
            run.receipt = {"version_actual": "5.7.44", "execution": "emulated"}
            rows = write_rows(run, raw, [("T1", "snapshot", "pass")], cleanup_failed=True)
            self.assertEqual(rows[0]["result"], "fail")
            self.assertTrue(rows[0]["emulated"])

    def test_matrix_unmeasured_success_is_downgraded(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / "mysql-5.7"
            path.mkdir()
            (path / "comparison-report.json").write_text("{}")
            report = {"run_id": "test", "results": [
                {"engine": "mysql", "version": "5.7", "suites": {"T1": "passed"}}]}
            self.assertEqual(matrix.index_rows(root, report, ["T1"])[0]["result"], "fail")

    def test_merge_rejects_escaping_raw_path(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            child = root / "child"
            child.mkdir()
            raw = root / "outside.json"
            raw.write_text("{}")
            row = {"target": "postgres-selfmanaged", "version": "17.11", "suite": "T1",
                   "run_id": "test", "sha256": "0" * 64,
                   "result": "pass", "evidence": "../outside.json"}
            (child / "index-rows.json").write_text(json.dumps([row]))
            with self.assertRaisesRegex(ValueError, "outside run"):
                merge(child)

    def test_incremental_requires_completed_checks_and_three_meter_results(self):
        report = {"T2": "passed", "C1": "passed", "C5": "passed", "cycles": [
            {"mode": mode, "T8": {"status": "passed"}} for mode in ("C1", "C5") for _ in range(3)]}
        self.assertTrue(all(r == "pass" for _, _, r in incremental(report)))
        report["cycles"].pop()
        self.assertIn(("T8", "c5", "fail"), incremental(report))
        report["T2"] = "not_passed"
        self.assertTrue(all(r == "fail" for _, _, r in incremental(report)))

    def test_partial_fault_selection_does_not_pass_entire_suite(self):
        report = {"cases": {"source_restart_mid_chunk": {"status": "passed"}}}
        self.assertTrue(all(r == "fail" for _, _, r in t3t4.suite_verdicts(report, "mysql", "8.4")))
        report["cases"] = {c: {"status": "passed"} for c in t3t4.ORDER}
        self.assertTrue(all(r == "pass" for _, _, r in t3t4.suite_verdicts(report, "mysql", "8.4")))
        self.assertIn(("T3", "c1", "skip"), t3t4.suite_verdicts(report, "oracle", "11"))
        report["target_cleanup_error"] = "failed"
        self.assertTrue(all(r == "fail" for _, _, r in t3t4.suite_verdicts(report, "mysql", "8.4")))

    def test_zero_hook_exit_without_verdict_does_not_pass(self):
        with tempfile.TemporaryDirectory(dir=runner.ROOT / "evidence") as directory:
            run = runner.Run(Path(directory), "postgres", "17", None)
            with patch.object(run, "command", return_value=(0, "")):
                self.assertEqual(runner.execute_suites(run, ["T1"], {"T1": "/true"}), {"T1": "failed"})
            run.cleanup()

    def test_naive_timestamp_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            raw = Path(directory) / "result.json"
            raw.write_text("{}")
            run = SimpleNamespace(engine="postgres", version="17", id="r",
                                  receipt={"version_actual": "17.11"})
            rows = write_rows(run, raw, [("T1", "snapshot", "pass")])
            rows[0]["observed_at"] = "2026-11-05T00:00:00"
            (raw.parent / "index-rows.json").write_text(json.dumps(rows))
            with self.assertRaisesRegex(ValueError, "must be UTC"):
                merge(raw.parent)


if __name__ == "__main__":
    unittest.main()
