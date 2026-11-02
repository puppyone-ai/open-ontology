import copy
import io
import json
from pathlib import Path
import unittest
from unittest.mock import Mock, patch
import tempfile
import urllib.error
import zipfile

import gate_receipts as gates


class Receipts(unittest.TestCase):
    def setUp(self):
        self.gate = "gate-docs"
        self.source = "a" * 40
        self.base = "b" * 40
        self.run_info = {
            "id": 100, "run_attempt": 2, "head_sha": self.source,
            "repository": {"full_name": "fixture/repo"}, "event": "pull_request",
            "path": ".github/workflows/pr.yml",
            "pull_requests": [{"head": {"sha": self.source}, "base": {"sha": self.base}}],
        }
        self.job = {
            "id": 200, "run_id": 100, "run_attempt": 2, "head_sha": self.source,
            "name": "gate-docs", "status": "completed", "conclusion": "success",
            "labels": ["ubuntu-24.04"], "started_at": "2026-11-02T10:00:00Z",
            "completed_at": "2026-11-02T10:01:00Z",
            "steps": [{"name": "docs", "status": "completed", "conclusion": "success"}],
        }
        self.log = b'{"status":"pass","exit_code":0}\n'
        self.receipt = {
            "protocol": gates.PROTOCOL, "gate": self.gate, "job": self.gate,
            "repository": "fixture/repo", "run_id": 100, "run_attempt": 2,
            "source_commit": self.source, "command": gates.COMMANDS[self.gate].split(),
            "step_script_sha256": gates.sha(gates.COMMANDS[self.gate].encode()),
            "exit_code": 0, "log_sha256": gates.sha(self.log), "log_bytes": len(self.log),
            "started_at": "2026-11-02T10:00:05Z", "finished_at": "2026-11-02T10:00:55Z",
            "release_authority": False, "protected_github_policy": False,
        }
        self.artifact = {
            "id": 300, "expired": False,
            "workflow_run": {"id": 100, "head_sha": self.source},
        }

    def receipt_check(self, receipt):
        gates.check_receipt(receipt, self.log, self.gate, self.run_info, self.job)

    def test_matching_run_job_receipt(self):
        gates.check_run(self.run_info, "fixture/repo", 100, 2, self.source, "pull_request", self.base)
        gates.check_job(self.job, self.gate, 100, 2, self.source)
        self.receipt_check(self.receipt)

    def test_receipt_cannot_forge_source_command_result_or_log(self):
        for key, bad in [
            ("source_commit", self.base), ("run_id", 101), ("run_attempt", 1),
            ("exit_code", 1), ("exit_code", False), ("command", ["echo", "pass"]),
            ("step_script_sha256", "b" * 64), ("log_sha256", "c" * 64),
            ("log_bytes", 0), ("repository", "other/repo"),
            ("started_at", "2026-11-02T09:00:00Z"), ("finished_at", "2026-11-02T10:02:00Z"),
            ("release_authority", True), ("protected_github_policy", True),
        ]:
            with self.subTest(key=key, bad=bad):
                value = copy.deepcopy(self.receipt)
                value[key] = bad
                with self.assertRaises(gates.GateFailure):
                    self.receipt_check(value)

    def test_failed_skipped_wrong_attempt_and_selfhosted_jobs_fail(self):
        for key, value in [
            ("conclusion", "failure"), ("conclusion", "skipped"), ("status", "queued"),
            ("run_attempt", 1), ("head_sha", self.base), ("name", "other"),
            ("labels", ["self-hosted", "ubuntu-24.04"]),
        ]:
            job = copy.deepcopy(self.job)
            job[key] = value
            with self.subTest(key=key), self.assertRaises(gates.GateFailure):
                gates.check_job(job, self.gate, 100, 2, self.source)
        job = copy.deepcopy(self.job)
        job["steps"][0]["conclusion"] = "skipped"
        with self.assertRaises(gates.GateFailure):
            gates.check_job(job, self.gate, 100, 2, self.source)

    def test_forged_run_or_pr_identity_fails(self):
        for key, value in [
            ("run_attempt", 1), ("head_sha", self.base), ("pull_requests", []),
            ("repository", {"full_name": "other/repo"}), ("path", ".github/workflows/fake.yml"),
        ]:
            run = copy.deepcopy(self.run_info)
            run[key] = value
            with self.subTest(key=key), self.assertRaises(gates.GateFailure):
                gates.check_run(run, "fixture/repo", 100, 2, self.source, "pull_request", self.base)

    def archive(self, members=None):
        buf = io.BytesIO()
        with zipfile.ZipFile(buf, "w") as archive:
            if members is None:
                members = [
                    ("gate-docs-receipt.json", json.dumps(self.receipt)),
                    ("gate-docs.log", self.log),
                ]
            for name, body in members:
                archive.writestr(name, body)
        data = buf.getvalue()
        self.artifact["digest"] = "sha256:" + gates.sha(data)
        return data

    def test_authentic_archive_checks_digest_names_and_run(self):
        data = self.archive()
        receipt, log = gates.artifact_files(data, self.artifact, self.gate, 100, self.source)
        self.assertEqual(receipt, self.receipt)
        self.assertEqual(log, self.log)
        for mutation in ("digest", "expired", "source"):
            value = copy.deepcopy(self.artifact)
            if mutation == "source":
                value["workflow_run"]["head_sha"] = self.base
            elif mutation == "expired":
                value["expired"] = True
            else:
                value["digest"] = "sha256:" + "0" * 64
            with self.subTest(mutation=mutation), self.assertRaises(gates.GateFailure):
                gates.artifact_files(data, value, self.gate, 100, self.source)
        for names in [("../escape", "gate-docs.log"), ("gate-docs.log", "gate-docs.log")]:
            data = self.archive([(name, b"not a receipt") for name in names])
            with self.assertRaises(gates.GateFailure):
                gates.artifact_files(data, self.artifact, self.gate, 100, self.source)

    def test_zero_shell_exit_cannot_hide_structured_failure(self):
        log = b'{"status":"failed","exit_code":1}\n'
        value = self.receipt | {"log_sha256": gates.sha(log), "log_bytes": len(log)}
        with self.assertRaises(gates.GateFailure):
            gates.check_receipt(value, log, self.gate, self.run_info, self.job)

    def test_api_requires_auth_and_cannot_follow_foreign_api_path(self):
        with self.assertRaises(gates.GateFailure):
            gates.Github("fixture/repo", "")
        api = gates.Github("fixture/repo", "synthetic")
        with self.assertRaises(gates.GateFailure):
            api.fetch("https://external.invalid/")

    def test_duplicate_json_fields_fail(self):
        with self.assertRaises(gates.GateFailure):
            gates.parse_json('{"exit_code":0,"exit_code":1}')

    def test_redirect_does_not_forward_authorization(self):
        api = gates.Github("fixture/repo", "synthetic")
        location = "https://fixture.blob.core.windows.net/result.zip?signature=synthetic"
        first, second = Mock(), Mock()
        first.open.side_effect = urllib.error.HTTPError(
            "https://api.github.com", 302, "redirect", {"Location": location}, None)
        second.open.return_value = io.BytesIO(b"artifact")
        with patch.object(gates.urllib.request, "build_opener", side_effect=[first, second]):
            self.assertEqual(api.fetch("/actions/artifacts/1/zip", binary=True), b"artifact")
        initial = first.open.call_args.args[0]
        self.assertEqual(initial.get_header("Authorization"), "Bearer synthetic")
        self.assertIsInstance(second.open.call_args.args[0], str)
        self.assertEqual(second.open.call_args.args[0], location)
        for destination in ("http://fixture.blob.core.windows.net/a", "https://external.invalid/a"):
            first.open.side_effect = urllib.error.HTTPError(
                "https://api.github.com", 302, "redirect", {"Location": destination}, None)
            with patch.object(gates.urllib.request, "build_opener", return_value=first), self.assertRaises(gates.GateFailure):
                api.fetch("/actions/artifacts/1/zip", binary=True)

    def test_paginated_job_inventory_cannot_be_incomplete(self):
        api = gates.Github("fixture/repo", "synthetic")
        api.fetch = Mock(side_effect=[
            {"total_count": 101, "jobs": list(range(100))},
            {"total_count": 101, "jobs": [100]},
        ])
        self.assertEqual(len(api.pages("/jobs", "jobs")), 101)
        api.fetch = Mock(return_value={"total_count": 2, "jobs": [1]})
        with self.assertRaises(gates.GateFailure):
            api.pages("/jobs", "jobs")


