import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import matrix
import runner


class MatrixTests(unittest.TestCase):
    def test_all_reports_every_missing_suite_without_docker(self):
        result = subprocess.run(
            [str(runner.ROOT / "tools/oo-test/oo-test"), "matrix", "--all",
             "--suites", "T0-T4,T7,T8"], capture_output=True, text=True)
        self.assertEqual(result.returncode, 1, result.stderr)
        root = runner.ROOT / result.stdout.splitlines()[0]
        rows = json.loads((root / "evidence-index.json").read_text())
        self.assertEqual(len(rows), 14 * 10)
        self.assertFalse(any(row["result"] == "pass" for row in rows))
        self.assertEqual({row["variant"] for row in rows if row["suite"] == "T2"}, {"c1", "c5"})
        self.assertTrue(all(len(row["sha256"]) == 64 for row in rows))
        self.assertTrue(any("no locked image" in (row["error"] or "") for row in rows))
        self.assertFalse(list(root.glob("*/[0-9]*.json")))
        spec = runner.importlib.util.spec_from_file_location(
            "support_generator", runner.ROOT / "specs/scripts/gen_support_matrix.py")
        generator = runner.importlib.util.module_from_spec(spec)
        spec.loader.exec_module(generator)
        self.assertEqual(generator.validate(rows), [])
        self.assertTrue(all(cap["level"] != "verified" for target in generator.derive(rows)
                            for cap in target["caps"].values()))

    def test_fixture_t0_is_not_product_acceptance(self):
        with tempfile.TemporaryDirectory(dir=runner.ROOT / "evidence") as directory:
            root = Path(directory)
            path = root / "postgres-17"
            path.mkdir()
            (path / "SHA256SUMS").write_text("test")
            report = {"run_id": "r1", "results": [
                {"engine": "postgres", "version": "17", "suites": {"T0": "passed"}}]}
            self.assertEqual(matrix.index_rows(root, report, ["T0"])[0]["result"], "skip")

    def test_t7_requires_both_real_hooks(self):
        with tempfile.TemporaryDirectory(dir=runner.ROOT / "evidence") as directory:
            run = runner.Run(Path(directory), "postgres", "17", None)
            with patch("runner.Audit") as audit:
                audit.return_value.collect.return_value = {"passed": True}
                results = runner.execute_suites(run, ["T1", "T2", "T7"], {})
            self.assertEqual(results, {"T1": "not_implemented", "T2": "not_implemented", "T7": "failed"})
            run.cleanup()

    def test_failure_still_collects_t7(self):
        with tempfile.TemporaryDirectory(dir=runner.ROOT / "evidence") as directory:
            run = runner.Run(Path(directory), "postgres", "17", None)
            with patch("runner.Audit") as audit, patch.object(run, "command", return_value=(1, "")):
                audit.return_value.collect.return_value = {"passed": True}
                results = runner.execute_suites(run, ["T1", "T2", "T7"], {"T1": "/false", "T2": "/false"})
                audit.return_value.collect.assert_called_once()
            self.assertEqual(results, {"T1": "failed", "T2": "failed", "T7": "failed"})
            run.cleanup()


if __name__ == "__main__":
    unittest.main()
