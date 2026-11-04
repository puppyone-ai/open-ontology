import argparse
import json
from pathlib import Path
import subprocess
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import runner


class DiskGateTests(unittest.TestCase):
    def test_waits_then_records_recovery(self):
        with tempfile.TemporaryDirectory() as directory:
            with patch("runner.shutil.disk_usage", side_effect=[
                SimpleNamespace(free=19 * 1024**3), SimpleNamespace(free=21 * 1024**3)
            ]), patch("runner.time.sleep") as sleep:
                runner.disk_gate(Path(directory))
            self.assertEqual(sleep.call_count, 1)
            self.assertEqual(len(json.loads((Path(directory) / "disk-gate.json").read_text())["observations"]), 2)

    def test_timeout_never_starts_work(self):
        with tempfile.TemporaryDirectory() as directory:
            with patch("runner.shutil.disk_usage", return_value=SimpleNamespace(free=0)):
                with self.assertRaisesRegex(RuntimeError, "host_disk_below_floor"):
                    runner.disk_gate(Path(directory), wait_seconds=0)

    def test_cached_image_is_not_pulled_and_missing_image_is_gated(self):
        for cached in (True, False):
            with tempfile.TemporaryDirectory() as directory:
                run = runner.Run(Path(directory), "postgres", "17", argparse.Namespace())
                result = subprocess.CompletedProcess([], 0 if cached else 1, "[]", "")
                with patch("runner.subprocess.run", return_value=result) as command, \
                     patch("runner.shutil.disk_usage", return_value=SimpleNamespace(free=24 * 1024**3)):
                    if cached:
                        self.assertEqual(run.command(["docker", "pull", "image@sha256:test"])[0], 0)
                    else:
                        with self.assertRaisesRegex(RuntimeError, "image_pull_floor"):
                            run.command(["docker", "pull", "image@sha256:test"])
                self.assertEqual(command.call_count, 1)
                self.assertEqual(command.call_args.args[0][:3], ["docker", "image", "inspect"])
