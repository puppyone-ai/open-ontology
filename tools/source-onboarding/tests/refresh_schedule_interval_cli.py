"""Focused interval CLI fixtures; never point this at a customer configuration."""
import copy
import fcntl
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import unittest

BIN = os.environ["SCHEDULER_BIN"]
EVIDENCE = Path(os.environ["SCHEDULER_EVIDENCE_DIR"])


def encode(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()


class IntervalCliTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="oo-interval-cli-")
        self.root = Path(self.temp.name).resolve()
        self.log = []
        self.path = self.root / "config.json"
        self.config = {
            "protocol": "oo.library.refresh-schedule/v1",
            "state_dir": "./state",
            "start_at_ms": int(time.time() * 1000) + 120_000,
            "interval_seconds": 20,
            "cycle_timeout_seconds": 10,
            "retry_delay_seconds": 1,
            "max_attempts": 3,
            "command": {
                "argv": ["/bin/sh", "./fixture.sh", "{cycle_dir}"],
                "resume_argv": ["/bin/sh", "./fixture.sh", "{cycle_dir}", "--resume"],
            },
        }
        self.write(self.path, self.config)
        script = self.root / "fixture.sh"
        script.write_text("touch ./UNEXPECTED_SOURCE_WORK\nexit 1\n")
        script.chmod(0o600)
        self.assertEqual(self.call("run-once")["action"], "not_due")

    def tearDown(self):
        EVIDENCE.mkdir(parents=True, exist_ok=True)
        (EVIDENCE / (self._testMethodName + ".json")).write_text(json.dumps(self.log, indent=2) + "\n")
        self.assertFalse((self.root / "UNEXPECTED_SOURCE_WORK").exists())
        self.temp.cleanup()

    def write(self, path, value):
        with path.open("wb") as handle:
            os.fchmod(handle.fileno(), 0o600)
            handle.write(encode(value) + b"\n")
            handle.flush()
            os.fsync(handle.fileno())

    def call(self, action, seconds=None, code=0):
        args = [BIN, "refresh-schedule", action, "--config", str(self.path)]
        if seconds is not None:
            args += ["--seconds", str(seconds)]
        result = subprocess.run(args, capture_output=True, text=True, timeout=10)
        self.log.append({"args": args[1:], "code": result.returncode, "stdout": result.stdout, "stderr": result.stderr})
        self.assertEqual(result.returncode, code, result.stdout + result.stderr)
        return json.loads(result.stdout) if result.stdout else None

    def snapshot(self):
        return {str(path.relative_to(self.root)): path.read_bytes()
                for path in self.root.rglob("*") if path.is_file()}

    def test_help_interval_change_and_actionable_timeout_refusal(self):
        result = subprocess.run([BIN, "refresh-schedule", "--help"], capture_output=True, text=True, check=True)
        self.assertIn("set-interval --config", result.stdout)
        result = subprocess.run([BIN, "refresh-schedule", "set-interval", "--help"], capture_output=True, text=True, check=True)
        self.assertIn("interval-change.pending.json", result.stdout)
        changed = self.call("set-interval", 30)
        self.assertEqual(changed["action"], "interval_changed")
        self.assertEqual(changed["state"]["next_due_at_ms"], self.config["start_at_ms"])
        self.assertEqual(changed["state"]["sequence"], 0)
        current = json.loads(self.path.read_text())
        self.assertEqual(current, self.config | {"interval_seconds": 30})
        before = self.snapshot()
        self.call("set-interval", 9, code=2)
        self.assertIn("cycle_timeout_seconds", self.log[-1]["stderr"])
        self.assertEqual(before, self.snapshot())

    def test_concurrent_lock_is_the_existing_scheduler_lock(self):
        before = self.snapshot()
        with (self.root / "state/lock").open("r+") as handle:
            fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
            self.call("set-interval", 30, code=2)
            self.assertIn("schedule_lock_busy", self.log[-1]["stderr"])
            self.assertEqual(before, self.snapshot())
            fcntl.flock(handle, fcntl.LOCK_UN)
        self.assertEqual(self.call("set-interval", 30)["action"], "interval_changed")

    def test_atomic_config_rename_crash_prefix_recovers_without_jobs(self):
        for action in ["status", "dry-run", "run-once"]:
            old = json.loads(self.path.read_text())
            old_state = json.loads((self.root / "state/state.json").read_text())
            new = old | {"interval_seconds": old["interval_seconds"] + 1}
            new_hash = hashlib.sha256(encode(new)).hexdigest()
            changed = int(time.time() * 1000)
            new_state = copy.deepcopy(old_state)
            new_state.update(config_sha256=new_hash, observed_at_ms=changed,
                             anchor_at_ms=old["start_at_ms"], next_due_at_ms=old["start_at_ms"])
            journal = {
                "protocol": "oo.library.refresh-interval-change/v1",
                "config_path": str(self.path),
                "changed_at_ms": changed,
                "old_config_sha256": old_state["config_sha256"],
                "new_config_sha256": new_hash,
                "old_config": old, "new_config": new,
                "old_state": old_state, "new_state": new_state,
            }
            pending = self.root / "state/interval-change.pending.json"
            self.write(pending, journal)
            # Exact durable prefix of a crash between the config/state renames.
            temporary = self.root / "config.fixture-new"
            self.write(temporary, new)
            os.replace(temporary, self.path)
            recovered = self.call(action)
            self.assertEqual(recovered["action"], "interval_recovered")
            self.assertEqual(recovered["state"], new_state)
            self.assertFalse(pending.exists())
            self.assertFalse((self.root / "state/cycles").exists())

    def test_manual_hash_drift_is_not_accepted_as_interval_change(self):
        self.write(self.path, self.config | {"interval_seconds": 30})
        before = self.snapshot()
        self.call("set-interval", 40, code=2)
        self.assertIn("schedule_config_drift", self.log[-1]["stderr"])
        self.assertEqual(before, self.snapshot())


if __name__ == "__main__":
    unittest.main(verbosity=2)
