"""Black-box local fixtures only. Requires SCHEDULER_BIN; no installed runtime."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import unittest


BIN = os.environ["SCHEDULER_BIN"]
EVIDENCE = Path(os.environ["SCHEDULER_EVIDENCE_DIR"]) if os.environ.get("SCHEDULER_EVIDENCE_DIR") else None


class SchedulerCliTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="oo-scheduler-cli-")
        self.root = Path(self.temp.name)
        self.root.chmod(0o700)
        self.processes = []
        self.snapshots = []
        self.config = {
            "protocol": "oo.library.refresh-schedule/v1",
            "state_dir": "./state",
            "interval_seconds": 20,
            "cycle_timeout_seconds": 10,
            "max_attempts": 3,
            "retry_delay_seconds": 1,
            "command": {
                "argv": ["/bin/sh", "./fixture.sh", "{cycle_dir}", "fresh"],
                "resume_argv": ["/bin/sh", "./fixture.sh", "{cycle_dir}", "resume"],
            },
        }
        self.write("fixture.sh", 'printf \'{"success":true}\\n\'\n')

    def tearDown(self):
        for process in self.processes:
            if process.poll() is None:
                process.kill()
            process.communicate(timeout=5)
        if EVIDENCE:
            EVIDENCE.mkdir(parents=True, exist_ok=True)
            (EVIDENCE / (self._testMethodName + ".json")).write_text(
                json.dumps(self.snapshots, indent=2) + "\n"
            )
        self.temp.cleanup()

    def write(self, name, content):
        path = self.root / name
        path.write_text(content)
        path.chmod(0o600)
        return path

    def configure(self):
        self.write("schedule.json", json.dumps(self.config))

    def argv(self, action):
        return [BIN, "refresh-schedule", action, "--config", str(self.root / "schedule.json")]

    def call(self, action="run-once", code=0):
        result = subprocess.run(self.argv(action), capture_output=True, text=True, timeout=15)
        self.snapshots.append({
            "action": action, "exit_code": result.returncode,
            "stdout": result.stdout, "stderr": result.stderr,
        })
        self.assertEqual(result.returncode, code, result.stderr + result.stdout)
        return json.loads(result.stdout) if result.stdout else None

    def start(self):
        process = subprocess.Popen(self.argv("run-once"), stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        self.processes.append(process)
        return process

    def wait_for(self, predicate, timeout=5):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if predicate():
                return
            time.sleep(0.02)
        self.fail("local fixture condition timed out")

    def state(self):
        return json.loads((self.root / "state/state.json").read_text())

    def test_standard_help_and_anchor_are_read_only_until_due(self):
        help_result = subprocess.run([BIN, "--help"], capture_output=True, text=True, check=True)
        self.assertIn("refresh-schedule run-once|status|dry-run", help_result.stdout)
        help_result = subprocess.run([BIN, "refresh-schedule", "--help"], capture_output=True, text=True, check=True)
        self.assertIn("start_at_ms", help_result.stdout)
        self.assertIn("templates", help_result.stdout)
        self.config["start_at_ms"] = int(time.time() * 1000) + 28_800_000
        self.config["interval_seconds"] = 28800
        self.configure()
        before = sorted(self.root.rglob("*"))
        self.assertFalse(self.call("dry-run")["plan"]["will_execute_now"])
        self.assertFalse(self.call("status")["due"])
        self.assertEqual(before, sorted(self.root.rglob("*")))
        result = self.call()
        self.assertEqual(result["action"], "not_due")
        self.assertEqual(result["state"]["next_due_at_ms"], self.config["start_at_ms"])
        self.assertFalse((self.root / "state/cycles").exists())

    def test_real_seven_gate_template_resume_and_next_fresh_cycle(self):
        pack, scope = "a" * 64, "b" * 64
        self.write("stage.sh",
            'printf "%s\\n" "$1" >> "$2/stages.log"\n'
            'if [ "$1" = final_probe ] && [ ! -e ./allow ]; then\n'
            '  printf \'{"error":"local fixture withheld final probe"}\\n\'; exit 0\n'
            'fi\n'
            f'printf \'{{"ok":true,"pack_digest":"{pack}","schema_scope_digest":"{scope}"}}\\n\'\n')
        template = {
            "protocol": "oo.library.refresh/v1",
            "expected_pack_digest": pack,
            "schema_scope_digest": scope,
            "stages": {
                stage: {"argv": ["/bin/sh", "{config_dir}/stage.sh", stage, "{cycle_dir}"], "timeout": 1}
                for stage in ["prepare", "capture", "verify", "publish", "probe", "activate", "final_probe"]
            },
        }
        self.write("refresh-template.json", json.dumps(template))
        self.config["templates"] = {"refresh.json": "./refresh-template.json"}
        argv = [BIN, "refresh", "--config", "{cycle_dir}/refresh.json", "--out", "{cycle_dir}/gates"]
        self.config["command"] = {"argv": argv, "resume_argv": argv + ["--resume"]}
        self.config["interval_seconds"] = 4
        self.config["cycle_timeout_seconds"] = 3
        self.configure()
        first = self.call(code=2)
        self.assertEqual(first["action"], "retry_wait")
        cycle = first["state"]["cycle"]
        directory = self.root / "state/cycles" / cycle["id"]
        receipt = json.loads((directory / "gates/receipt.json").read_text())
        self.assertFalse(receipt["success"])
        self.assertEqual(receipt["stages"]["final_probe"]["status"], "failed")
        # Editing the source template cannot rewrite this already-started cycle.
        self.write("refresh-template.json", json.dumps(template | {"unused_future_change": True}))
        self.write("allow", "local fixture gate enabled")
        time.sleep(1.05)
        resumed = self.call()
        self.assertEqual(resumed["action"], "succeeded")
        self.assertEqual(resumed["state"]["cycle"]["id"], cycle["id"])
        self.assertEqual(resumed["state"]["cycle"]["deadline_at_ms"], cycle["deadline_at_ms"])
        self.assertEqual((directory / "stages.log").read_text().splitlines().count("prepare"), 1)
        self.assertEqual((directory / "stages.log").read_text().splitlines().count("final_probe"), 2)
        self.write("refresh-template.json", json.dumps(template))
        self.assertEqual(self.call()["action"], "not_due")
        due = resumed["state"]["next_due_at_ms"]
        time.sleep(max(0, due / 1000 - time.time()) + 0.03)
        fresh = self.call()
        self.assertEqual(fresh["action"], "succeeded")
        self.assertNotEqual(fresh["state"]["cycle"]["id"], cycle["id"])
        self.assertFalse(fresh["state"]["cycle"]["attempts"][0]["resume"])
        self.assertTrue((directory / "cycle.json").exists())

    def test_process_lock_and_killed_parent_do_not_overlap_surviving_child(self):
        self.write("fixture.sh",
            'printf "%s\\n" "$2" >> "$1/modes"\n'
            'if [ "$2" = fresh ]; then touch ./ready; sleep 1.5; touch ./finished; fi\n'
            'printf \'{"success":true}\\n\'\n')
        self.configure()
        first = self.start()
        self.wait_for(lambda: (self.root / "ready").exists())
        state = self.state()
        self.call(code=2)
        self.assertIn("schedule_lock_busy", self.snapshots[-1]["stderr"])
        self.assertTrue(self.call("status")["locked"])
        first.kill()
        first.communicate(timeout=5)
        self.assertEqual(self.call(code=2)["action"], "child_still_running")
        self.wait_for(lambda: (self.root / "finished").exists())
        time.sleep(0.1)
        resumed = self.call()
        self.assertEqual(resumed["action"], "succeeded")
        self.assertEqual(resumed["state"]["cycle"]["id"], state["cycle"]["id"])
        self.assertEqual(resumed["state"]["cycle"]["deadline_at_ms"], state["cycle"]["deadline_at_ms"])
        self.assertEqual(resumed["state"]["cycle"]["attempts"][0]["status"], "interrupted")
        directory = self.root / "state/cycles" / state["cycle"]["id"]
        self.assertEqual((directory / "modes").read_text(), "fresh\nresume\n")

    def test_deadline_kills_a_nested_refresh_stage_not_only_its_parent(self):
        # refresh.rs creates a process group per stage: the scheduler must stop
        # that local descendant too, rather than just its immediate child group.
        self.write("slow.sh", 'sleep 2; touch ./LATE_SIDE_EFFECT\nprintf \'{"success":true}\\n\'\n')
        pack, scope = "a" * 64, "b" * 64
        template = {
            "protocol": "oo.library.refresh/v1",
            "expected_pack_digest": pack, "schema_scope_digest": scope,
            "stages": {
                stage: {"argv": ["/bin/sh", "{config_dir}/slow.sh"], "timeout": 5}
                for stage in ["prepare", "capture", "verify", "publish", "probe", "activate", "final_probe"]
            },
        }
        self.write("refresh-template.json", json.dumps(template))
        self.config["templates"] = {"refresh.json": "./refresh-template.json"}
        argv = [BIN, "refresh", "--config", "{cycle_dir}/refresh.json", "--out", "{cycle_dir}/gates"]
        self.config["command"] = {"argv": argv, "resume_argv": argv + ["--resume"]}
        self.config["cycle_timeout_seconds"] = 1
        self.configure()
        result = self.call(code=2)
        self.assertEqual(result["action"], "expired")
        self.assertIsNone(result["state"]["last_success"])
        time.sleep(1.2)
        self.assertFalse((self.root / "LATE_SIDE_EFFECT").exists())

    def test_crash_downtime_expires_original_budget_without_reusing_old_success(self):
        self.write("fixture.sh",
            'printf "%s\\n" "$2" >> "$1/modes"\n'
            'touch ./ready; sleep 1.5; touch ./finished\n'
            'printf \'{"success":true}\\n\'\n')
        self.config["cycle_timeout_seconds"] = 1
        self.configure()
        first = self.start()
        self.wait_for(lambda: (self.root / "ready").exists())
        original = self.state()
        first.kill()
        first.communicate(timeout=5)
        deadline = original["cycle"]["deadline_at_ms"]
        time.sleep(max(0, deadline / 1000 - time.time()) + 0.05)
        blocked = self.call(code=2)
        self.assertEqual(blocked["action"], "child_still_running")
        self.assertEqual(blocked["state"]["cycle"]["status"], "expired")
        self.assertEqual(blocked["state"]["cycle"]["deadline_at_ms"], deadline)
        self.wait_for(lambda: (self.root / "finished").exists())
        time.sleep(0.1)
        skipped = self.call()
        self.assertEqual(skipped["action"], "not_due")
        self.assertIsNone(skipped["state"]["last_success"])
        self.assertEqual(len(skipped["state"]["cycle"]["attempts"]), 1)

    def test_nonzero_success_and_nested_zero_exit_error_are_not_success(self):
        self.config["max_attempts"] = 1
        self.write("fixture.sh", 'printf \'{"success":true,"nested":{"error":"fixture refusal"}}\\n\'\n')
        self.configure()
        result = self.call(code=2)
        self.assertEqual(result["action"], "failed")
        self.assertIsNone(result["state"]["last_success"])
        self.assertIn("schedule_structured_failure", result["state"]["last_failure"]["reason"])


if __name__ == "__main__":
    unittest.main(verbosity=2)
