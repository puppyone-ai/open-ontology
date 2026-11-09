import subprocess
import json
import unittest
from pathlib import Path

import t2


class T2Contract(unittest.TestCase):
    def test_summary_metrics_do_not_require_metrics_on_table_outcomes(self):
        summary = dict(protocol="oo.sync/v1", mode="c1", durable_position={"lsn": 1},
                       target_write_bytes=0, new_generations=0)
        table = dict(protocol="oo.sync/v1", mode="c1", table={"name": "items"},
                     status="noop", stale_reason=None)
        encode = lambda *rows: "\n".join(json.dumps(r) for r in rows)
        self.assertEqual(t2.c1_summaries(encode(summary, table)), [summary])
        for rows in [(table,), (summary, summary), (dict(summary, error="failed"),),
                     (summary, dict(table, status="stale", stale_reason="source_timeout"))]:
            with self.assertRaises(RuntimeError):
                t2.c1_summaries(encode(*rows))
        missing = dict(summary)
        del missing["durable_position"]
        with self.assertRaises(RuntimeError):
            t2.c1_summaries(encode(missing))

    def test_lsn_order_is_numeric(self):
        self.assertLess(t2.lsn("F/FFFFFFFF"), t2.lsn("10/0"))
        for value in ("no-position", "0/X"):
            with self.assertRaises(ValueError):
                t2.lsn(value)

    def test_entrypoint_and_rejected_versions_do_not_start_containers(self):
        entry = Path(__file__).with_name("oo-test")
        result = subprocess.run([str(entry), "t2", "--help"], capture_output=True, text=True)
        self.assertEqual(result.returncode, 0)
        self.assertIn("PostgreSQL C1/C5", result.stdout)
        for versions in ("17,17", "12", "mysql"):
            result = subprocess.run(
                [str(entry), "t2", "--versions", versions, "--run-id", "unit-invalid"],
                capture_output=True, text=True)
            self.assertEqual(result.returncode, 2)


if __name__ == "__main__":
    unittest.main()
