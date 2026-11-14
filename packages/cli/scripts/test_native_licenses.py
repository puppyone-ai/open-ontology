from pathlib import Path
import tempfile
import unittest

from licenses import NATIVE_LIBRARIES, TARGETS, native_inventory


class NativeLicenseTests(unittest.TestCase):
    def test_static_openssl_notice_is_pinned_and_linux_only(self):
        entry = NATIVE_LIBRARIES[0]
        record, body = native_inventory(entry, TARGETS)
        self.assertEqual(record["unresolved"], [])
        self.assertEqual(record["targets"], ["aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu"])
        self.assertEqual(record["license_files"][0]["sha256"], entry["sha256"])
        self.assertIn("Apache License", body)
        dockerfile = (Path(__file__).resolve().parents[3] / "ci/cli.Dockerfile").read_text()
        self.assertIn(entry["upstream_url"], dockerfile)
        self.assertIn(entry["tarball_sha256"], dockerfile)
        with tempfile.TemporaryDirectory() as directory:
            Path(directory, entry["file"]).write_text("tampered\n")
            record, _ = native_inventory(entry, TARGETS, Path(directory))
            self.assertEqual(record["unresolved"], ["native_notice_digest_mismatch"])


if __name__ == "__main__":
    unittest.main()
