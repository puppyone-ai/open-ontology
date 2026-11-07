import json
from pathlib import Path
import tempfile
import unittest
from verdict_rows import write


class VerdictTests(unittest.TestCase):
    def test_probe_cannot_promote_t0_or_small_fixture_to_matrix_pass(self):
        with tempfile.TemporaryDirectory(dir=Path(__file__).resolve().parents[3]) as directory:
            raw = Path(directory) / "report.json"
            report = dict(version_actual="11.5.9.0", scope="four-table focused acceptance", T1="pass", T8="pass")
            raw.write_text(json.dumps(report))
            rows = write("fixture", raw, cleanup_ok=True)
            self.assertEqual([r["result"] for r in rows], ["skip", "fail", "fail"])
            report["scope"] = "231 tables / 2M events / 300 columns"
            raw.write_text(json.dumps(report))
            rows = write("fixture", raw, cleanup_ok=True)
            self.assertEqual([r["result"] for r in rows], ["skip", "pass", "pass"])
            self.assertTrue(all(r["target"] == "db2-luw-11_5" for r in rows))
            self.assertEqual(write("fixture", raw, cleanup_ok=False)[1]["result"], "fail")
