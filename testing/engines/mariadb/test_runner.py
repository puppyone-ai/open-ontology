"""Safety gates do not require a Docker daemon or a database."""
import importlib.util
from pathlib import Path
from types import SimpleNamespace
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("family_runner", Path(__file__).with_name("run.py"))
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


class SafetyTests(unittest.TestCase):
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
