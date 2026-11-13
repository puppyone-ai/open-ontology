import datetime
import json
from pathlib import Path
import tempfile
import unittest

import l3
import t3t4
import runner


class L3Tests(unittest.TestCase):
    def test_three_day_rotation_covers_every_engine_with_full_commands(self):
        day = datetime.date(2026, 11, 12)
        plans = [l3.slice_for(day + datetime.timedelta(days=i)) for i in range(3)]
        self.assertEqual({p["engine"] for p in plans}, {"postgres", "mysql", "oracle"})
        for plan in plans:
            matrix = plan["commands"][0]
            self.assertNotIn("--scale", matrix)
            self.assertNotIn("--cases", plan["commands"][2])

    def test_pairs_require_identical_verdicts(self):
        with tempfile.TemporaryDirectory(dir=runner.ROOT / "evidence") as directory:
            fast, real = Path(directory) / "fast.json", Path(directory) / "real.json"
            cases = {c: {"status": "passed"} for c in t3t4.FAST_PAIRS}
            fast.write_text(json.dumps({"cases": cases}))
            real.write_text(json.dumps({"cases": cases}))
            self.assertEqual(l3.pairs(fast, real)["status"], "passed")
            cases = dict(cases, sigkill_mid_cycle={"status": "failed"})
            real.write_text(json.dumps({"cases": cases}))
            self.assertEqual(l3.pairs(fast, real)["status"], "failed")

    def test_full_verdicts_latest_rerun_wins_and_versions_normalize(self):
        with tempfile.TemporaryDirectory(dir=runner.ROOT / "evidence") as directory:
            root = Path(directory)
            (root / "a").mkdir()
            (root / "b").mkdir()
            row = {"target": "mysql57-selfmanaged", "version": "5.7.44", "suite": "T3"}
            (root / "a/index-rows.json").write_text(json.dumps([dict(row, result="fail",
                                                    observed_at="2026-11-12T05:00:00Z")]))
            (root / "b/index-rows.json").write_text(json.dumps([dict(row, result="pass",
                                                    observed_at="2026-11-12T14:00:00Z")]))
            self.assertEqual(l3.full_verdicts([root]), {("mysql", "5.7", "T3"): "pass"})

    def test_diff_reports_only_comparable_cells(self):
        with tempfile.TemporaryDirectory(dir=runner.ROOT / "evidence") as directory:
            root = Path(directory)
            (root / "full").mkdir()
            (root / "full/index-rows.json").write_text(json.dumps([
                {"target": "postgres-selfmanaged", "version": "17.11", "suite": "T1",
                 "result": "pass", "observed_at": "x"},
                {"target": "postgres-selfmanaged", "version": "17.11", "suite": "T7",
                 "result": "fail", "observed_at": "x"}]))
            report = {"layers": {"L1": {"results": [
                {"engine": "postgres", "version": "17",
                 "suites": {"T1": "passed", "T7": "passed", "T0": "passed"}}]}}}
            (root / "fast.json").write_text(json.dumps(report))
            result = l3.diff(root / "fast.json", [root / "full"])
            self.assertEqual(result["comparable"], 2)
            self.assertEqual([r["suite"] for r in result["disagree"]], ["T7"])


if __name__ == "__main__":
    unittest.main()
