import copy
import json
import os
from pathlib import Path
import subprocess
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import t0


def report():
    return {
        "protocol": "oo.doctor/v1", "read_only": True, "status": "ok",
        "model": {"status": "ok"},
        "sources": [{
            "status": "ok",
            "probe": {
                "family": "postgres", "version": {"major": 17, "minor": 11, "patch": 0},
                "settings": {"wal_level": "replica"},
                "capabilities": {
                    name: {"state": "available"}
                    for name in ("catalog", "chunked_snapshot", "digest_diff", "log_cdc")
                },
            },
            "dba_requirements": [],
            "chunk_plans": [{
                "table": {"schema": "public", "name": "events"}, "strategy": "primary_key",
                "chunks": 1, "estimated_rows": 1000, "estimated_bytes": None,
                "bytes_basis": "unknown",
            }],
        }],
    }


class DoctorTests(unittest.TestCase):
    def validate(self, value, code=0, mode="auto"):
        return t0.validate(value, code, "17.11", "postgres", mode, ["public.events"])

    def test_healthy_unknown_estimates_and_two_component_version(self):
        self.assertTrue(self.validate(report())["ready"])

    def test_blocked_c1_requires_matching_exit_and_actionable_grants(self):
        value = report()
        value["status"] = value["sources"][0]["status"] = "blocked"
        source = value["sources"][0]
        source["probe"]["capabilities"]["log_cdc"] = {"state": "needs_setup"}
        source["dba_requirements"] = [{"kind": "privilege", "dba_statement": "ALTER ROLE reader REPLICATION;"}]
        self.assertFalse(self.validate(value, 1, "C1")["ready"])
        with self.assertRaises(ValueError):
            self.validate(value, 0, "C1")
        source["dba_requirements"][0]["dba_statement"] = None
        with self.assertRaises(ValueError):
            self.validate(value, 1, "C1")

    def test_fail_closed_on_missing_or_inconsistent_json(self):
        original = report()
        cases = []
        for key in ("protocol", "read_only", "sources", "model"):
            value = copy.deepcopy(original)
            del value[key]
            cases.append(value)
        for key in ("probe", "chunk_plans", "dba_requirements"):
            value = copy.deepcopy(original)
            del value["sources"][0][key]
            cases.append(value)
        value = copy.deepcopy(original)
        value["sources"][0]["probe"]["version"]["minor"] = 10
        cases.append(value)
        value = copy.deepcopy(original)
        value["sources"][0]["chunk_plans"] = []
        cases.append(value)
        value = copy.deepcopy(original)
        value["sources"][0]["chunk_plans"][0]["estimate_error"] = "denied"
        cases.append(value)
        value = copy.deepcopy(original)
        value["sources"][0]["probe"]["capabilities"]["digest_diff"]["state"] = "unsupported"
        cases.append(value)
        value = copy.deepcopy(original)
        value["sources"][0]["error"] = "source timeout"
        cases.append(value)
        for value in cases:
            with self.subTest(value=value), self.assertRaises((ValueError, KeyError)):
                self.validate(value, mode="C5")
        with self.assertRaises(ValueError):
            self.validate(original, 1)

    def test_worker_shim_only_relabels_exact_product_invocation(self):
        with tempfile.TemporaryDirectory(dir=t0.ROOT / "evidence") as temporary:
            home = Path(temporary)
            docker = home / "record-docker"
            docker.write_text(f"#!{t0.sys.executable}\nimport json,sys\nprint(json.dumps(sys.argv[1:]))\n")
            docker.chmod(0o700)
            image = "sha256:" + "a" * 64
            with patch("t0.shutil.which", return_value=str(docker)):
                name, env = t0.worker_environment(SimpleNamespace(id="owned-run"), home, image)
            args = ["run", "--rm", "-i", "--platform", "linux/amd64", "--label", "oo.lane=a",
                    "--cap-drop", "ALL", "--security-opt", "no-new-privileges", image]
            proc = subprocess.run(["docker", *args], env=dict(os.environ, **env),
                                  capture_output=True, text=True, check=True)
            actual = json.loads(proc.stdout)
            self.assertIn("oo.lane=h", actual)
            self.assertNotIn("oo.lane=a", actual)
            self.assertIn("oo.run_id=owned-run", actual)
            self.assertIn(name, actual)
            self.assertEqual(actual[-1], image)
            self.assertEqual(subprocess.run(["docker", "ps"], env=dict(os.environ, **env)).returncode, 125)
            with patch("t0.shutil.which", return_value=str(docker)):
                next_name, _ = t0.worker_environment(SimpleNamespace(id="owned-run"), home, image)
            self.assertNotEqual(name, next_name, "successive modes must not race Docker --rm")