class Policy(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.baseline = gates.yaml_document((gates.ROOT / "ci/baseline/gates-baseline.yaml").read_text())
        cls.current = gates.yaml_document((gates.ROOT / "ci/gates.yaml").read_text())

    def test_real_manifest_activates_all_four_capabilities(self):
        expected = gates.policy_jobs(self.baseline, self.current, [
            "crates/oo-pipeline/src/lib.rs", "crates/oo-gateway/src/lib.rs",
            "crates/oo-builder/src/lib.rs",
        ])
        self.assertEqual(expected, set(gates.COMMANDS))

    def test_cannot_remove_weaken_delay_or_retarget_gate(self):
        for kind in ("remove", "optional", "prefix", "deadline", "requirements", "success", "owner"):
            value = copy.deepcopy(self.current)
            target = next(g for g in value["gates"] if g["id"] == "gate-integration-a")
            if kind == "remove":
                value["gates"].remove(target)
            elif kind == "optional":
                target["status"] = "optional"
            elif kind == "prefix":
                target["path_prefix"] = "not-present/"
            elif kind == "deadline":
                target["latest_phase"] = "SP15"
            elif kind == "requirements":
                target["requirements"] = []
            elif kind == "success":
                target["closes_acceptance"] = True
            else:
                value["provenance"]["owner_appointed"] = True
            with self.subTest(kind=kind), self.assertRaises(gates.GateFailure):
                gates.policy_jobs(self.baseline, value, ["crates/oo-pipeline/src/lib.rs"])

    def test_unchanged_dormant_manifest_cannot_cover_new_capability(self):
        with self.assertRaises(gates.GateFailure):
            gates.policy_jobs(self.baseline, self.baseline, ["crates/oo-pipeline/src/lib.rs"])

    def test_foundation_workflow_commands_and_receipt_uploads_exist(self):
        for name in ("pr", "nightly"):
            workflow = gates.yaml_document((gates.ROOT / ".github/workflows" / (name + ".yml")).read_text())
            for gate, command in gates.COMMANDS.items():
                steps = workflow["jobs"][gates.JOB_NAMES[gate]]["steps"]
                record = next(s for s in steps if s.get("id") == "record")
                self.assertIn(command + " | tee ", record["run"])
                gates.check_step(record, gate)
                self.assertIn("gate_receipts.py record --gate " + gate, record["run"])
                upload = next(s for s in steps if s.get("uses", "").startswith("actions/upload-artifact@"))
                self.assertIn("github.run_attempt", upload["with"]["name"])
                self.assertIn("-receipt.json", upload["with"]["path"])

    def test_active_base_manifest_is_also_a_required_floor(self):
        lowered = copy.deepcopy(self.current)
        for gate in lowered["gates"]:
            if gate["id"].removeprefix("gate-") in gates.SUITES:
                gate["status"] = "not_applicable_until_phase"
        with self.assertRaises(gates.GateFailure):
            gates.policy_jobs(self.current, lowered, ["ci/gates.yaml"])

    def test_commented_dead_branch_or_modified_capture_is_refused(self):
        good = {"name": "docs", "id": "record", "shell": "bash", "run": gates.capture_script("gate-docs")}
        for body in [
            good["run"].replace("cargo xtask docs", "# cargo xtask docs"),
            "if false; then\n" + good["run"] + "\nfi\n",
            good["run"].replace("code=${PIPESTATUS[0]}", "code=0"),
            good["run"].replace('exit "${code}"', "exit 0"),
        ]:
            with self.assertRaises(gates.GateFailure):
                gates.check_step(good | {"run": body}, "gate-docs")
        with self.assertRaises(gates.GateFailure):
            gates.check_step(good | {"continue-on-error": True}, "gate-docs")


class FullVerification(unittest.TestCase):
    setUp = Receipts.setUp
    def test_full_authenticated_adapter_and_inventory_mutations(self):
        jobs, artifacts, downloads = [], [], {}
        for number, gate in enumerate(gates.COMMANDS):
            name = gates.JOB_NAMES[gate]
            job = copy.deepcopy(self.job)
            job.update(id=200 + number, name=name)
            job["steps"][0]["name"] = gates.STEP_NAMES[gate]
            jobs.append(job)
            log = self.log
            if gate == "gate-evidence":
                log = b'{"status":"self_test_pass","exit_code":0}\n'
            if gate == "gate-dependencies":
                log = b'{"status":"tools_ran","exit_code":0}\n'
            if gate.removeprefix("gate-") in gates.SUITES:
                log = json.dumps({"status": "pass", "exit_code": 0, "details": {"receipt": {
                    "protocol": "oo.ci.capability-suite/v1", "suite": gate.removeprefix("gate-"),
                    "status": "passed", "tests_executed": 1, "cleanup_complete": True,
                    "acceptance_closed": False, "commands": [{"argv": ["cargo", "test"], "exit_code": 0}],
                }}}).encode()
            receipt = self.receipt | {
                "gate": gate, "job": name, "command": gates.COMMANDS[gate].split(),
                "step_script_sha256": gates.sha(gates.COMMANDS[gate].encode()),
                "log_sha256": gates.sha(log), "log_bytes": len(log),
            }
            buf = io.BytesIO()
            with zipfile.ZipFile(buf, "w") as archive:
                archive.writestr(name + ".log", log)
                archive.writestr(name + "-receipt.json", json.dumps(receipt))
            data = buf.getvalue()
            artifact = self.artifact | {
                "id": 300 + number, "name": f"{name}-100-2",
                "digest": "sha256:" + gates.sha(data),
            }
            artifacts.append(artifact)
            downloads[f"/actions/artifacts/{artifact['id']}/zip"] = data
        api = Mock()
        api.fetch.side_effect = lambda path, **kwargs: self.run_info if path == "/actions/runs/100" else downloads[path]
        baseline = (gates.ROOT / "ci/baseline/gates-baseline.yaml").read_text()
        active = (gates.ROOT / "ci/gates.yaml").read_text()
        # The base tip need not be an ancestor: changed paths use the common ancestor.
        fork = "c" * 40
        calls = {
            ("rev-parse", "HEAD"): self.source, ("status", "--porcelain"): "",
            ("show", self.base + ":ci/baseline/gates-baseline.yaml"): baseline,
            ("show", self.base + ":ci/gates.yaml"): active,
            ("merge-base", self.base, self.source): fork,
            ("diff", "--name-only", fork, self.source): "ci/gate_receipts.py",
        }
        with tempfile.TemporaryDirectory() as directory:
            event = Path(directory) / "event.json"
            event.write_text(json.dumps({"pull_request": {
                "base": {"sha": self.base}, "head": {"sha": self.source}}}))
            env = {
                "GITHUB_REPOSITORY": "fixture/repo", "GITHUB_RUN_ID": "100",
                "GITHUB_RUN_ATTEMPT": "2", "GITHUB_TOKEN": "synthetic",
                "GITHUB_EVENT_NAME": "pull_request", "GITHUB_EVENT_PATH": str(event),
            }
            with patch.dict(gates.os.environ, env), patch.object(gates, "git", side_effect=lambda *args: calls[args]), patch.object(gates, "Github", return_value=api):
                api.pages.side_effect = [jobs, artifacts]
                result = gates.verify(self.base)
                self.assertEqual(len(result["proofs"]), 13)
                self.assertFalse(result["release_authority"])
                for broken_jobs, broken_artifacts in [
                    (jobs[:-1], artifacts), (jobs + jobs[:1], artifacts),
                    (jobs, artifacts[:-1]), (jobs, artifacts + artifacts[:1]),
                    (jobs, [a | {"name": a["name"].removesuffix("-2") + "-1"} for a in artifacts]),
                ]:
                    api.pages.side_effect = [broken_jobs, broken_artifacts]
                    with self.assertRaises(gates.GateFailure):
                        gates.verify(self.base)


if __name__ == "__main__":
    unittest.main()
