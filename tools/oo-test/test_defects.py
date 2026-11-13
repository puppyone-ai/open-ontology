import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import defects
from defect_checks import ReplayChecks  # Include the detectors in ordinary discovery.


class DefectGateTests(unittest.TestCase):
    def test_mutations_cannot_escape_owned_scratch(self):
        with tempfile.TemporaryDirectory(dir=defects.runner.ROOT / "evidence") as directory:
            root = Path(directory)
            scratch = root / "scratch"
            scratch.mkdir()
            (scratch / "external").symlink_to(root)
            for path in ("../outside", "/outside", ".git/config", "external/outside"):
                with self.assertRaises(ValueError):
                    defects.scratch_path(scratch, path)

    def test_timeout_cleanup_refuses_other_lane_or_name(self):
        with tempfile.TemporaryDirectory(dir=defects.runner.ROOT / "evidence") as directory:
            root = Path(directory)
            live = root / "baseline-live"
            live.mkdir()
            (live / "gate-owner.json").write_text(json.dumps({"name": "ours", "run_id": "ours"}))
            info = [{"Name": "/other", "Config": {"Labels": {"oo.lane": "ta", "oo.run_id": "ours"}}}]
            with patch.object(defects.subprocess, "check_output",
                              side_effect=["container\n", json.dumps(info)]), \
                 patch.object(defects.subprocess, "run") as remove:
                with self.assertRaisesRegex(RuntimeError, "ownership"):
                    defects.cleanup_live(root)
                remove.assert_not_called()

    def test_only_completed_assertion_failures_are_caught(self):
        self.assertEqual(defects.classify(
            1, "Ran 2 tests in 0.01s\n\nFAILED (failures=1)\n", "python"), "caught")
        for output in ("Ran 0 tests in 0.01s\n\nOK\n",
                       "Ran 1 test in 0.01s\n\nFAILED (errors=1)\n",
                       "Ran 2 tests in 0.01s\n\nFAILED (failures=1, errors=1)\n",
                       "ImportError", "command timeout"):
            self.assertEqual(defects.classify(1, output, "python"), "error")
        good = "Ran 1 test in 0.01s\n\nOK\n"
        self.assertEqual(defects.classify(0, good, "python"), "missed")
        self.assertEqual(defects.classify(0, good, "python", True), "passed")

    def test_rust_compile_errors_and_empty_filters_do_not_count(self):
        good = "test result: ok. 1 passed; 0 failed; 0 ignored;"
        failed = "test result: FAILED. 0 passed; 1 failed; 0 ignored;"
        self.assertEqual(defects.classify(101, failed, "rust"), "caught")
        self.assertEqual(defects.classify(101, failed, "rust", True), "baseline_failed")
        self.assertEqual(defects.classify(0, good, "rust"), "missed")
        for code, output in ((101, "error[E0308]"),
                             (0, "test result: ok. 0 passed; 0 failed; 1 ignored;"),
                             (137, failed), (101, failed + "\nerror[E0308]")):
            self.assertEqual(defects.classify(code, output, "rust"), "error")

    def test_mutations_are_exact_and_never_silently_noop(self):
        with tempfile.TemporaryDirectory(dir=defects.runner.ROOT / "evidence") as directory:
            path = Path(directory) / "source"
            path.write_text("fixed fixed")
            with self.assertRaisesRegex(ValueError, "preimage"):
                defects.replace_exact(path, "fixed", "broken")
            self.assertEqual(path.read_text(), "fixed fixed")
            defects.replace_exact(path, "fixed", "broken", 2)
            self.assertEqual(path.read_text(), "broken broken")
            with self.assertRaises(ValueError):
                defects.replace_exact(path, "fixed", "broken", 2)

    def test_corpus_has_unique_required_ids_and_nonempty_mutations(self):
        rows = json.loads(defects.CORPUS.read_text())
        ids = [row["id"] for row in rows]
        self.assertEqual(len(ids), len(set(ids)))
        self.assertTrue({"oracle-worker-code", "decimal256-carry",
                         "publication-identity-0135-0136", "adjudication-summary-hash",
                         "c1-run-id-replay", "overbroad-audit-grants"} <= set(ids))
        for row in rows:
            self.assertTrue(row["fix"])
            for mutation in row["mutations"]:
                self.assertNotEqual(mutation["before"], mutation["after"])
                self.assertFalse(Path(mutation["path"]).is_absolute())
                self.assertNotIn("..", Path(mutation["path"]).parts)

    def test_fix_overlay_baseline_then_base_code_mutant(self):
        with tempfile.TemporaryDirectory(dir=defects.runner.ROOT / "evidence") as directory:
            scratch = Path(directory) / "tree"
            cell = Path(directory) / "cell"
            scratch.mkdir(); cell.mkdir()
            (scratch / "f.rs").write_text("carry >>= 8; carry >>= 8;")
            seen = []
            def fake(command, cwd, log, env, timeout):
                seen.append((scratch / "f.rs").read_text())
                ok = "test result: ok. 1 passed; 0 failed; 0 ignored;"
                bad = "test result: FAILED. 0 passed; 1 failed; 0 ignored;"
                return (0, ok, 1.0) if len(seen) == 1 else (101, bad, 1.0)
            defect = {"id": "d", "detector": "rust", "command": ["cargo", "test"],
                      "baseline_patches": [{"path": "f.rs", "before": "carry >>= 8;",
                                            "after": "carry = wide >> 8;", "count": 2}],
                      "mutations": [{"path": "f.rs", "before": "carry = wide >> 8;",
                                     "after": "carry >>= 8;", "count": 2}]}
            row = {}
            with patch.object(defects, "execute", side_effect=fake), \
                 patch.object(defects.subprocess, "check_output", return_value=b"diff"):
                defects.replay(defect, scratch, cell, {}, 10, row)
            self.assertEqual(seen, ["carry = wide >> 8; carry = wide >> 8;",
                                    "carry >>= 8; carry >>= 8;"])
            self.assertEqual(row["status"], "caught")
            self.assertTrue(row["defect_present_at_base"])
            self.assertEqual((scratch / "f.rs").read_text(), "carry >>= 8; carry >>= 8;")
            self.assertIn("| d |", defects.table([dict(row, id="d", fix="x", base="head",
                                                       layer="L0")]))

    def test_fix_overlay_skipped_when_base_already_has_the_fix(self):
        with tempfile.TemporaryDirectory(dir=defects.runner.ROOT / "evidence") as directory:
            scratch = Path(directory) / "tree"
            cell = Path(directory) / "cell"
            scratch.mkdir(); cell.mkdir()
            (scratch / "f.rs").write_text("carry = wide >> 8; carry = wide >> 8;")
            seen = []
            def fake(command, cwd, log, env, timeout):
                seen.append((scratch / "f.rs").read_text())
                ok = "test result: ok. 1 passed; 0 failed; 0 ignored;"
                bad = "test result: FAILED. 0 passed; 1 failed; 0 ignored;"
                return (0, ok, 1.0) if len(seen) == 1 else (101, bad, 1.0)
            defect = {"id": "d", "detector": "rust", "command": ["cargo", "test"],
                      "baseline_patches": [{"path": "f.rs", "before": "carry >>= 8;",
                                            "after": "carry = wide >> 8;", "count": 2}],
                      "mutations": [{"path": "f.rs", "before": "carry = wide >> 8;",
                                     "after": "carry >>= 8;", "count": 2}]}
            row = {}
            with patch.object(defects, "execute", side_effect=fake), \
                 patch.object(defects.subprocess, "check_output", return_value=b"diff"):
                defects.replay(defect, scratch, cell, {}, 10, row)
            self.assertEqual(seen, ["carry = wide >> 8; carry = wide >> 8;",
                                    "carry >>= 8; carry >>= 8;"])
            self.assertEqual(row["status"], "caught")
            self.assertNotIn("defect_present_at_base", row)
            # A half-applied fix is still a preimage mismatch, never a silent skip.
            (scratch / "f.rs").write_text("carry = wide >> 8; carry >>= 8;")
            row = {}
            with patch.object(defects, "execute", side_effect=fake):
                defects.replay(defect, scratch, cell, {}, 10, row)
            self.assertEqual(row["status"], "error")


if __name__ == "__main__":
    unittest.main()
