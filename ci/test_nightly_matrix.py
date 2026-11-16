import datetime
import unittest
from unittest import mock

import nightly_matrix
from nightly_matrix import summarize


class StabilityTests(unittest.TestCase):
    def test_three_identical_failures_are_not_stable(self):
        row = {"target": "mysql8-selfmanaged", "version": "8.4", "suite": "T7",
               "variant": None, "result": "fail"}
        summary = summarize([{"exit": 1, "rows": [row]}] * 3)
        self.assertTrue(summary["repeatable_outcomes"])
        self.assertFalse(summary["stable"])
        self.assertFalse(summary["consecutive_nights"])

    def test_empty_or_skipped_run_cannot_pass(self):
        self.assertFalse(summarize([])["stable"])
        self.assertFalse(summarize([{"exit": 0, "rows": []}])["stable"])
        row = {"target": "pg", "version": "17", "suite": "T0", "result": "skip"}
        self.assertFalse(summarize([{"exit": 0, "rows": [row]}])["stable"])

    @mock.patch.object(nightly_matrix, "image_present", return_value=False)
    def test_only_checked_host_conditions_skip_and_skips_are_not_rows(self, present):
        day = datetime.date(2026, 11, 17)  # Oracle 19c is that day's L2 representative.
        cells, l2, skipped = nightly_matrix.plan(day, "linux/amd64", {})
        skipped_keys = {(s["layer"], s["engine"], s["version"]) for s in skipped}
        self.assertEqual(skipped_keys, {("L1", "oracle", "11"), ("L1", "oracle", "19"),
                                        ("L2", "oracle", "19")})
        self.assertTrue(all(s["result"] == "skip" and s["reasons"] for s in skipped))
        nineteen = next(s for s in skipped if s["version"] == "19")
        self.assertEqual(len(nineteen["reasons"]), 2)  # arm64-only and registry sign-in
        executed = {(c["engine"], c["version"]) for c in cells}
        self.assertIn(("oracle", "21"), executed)
        self.assertIn(("oracle", "23"), executed)
        self.assertEqual(len(executed), 12)  # 6 PostgreSQL, 4 MySQL, 2 Oracle
        self.assertEqual({e["engine"] for e in l2}, {"postgres", "mysql"})
        # The worker image makes 11g runnable; nothing else can be skipped by choice.
        cells, _, skipped = nightly_matrix.plan(day, "linux/amd64",
                                                {"OO_TEST_ORACLE_WORKER_IMAGE": "img@sha256:" + "0" * 64})
        self.assertIn(("oracle", "11"), {(c["engine"], c["version"]) for c in cells})
        _, _, skipped = nightly_matrix.plan(day, "linux/arm64", {})
        self.assertNotIn(("L1", "oracle", "21"), {(s["layer"], s["engine"], s["version"]) for s in skipped})
        # A host that already holds the pinned 19c image (arm64) runs it.
        present.return_value = True
        _, _, skipped = nightly_matrix.plan(day, "linux/arm64", {})
        self.assertNotIn(("L1", "oracle", "19"), {(s["layer"], s["engine"], s["version"]) for s in skipped})

    def test_failed_pending_or_missing_cells_are_failures(self):
        report = {"layers": {"L1": {"results": [
            {"engine": "postgres", "version": "13", "status": "passed",
             "suites": {"T0": "passed", "T1": "passed", "T2": "passed"}},
            {"engine": "oracle", "version": "23", "status": "pending", "suites": {},
             "reason": "oracle_slot_occupied"},
            {"engine": "mysql", "version": "9", "status": "failed",
             "suites": {"T0": "passed", "error": "boom"}},
        ]}}}
        rows = nightly_matrix.rows_from(report, "L1", "r", "0" * 64)
        results = {(r["target"], r["suite"]): r["result"] for r in rows}
        self.assertEqual(results[("postgres", "T2")], "pass")
        self.assertEqual(results[("oracle", "cell")], "fail")
        self.assertEqual(results[("mysql", "cell")], "fail")
        self.assertFalse(summarize([{"exit": 1, "rows": rows}])["stable"])
        self.assertEqual(nightly_matrix.rows_from(None, "L2", "r", None), [])

    def test_only_declared_case_skips_are_skips(self):
        import json, tempfile
        from pathlib import Path
        root = nightly_matrix.ROOT / "evidence"
        root.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=root) as directory:
            evidence = Path(directory)
            def report(cases):
                (evidence / "matrix.json").write_text(json.dumps({"23": {"cases": cases}}))
                rel = str(evidence.relative_to(nightly_matrix.ROOT))
                return {"layers": {"L2": {"results": [
                    {"engine": "oracle", "version": "23", "status": "failed", "evidence": rel}]}}}
            rmans = {"status": "not_run",
                     "reason": "pinned slim image has no RMAN; physical archive expiry not injected"}
            skipped = []
            rows = nightly_matrix.rows_from(report({
                "sigkill_mid_cycle": {"status": "passed"}, "retention_expiry": rmans,
                "slot_invalidation": {"status": "not_applicable", "reason": "PostgreSQL replication slots only"},
            }), "L2", "r", "0" * 64, skipped)
            self.assertTrue(all(r["result"] == "pass" for r in rows))
            self.assertEqual([(s["case"], s["result"]) for s in skipped], [("retention_expiry", "skip")])
            # Another reason, another case or a failure is never a skip.
            for cases in ({"retention_expiry": dict(rmans, reason="no time")},
                          {"network_drop_mid_c1": rmans},
                          {"sigkill_mid_cycle": {"status": "failed"}}, {}):
                skipped = []
                rows = nightly_matrix.rows_from(report(cases), "L2", "r", "0" * 64, skipped)
                self.assertEqual(skipped, [])
                self.assertTrue(any(r["result"] == "fail" for r in rows), cases)
        missing = {"layers": {"L2": {"results": [{"engine": "mysql", "version": "9", "status": "failed"}]}}}
        self.assertEqual(nightly_matrix.rows_from(missing, "L2", "r", None)[0]["error"], "no_matrix")


if __name__ == "__main__":
    unittest.main()
