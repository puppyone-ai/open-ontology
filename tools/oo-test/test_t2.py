import subprocess
import unittest
from pathlib import Path

import t2


class T2Contract(unittest.TestCase):
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
