import contextlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import recompare
from run import ROOT, TABLES, canonical_digest


class ComparisonTests(unittest.TestCase):
    def test_normalization_ignores_object_key_order_not_row_order(self):
        self.assertEqual(canonical_digest('{"id":1,"v":null}\n'),
                         canonical_digest('{"v":null,"id":1}\n'))
        self.assertNotEqual(canonical_digest('{"id":1}\n{"id":2}\n'),
                            canonical_digest('{"id":2}\n{"id":1}\n'))

    def test_null_empty_and_exact_decimal_remain_distinct(self):
        for a, b in [('{"v":null}', '{"v":""}'),
                     ('{"v":1.25}', '{"v":"1.25"}'),
                     ('{"v":12345678901234567890.01}', '{"v":12345678901234567890.02}')]:
            self.assertNotEqual(canonical_digest(a), canonical_digest(b))
        self.assertEqual(canonical_digest("")["rows"], 0)

    def test_invalid_json_is_not_ignored(self):
        with self.assertRaises(ValueError):
            canonical_digest('NOTICE: missing data\n')

    def test_comparison_chunks_cover_all_rows(self):
        result = canonical_digest('{"id":1}\n' * 10_001)
        self.assertEqual(result["rows"], 10_001)
        self.assertEqual([c["rows"] for c in result["chunks"]], [10_000, 1])

    def test_recomparison_preserves_failures_and_refuses_overwrite(self):
        with tempfile.TemporaryDirectory(dir=ROOT / "evidence") as temporary:
            root = Path(temporary)
            out = root / "evidence/check"
            out.mkdir(parents=True)
            for name in ["tools/oo-test/comparison.py", "testing/engines/kingbase/run.py",
                         "testing/engines/kingbase/recompare.py"]:
                p = root / name
                p.parent.mkdir(parents=True, exist_ok=True)
                p.write_text("test")
            for cycle in range(1, 4):
                for table in TABLES:
                    for side in ["source", "target"]:
                        (out / f"c5-{cycle}-{table}-{side}.stdout").write_text('{"id":1}\n')
            (out / "c5-2-items-target.stdout").write_text('{"id":2}\n')
            with patch.object(recompare, "ROOT", root), patch(
                    "sys.argv", ["recompare", "--run-id", "check"]), contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(recompare.main(), 1)
                self.assertFalse(json.loads((out / "typed-recomparison.json").read_text())["pass"])
                with self.assertRaises(FileExistsError):
                    recompare.main()


if __name__ == "__main__":
    unittest.main()
