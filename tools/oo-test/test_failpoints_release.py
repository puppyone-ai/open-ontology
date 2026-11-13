from pathlib import Path
import tempfile
import unittest

import failpoints_release as release
import runner


class ReleaseCheckTests(unittest.TestCase):
    def test_points_come_from_sources_and_release_must_be_clean(self):
        names = release.point_names()
        self.assertIn("load_chunk_commit", names)
        self.assertIn("c1_apply_commit", names)
        with tempfile.TemporaryDirectory(dir=runner.ROOT / "evidence") as directory:
            clean, full = Path(directory) / "clean", Path(directory) / "full"
            clean.write_bytes(b"\x00binary\x00")
            full.write_bytes(b"\x00".join(n.encode() for n in ["OO_FAILPOINTS", *names]))
            self.assertEqual(release.check(clean, full)["status"], "passed")
            self.assertEqual(release.check(full, full)["status"], "failed")
            self.assertEqual(release.check(clean, clean)["status"], "failed")


if __name__ == "__main__":
    unittest.main()
