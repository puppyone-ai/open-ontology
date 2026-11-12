"""Small behavioral detectors used by the historical mutation gate."""
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest

import runner
import t3t4


class ReplayChecks(unittest.TestCase):
    def test_c1_cycles_have_distinct_target_run_ids(self):
        root = runner.ROOT / "evidence"
        root.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=root) as directory:
            h = object.__new__(t3t4.Harness)
            h.calls = 0
            h.store = Path(directory)
            h.run = SimpleNamespace(path=h.store)
            h.target = SimpleNamespace(port=5432)
            h.binary = Path("oo-load")
            h.stack = {"sources": [{"id": "fixture", "_chunk_rows": 8}]}
            ids = []
            for cycle in ("baseline", "drained-1", "drained-2"):
                command = h.argv("fixture", cycle, "--sync", {})
                target = Path(command[command.index("--target") + 1])
                ids.append(json.loads(target.read_text())["run_id"])
            self.assertEqual(ids, ["baseline", "drained-1", "drained-2"])

    def test_mysql_patch_version_uses_change_column(self):
        for version in ("5.7", "5.7.44"):
            h = SimpleNamespace(run=SimpleNamespace(engine="mysql", version=version))
            statements, columns = t3t4.ddl(h, "rename_column")
            self.assertIn("CHANGE COLUMN", statements[0])
            self.assertEqual(columns, ("id", "label", "n"))


if __name__ == "__main__":
    unittest.main()
