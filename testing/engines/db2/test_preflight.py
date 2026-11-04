import contextlib
import io
import json
import subprocess
import unittest
from unittest.mock import patch

import preflight


class PreflightTests(unittest.TestCase):
    def assess(self, disk=preflight.MIN_FREE_BYTES, ids=(), memory=preflight.MIN_MEMORY_BYTES):
        return preflight.assess(disk, ids, memory)

    def test_exact_disk_threshold(self):
        self.assertTrue(self.assess()["ready"])
        self.assertFalse(self.assess(disk=preflight.MIN_FREE_BYTES - 1)["ready"])

    def test_exact_memory_threshold(self):
        self.assertTrue(self.assess()["ready"])
        self.assertFalse(self.assess(memory=preflight.MIN_MEMORY_BYTES - 1)["ready"])

    def test_lane_containers_block_without_leaking_ids(self):
        report = self.assess(ids=["lane-service"])
        self.assertFalse(report["ready"])
        self.assertEqual(report["lane_running_container_count"], 1)
        self.assertNotIn("lane-service", json.dumps(report))

    def test_all_blockers_are_reported(self):
        self.assertEqual(len(self.assess(disk=0, ids=["id"], memory=0)["blockers"]), 3)

    def test_host_readiness_is_not_product_acceptance(self):
        self.assertEqual(set(self.assess()["product_suites"].values()), {"not_run"})

    def test_memory_units_and_invalid_values(self):
        self.assertEqual(preflight.memory_bytes("1.5GiB"), 1610612736)
        self.assertEqual(preflight.memory_bytes("512MiB"), 536870912)
        self.assertEqual(preflight.memory_bytes("0B"), 0)
        for value in ("", "-1GiB", "NaNGiB", "1GB", "private endpoint"):
            with self.subTest(value=value), self.assertRaises(ValueError):
                preflight.memory_bytes(value)

    def test_unrelated_containers_do_not_block(self):
        with patch.object(preflight.subprocess, "run") as run, \
                patch.object(preflight.shutil, "disk_usage") as disk, \
                contextlib.redirect_stdout(io.StringIO()) as output:
            run.side_effect = [
                subprocess.CompletedProcess([], 0, stdout=value)
                for value in ("", "", str(8 * 1024**3), "2GiB / 8GiB\n")
            ]
            disk.return_value.free = preflight.MIN_FREE_BYTES
            self.assertEqual(preflight.main(), 0)
            self.assertEqual([call.args[0] for call in run.call_args_list], [
                ["docker", "ps", "--quiet", "--no-trunc", "--filter", "label=oo.lane=e"],
                ["docker", "ps", "--quiet", "--no-trunc", "--filter", "label=oo.lane=E"],
                ["docker", "info", "--format", "{{.MemTotal}}"],
                ["docker", "stats", "--no-stream", "--format", "{{.MemUsage}}"],
            ])
            self.assertEqual(json.loads(output.getvalue())["docker_memory_headroom_bytes"],
                             preflight.MIN_MEMORY_BYTES)

    def test_invalid_memory_accounting_fails_closed(self):
        for total, stats in (("0", ""), ("1", "2GiB / 8GiB"),
                             ("unknown", ""), ("8", "invalid")):
            with self.subTest(total=total, stats=stats), \
                    patch.object(preflight, "docker", side_effect=["", "", total, stats]), \
                    contextlib.redirect_stdout(io.StringIO()) as output:
                self.assertEqual(preflight.main(), 2)
                self.assertEqual(json.loads(output.getvalue())["blockers"], ["host_probe_failed"])

    def test_probe_errors_fail_closed_and_redact_diagnostics(self):
        for error in (OSError("private endpoint"), ValueError("private endpoint"),
                      subprocess.TimeoutExpired("private endpoint", 30),
                      subprocess.CalledProcessError(1, "private endpoint")):
            with self.subTest(error=type(error)), \
                    patch.object(preflight.subprocess, "run", side_effect=error), \
                    contextlib.redirect_stdout(io.StringIO()) as output:
                self.assertEqual(preflight.main(), 2)
                self.assertEqual(json.loads(output.getvalue())["blockers"], ["host_probe_failed"])
                self.assertNotIn("private endpoint", output.getvalue())


if __name__ == "__main__":
    unittest.main()
