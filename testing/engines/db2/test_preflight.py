import contextlib
import io
import json
import subprocess
import unittest
from unittest.mock import patch

import preflight


class PreflightTests(unittest.TestCase):
    def test_exact_disk_threshold(self):
        self.assertTrue(preflight.assess(preflight.MIN_FREE_BYTES, [])["ready"])
        self.assertFalse(preflight.assess(preflight.MIN_FREE_BYTES - 1, [])["ready"])

    def test_busy_host_is_blocked_without_leaking_container_ids(self):
        report = preflight.assess(preflight.MIN_FREE_BYTES, ["unrelated-service"])
        self.assertFalse(report["ready"])
        self.assertEqual(report["running_container_count"], 1)
        self.assertNotIn("unrelated-service", json.dumps(report))

    def test_both_blockers_are_reported(self):
        self.assertEqual(len(preflight.assess(0, ["id"])["blockers"]), 2)

    def test_host_readiness_is_not_product_acceptance(self):
        report = preflight.assess(preflight.MIN_FREE_BYTES, [])
        self.assertEqual(set(report["product_suites"].values()), {"not_run"})

    def test_command_only_lists_containers(self):
        with patch.object(preflight.subprocess, "run") as run, \
                patch.object(preflight.shutil, "disk_usage") as disk, \
                contextlib.redirect_stdout(io.StringIO()):
            run.return_value.stdout = "id\n"
            disk.return_value.free = preflight.MIN_FREE_BYTES
            self.assertEqual(preflight.main(), 2)
            run.assert_called_once_with(
                ["docker", "ps", "--quiet", "--no-trunc"],
                capture_output=True, text=True, check=True, timeout=30,
            )

    def test_probe_errors_fail_closed_and_redact_diagnostics(self):
        for error in (OSError("private endpoint"),
                      subprocess.TimeoutExpired("private endpoint", 30),
                      subprocess.CalledProcessError(1, "private endpoint")):
            with self.subTest(error=type(error)), \
                    patch.object(preflight.subprocess, "run", side_effect=error), \
                    contextlib.redirect_stdout(io.StringIO()) as output:
                self.assertEqual(preflight.main(), 2)
                self.assertEqual(json.loads(output.getvalue())["blockers"],
                                 ["host_probe_failed"])
                self.assertNotIn("private endpoint", output.getvalue())


if __name__ == "__main__":
    unittest.main()
