"""Safety gates do not require a Docker daemon or a database."""
import importlib.util
from pathlib import Path
from types import SimpleNamespace
import unittest
import sys
from unittest.mock import MagicMock, patch

spec = importlib.util.spec_from_file_location("family_runner", Path(__file__).with_name("run.py"))
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)
sys.path.insert(0, str(runner.ROOT / "tools/oo-test"))
from t2_p0 import duration_micros, mariadb_source_epoch


class SafetyTests(unittest.TestCase):
    def test_generated_bootstrap_credentials_are_redacted(self):
        directory = MagicMock()
        path = directory.__truediv__.return_value
        path.exists.return_value = True
        path.read_bytes.return_value = b"generated-fixture-value\n"
        self.assertEqual(
            runner.redact_generated(directory, b"bootstrap generated-fixture-value done"),
            b"bootstrap [REDACTED] done",
        )
        path.read_bytes.return_value = b""
        self.assertEqual(runner.redact_generated(directory, b"unchanged"), b"unchanged")

    def test_duration_keeps_fraction_sign_and_hours_beyond_day(self):
        self.assertEqual(duration_micros("-27:02:03.123456"), -97323123456)
        self.assertEqual(duration_micros("00:00:00.000001"), 1)
        self.assertEqual(duration_micros("-00:00:00.000001"), -1)
        self.assertEqual(duration_micros("00:00:00"), 0)
        self.assertIsNone(duration_micros(None))

    def test_epoch_matches_product_domain_identity(self):
        epoch = mariadb_source_epoch("731", "0", "0-731-2,7-42-1")
        self.assertEqual(epoch, "mariadb:731:0:0-731,7-42")
        self.assertEqual(epoch, mariadb_source_epoch("731", "0", "7-42-99,0-731-12"))
        self.assertNotEqual(epoch, mariadb_source_epoch("731", "1", "0-731-2,7-42-1"))
        self.assertNotEqual(epoch, mariadb_source_epoch("731", "0", "0-731-2,7-43-1"))

    def test_each_version_has_one_exact_platform_pin(self):
        for family, images in runner.IMAGES.items():
            platform = "linux/arm64" if family == "mariadb" else "linux/amd64"
            for image in images:
                self.assertRegex(runner.pinned_image(image, family, platform),
                                 r"^[a-z/-]+@sha256:[0-9a-f]{64}$")
        with self.assertRaisesRegex(RuntimeError, "platform image pin"):
            runner.pinned_image("mariadb:99.0.0", "mariadb", "linux/arm64")

    def test_low_disk_stops_before_docker(self):
        with patch.object(runner.shutil, "disk_usage", return_value=SimpleNamespace(free=0)), patch.object(runner, "run") as docker:
            for family in runner.IMAGES:
                with self.assertRaisesRegex(RuntimeError, "free host disk"):
                    runner.preflight(family)
            docker.assert_not_called()

    def test_oceanbase_runs_alone_and_two_is_the_lane_limit(self):
        with patch.object(runner.shutil, "disk_usage", return_value=SimpleNamespace(free=32 * 1024**3)):
            with patch.object(runner, "run", return_value=b"owned-one\n"):
                runner.preflight("mariadb")
                with self.assertRaisesRegex(RuntimeError, "alone"):
                    runner.preflight("oceanbase")
            with patch.object(runner, "run", return_value=b"owned-one\nowned-two\n"):
                with self.assertRaisesRegex(RuntimeError, "limit"):
                    runner.preflight("tidb")
            with patch.object(runner, "run", return_value=b""):
                runner.preflight("oceanbase")

    def test_all_families_require_twenty_gib(self):
        for family in runner.IMAGES:
            with patch.object(runner.shutil, "disk_usage", return_value=SimpleNamespace(free=20 * 1024**3 - 1)), patch.object(runner, "run") as docker:
                with self.assertRaisesRegex(RuntimeError, "20 GiB"):
                    runner.preflight(family)
                docker.assert_not_called()
            with patch.object(runner.shutil, "disk_usage", return_value=SimpleNamespace(free=20 * 1024**3)), patch.object(runner, "run", return_value=b""):
                runner.preflight(family)


if __name__ == "__main__":
    unittest.main()
