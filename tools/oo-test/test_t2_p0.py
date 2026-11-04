import subprocess
import sys
import unittest
from pathlib import Path

import t2_p0


class P0Cycles(unittest.TestCase):
    def test_comparison_normalizes_transport_delimiters_not_values(self):
        self.assertEqual(t2_p0.canonical("1\tnew\n"), t2_p0.canonical(" 1 |new\n"))
        self.assertNotEqual(t2_p0.canonical("1|new\n"), t2_p0.canonical("1|old\n"))
        self.assertNotEqual(t2_p0.canonical("1|1.01\n"), t2_p0.canonical("1|1.02\n"))

    def test_invalid_scope_exits_before_creating_resources(self):
        entry = str(Path(__file__).with_name("t2_p0.py"))
        for engine, versions in [("mysql", "5.7,5.7"), ("oracle", "19"), ("mysql", "17")]:
            result = subprocess.run(
                [sys.executable, entry, "--engine", engine, "--versions", versions,
                 "--run-id", "invalid-unit"], capture_output=True, text=True)
            self.assertEqual(result.returncode, 2)

    def test_requested_mysql_patch_versions_are_explicit(self):
        self.assertEqual(list(t2_p0.MYSQL.values()), ["5.7.44", "8.0.44", "8.4.7", "9.5.0"])

    def test_oracle_miner_sampling_uses_the_root_container(self):
        queries = []

        class FakeRun:
            engine = "oracle"

            def sql(self, text, admin):
                queries.append(text)
                meter.stop.set()
                return 0, "2|1"

        meter = t2_p0.SourceMeter(FakeRun(), 0, 2, include_logminer=True)
        meter.sample()
        self.assertIsNone(meter.error)
        self.assertTrue(queries[0].startswith("ALTER SESSION SET CONTAINER=CDB$ROOT;"))
        self.assertIn("'FIXTURE_READER','C##OO_MINER'", queries[0])
        self.assertEqual(meter.samples[0][1:], (2, 1))


if __name__ == "__main__":
    unittest.main()
