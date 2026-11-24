import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.dont_write_bytecode = True
import super_ontology_live_suites as live  # noqa: E402


class LiveSuites(unittest.TestCase):
    def test_every_ignored_oo_docs_target_is_discovered(self):
        found = set(live.ignored_targets())
        on_disk = {p.stem for p in live.TESTS.glob("*.rs") if "#[ignore" in p.read_text(encoding="utf-8")
                   and any(l.lstrip().startswith("#[ignore") for l in p.read_text(encoding="utf-8").splitlines())}
        self.assertEqual(found, on_disk)
        self.assertTrue({"share_live", "suggest_live", "eval_live", "download", "faults", "bench_scale"} <= found)

    def test_special_kinds_name_real_targets(self):
        self.assertFalse(set(live.KINDS) - set(live.ignored_targets()))


if __name__ == "__main__":
    unittest.main()
