import hashlib
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import build


class BuildTests(unittest.TestCase):
    def test_checksum_match(self):
        with tempfile.TemporaryDirectory(dir=build.ROOT) as tmp:
            path = Path(tmp) / "package"
            path.write_bytes(b"fixture")
            build.verify(path, hashlib.sha256(b"fixture").hexdigest())

    def test_wrong_package_never_creates_context(self):
        with tempfile.TemporaryDirectory(dir=build.ROOT) as tmp:
            archive = Path(tmp) / "wrong.zip"
            archive.write_bytes(b"wrong package")
            out = Path(tmp) / "build"
            with patch.object(build.subprocess, "run") as run:
                with self.assertRaisesRegex(ValueError, "checksum mismatch"):
                    build.prepare(archive, out)
                run.assert_not_called()
            self.assertFalse(out.exists())

    def test_requires_explicit_authorisation(self):
        with patch("sys.argv", ["build.py", "--archive", "unused.zip",
                               "--run-id", "test"]):
            with patch.object(build, "prepare") as prepare:
                with self.assertRaises(SystemExit) as error:
                    build.main()
                self.assertEqual(error.exception.code, 2)
                prepare.assert_not_called()

    def test_arm_package_is_a_distinct_pinned_download(self):
        self.assertNotEqual(build.ARM64["archive"], build.ARCHIVE_SHA256)
        self.assertIn("HWarm920_uos20", build.ARM64["iso_name"])
        self.assertTrue(build.ARM64["url"].startswith("https://download.dameng.com/"))
        with tempfile.TemporaryDirectory(dir=build.ROOT) as tmp:
            archive = Path(tmp) / "wrong.zip"
            archive.write_bytes(b"wrong package")
            out = Path(tmp) / "build"
            with patch.object(build.subprocess, "run") as run:
                with self.assertRaisesRegex(ValueError, "checksum mismatch"):
                    build.prepare(archive, out, arm64=True)
                run.assert_not_called()
            self.assertFalse(out.exists())

    def test_rejects_traversal_run_id(self):
        with patch("sys.argv", ["build.py", "--archive", "unused.zip",
                               "--run-id", "../escape",
                               "--owner-authorised-e340"]):
            with patch.object(build, "prepare") as prepare:
                with self.assertRaises(SystemExit) as error:
                    build.main()
                self.assertEqual(error.exception.code, 2)
                prepare.assert_not_called()

    def test_rejects_archive_outside_evidence(self):
        with patch("sys.argv", ["build.py", "--archive", "unused.zip",
                               "--run-id", "test",
                               "--owner-authorised-e340"]):
            with patch.object(build, "prepare") as prepare:
                with self.assertRaises(SystemExit) as error:
                    build.main()
                self.assertEqual(error.exception.code, 2)
                prepare.assert_not_called()


if __name__ == "__main__":
    unittest.main()
