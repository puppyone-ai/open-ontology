import tempfile
from pathlib import Path
import unittest

import l1_clean


class CleanProbeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        (l1_clean.ROOT / "evidence").mkdir(exist_ok=True)

    def test_environment_does_not_inherit_product_overrides(self):
        with tempfile.TemporaryDirectory(dir=l1_clean.ROOT / "evidence") as directory:
            root = Path(directory)
            env = l1_clean.clean_env(root, root / "tools", root / "npm")
            self.assertEqual(env["HOME"], directory)
            self.assertEqual(set(env), {"HOME", "PATH", "TERM", "LANG", "TMPDIR",
                                        "npm_config_cache", "npm_config_update_notifier"})
            self.assertNotIn("OO_LITE_RUNTIME_IMAGE", env)

    def test_secret_scan_detects_bytes_without_returning_value(self):
        with tempfile.TemporaryDirectory(dir=l1_clean.ROOT / "evidence") as directory:
            path = Path(directory) / "terminal.raw"
            path.write_bytes(b"prefix private-value suffix")
            self.assertEqual(l1_clean.secret_hits([path], [b"private-value"]),
                             ["terminal.raw"])
            self.assertEqual(l1_clean.secret_hits([path], [b"absent"]), [])

    def test_ca_prompt_handles_terminal_cursor_spacing(self):
        self.assertTrue(l1_clean.reached_ca_prompt(b"TLS CA\x1b[1Csecret_ref"))
        self.assertTrue(l1_clean.reached_ca_prompt(b"TLS CA secret_ref"))
        self.assertFalse(l1_clean.reached_ca_prompt(b"Engine: postgres"))

    def test_gold_subset_only_uses_scalar_customers_orders(self):
        import re
        sql = (l1_clean.ROOT / "testing/engines/gold/postgres.sql").read_text()
        selected = []
        for question, statement in re.findall(
                r"-- (Q\d+):[^\n]*\n(.*?)(?=\n-- Q|\Z)", sql, re.S):
            if question in l1_clean.GOLD:
                selected.append(question)
                tables = re.findall(r"\b(?:FROM|JOIN)\s+(\w+)", statement, re.I)
                self.assertTrue(set(tables) <= {"customers", "orders"})
        self.assertEqual(selected, l1_clean.GOLD)
        self.assertNotIn("Q14", l1_clean.GOLD)


if __name__ == "__main__":
    unittest.main()
