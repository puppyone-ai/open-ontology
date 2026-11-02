#!/usr/bin/env python3
"""Record gate results and verify them against authenticated GitHub run metadata.

Local JSON is never execution authority. The verify command fetches the current
run, exact attempt's jobs and immutable artifacts from GitHub over HTTPS.
Engineering CI success does not appoint owners or authorize a release.
"""

import argparse
from datetime import datetime
import hashlib
import io
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import urllib.error
import urllib.parse
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[1]
MAX_DOWNLOAD = 64 * 1024 * 1024
PROTOCOL = "oo.github-gate-receipt/v1"
BASE_COMMANDS = {
    "gate-docs": "cargo xtask docs --check --format json",
    "gate-fmt": "cargo fmt --all -- --check",
    "gate-clippy": "cargo clippy --workspace --locked --all-targets -- -D warnings",
    "gate-unit": "cargo test --workspace --locked",
    "gate-contracts": "cargo xtask contracts --check --format json",
    "gate-architecture": "cargo xtask architecture --check --format json",
    "gate-secrets": "cargo xtask secrets --check --format json",
    "gate-dependencies": "cargo xtask supply --run-tools --format json",
    "gate-evidence": "cargo xtask evidence --self-test --format json",
}
SUITES = ("integration-a", "security-b", "ontology-d", "nightly-a-m")
COMMANDS = BASE_COMMANDS | {
    "gate-" + suite: f"cargo xtask test --suite {suite} --format json" for suite in SUITES
}
JOB_NAMES = {gate: ("secret-scan" if gate == "gate-secrets" else gate) for gate in COMMANDS}
STEP_NAMES = {
    "gate-docs": "docs", "gate-fmt": "fmt", "gate-clippy": "clippy",
    "gate-unit": "unit", "gate-contracts": "contracts", "gate-architecture": "architecture",
    "gate-secrets": "secret scan", "gate-dependencies": "dependencies",
    "gate-evidence": "evidence",
} | {"gate-" + suite: "capability" for suite in SUITES}


class GateFailure(RuntimeError):
    pass


def require(condition, code):
    if not condition:
        raise GateFailure(code)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def capture_script(gate):
    job, command = JOB_NAMES[gate], COMMANDS[gate]
    build_dir = 'export CARGO_TARGET_DIR="${RUNNER_TEMP}/secret-scan-build"\n' if gate == "gate-secrets" else ""
    return (
        'set -o pipefail\n'
        'rustc --version | tee "${RUNNER_TEMP}/rustc.txt"\n'
        "grep -q '^rustc 1.98.1 ' \"${RUNNER_TEMP}/rustc.txt\"\n"
        'started=$(date -u +%Y-%m-%dT%H:%M:%SZ)\n'
        'set +e\n' + build_dir +
        f'{command} | tee "${{RUNNER_TEMP}}/{job}.log"\n'
        'code=${PIPESTATUS[0]}\n'
        'set -e\n'
        'finished=$(date -u +%Y-%m-%dT%H:%M:%SZ)\n' +
        f'sha=$(sha256sum "${{RUNNER_TEMP}}/{job}.log" | awk \'{{print $1}}\')\n'
        f'script=$(printf \'%s\' \'{command}\' | sha256sum | awk \'{{print $1}}\')\n' +
        '{\n'
        '  echo "exit_code=${code}"\n'
        '  echo "log_sha256=${sha}"\n'
        '  echo "step_script_sha256=${script}"\n'
        '  echo "started_at=${started}"\n'
        '  echo "finished_at=${finished}"\n'
        '  echo "run_id=${GITHUB_RUN_ID}"\n'
        '} >> "${GITHUB_OUTPUT}"\n' +
        f'python3 ci/gate_receipts.py record --gate {gate} --log "${{RUNNER_TEMP}}/{job}.log" '
        f'--output "${{RUNNER_TEMP}}/{job}-receipt.json" --exit-code "${{code}}" '
        '--started "${started}" --finished "${finished}"\n'
        'exit "${code}"\n'
    )


def check_step(step, gate):
    require(set(step) == {"name", "id", "shell", "run"}, "workflow_step_fields")
    require(step["name"] == STEP_NAMES[gate] and step["id"] == "record" and
            step["shell"] == "bash", "workflow_step_identity")
    require(step["run"] == capture_script(gate), "workflow_capture_contract")


def parse_json(data):
    def unique(pairs):
        out = {}
        for key, value in pairs:
            require(key not in out, "json_duplicate_key")
            out[key] = value
        return out
    return json.loads(data, object_pairs_hook=unique)


def git(*args):
    return subprocess.check_output(["git", *args], cwd=ROOT, timeout=30).decode().strip()


def yaml_document(text):
    # Reuse the repository's existing Ruby YAML parser; never unsafe-load YAML.
    result = subprocess.run(
        ["ruby", "-ryaml", "-rjson", "-e", "puts JSON.generate(YAML.safe_load(STDIN.read))"],
        input=text, text=True, capture_output=True, timeout=30, check=True,
    )
    return parse_json(result.stdout)


def gate_map(document):
    require(document.get("schema_version") == "gates/v1", "policy_schema")
    gates = document.get("gates", [])
    require(isinstance(gates, list) and gates, "policy_empty")
    result = {}
    for gate in gates:
        require(gate["id"] not in result, "policy_duplicate_gate")
        result[gate["id"]] = gate
    return result


def phase(value):
    require(isinstance(value, str) and re.fullmatch(r"SP\d\d", value), "policy_phase")
    return int(value[2:])


def policy_jobs(baseline, candidate, changed):
    old, new = gate_map(baseline), gate_map(candidate)
    require(old.keys() <= new.keys(), "policy_gate_removed")
    require(phase(candidate["current_phase"]) >= phase(baseline["current_phase"]), "policy_phase_reversed")
    rank = {"optional": 0, "not_applicable_until_phase": 1, "pending": 2, "required": 3}
    expected = set()
    for name, gate in new.items():
        prior = old.get(name)
        require(gate.get("status") in rank, "policy_status")
        require(gate.get("closes_acceptance") is False, "policy_acceptance_claim")
        if prior:
            require(rank[gate["status"]] >= rank[prior["status"]], "policy_gate_weakened")
            for key in ("job", "evaluated_by", "capability", "path_prefix", "success_forbidden", "echo_forbidden"):
                require(gate.get(key) == prior.get(key), "policy_boundary_changed")
            for key in ("requirements", "acceptance"):
                require(set(prior.get(key, [])) <= set(gate.get(key, [])), "policy_requirement_removed")
            for key in ("effective_phase", "latest_phase"):
                if prior.get(key) is not None:
                    require(gate.get(key) is not None and phase(gate[key]) <= phase(prior[key]), "policy_deadline_extended")
        prefix = gate.get("path_prefix")
        introduced = prefix and any(p == prefix.rstrip("/") or p.startswith(prefix.rstrip("/") + "/") for p in changed)
        due = any(gate.get(key) and phase(candidate["current_phase"]) >= phase(gate[key])
                  for key in ("effective_phase", "latest_phase"))
        if introduced or due:
            require(gate["status"] == "required", "policy_capability_inactive")
        if gate["status"] == "required" and gate["evaluated_by"] == "job":
            require(name in COMMANDS and gate["job"] == name, "policy_unknown_command")
            require(gate.get("success_forbidden") is False, "policy_success_forbidden")
            expected.add(name)
    require(set(BASE_COMMANDS) <= expected, "policy_foundation_missing")
    provenance = candidate.get("provenance", {})
    require(provenance.get("protected_github_policy") is False, "policy_protected_claim")
    require(provenance.get("owner_appointed") is False, "policy_owner_claim")
    return expected


def record(gate, log, output, code, started, finished):
    require(gate in COMMANDS, "unknown_gate")
    require(not git("status", "--porcelain"), "source_dirty")
    path = Path(log)
    require(path.is_file() and not path.is_symlink(), "log_missing")
    content = path.read_bytes()
    require(len(content) <= MAX_DOWNLOAD, "log_oversized")
    receipt = {
        "protocol": PROTOCOL, "gate": gate, "job": JOB_NAMES[gate],
        "repository": os.environ["GITHUB_REPOSITORY"],
        "run_id": int(os.environ["GITHUB_RUN_ID"]),
        "run_attempt": int(os.environ["GITHUB_RUN_ATTEMPT"]),
        "source_commit": git("rev-parse", "HEAD"),
        "command": COMMANDS[gate].split(),
        "step_script_sha256": sha(COMMANDS[gate].encode()),
        "exit_code": code, "log_sha256": sha(content), "log_bytes": len(content),
        "started_at": started, "finished_at": finished,
        "release_authority": False, "protected_github_policy": False,
    }
    Path(output).write_text(json.dumps(receipt, sort_keys=True) + "\n")


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


class Github:
    def __init__(self, repository, credential):
        require(bool(re.fullmatch(r"[\w.-]+/[\w.-]+", repository)), "repository_shape")
        require(bool(credential), "github_auth_absent")
        self.base = "https://api.github.com/repos/" + repository
        self.authorization = "Bearer " + credential

    def fetch(self, path, *, binary=False):
        require(path.startswith("/") and ".." not in path, "api_path")
        request = urllib.request.Request(self.base + path, headers={
            "Authorization": self.authorization, "Accept": "application/vnd.github+json",
            "X-GitHub-Api-Version": "2022-11-28",
        })
        try:
            response = urllib.request.build_opener(NoRedirect).open(request, timeout=60)
        except urllib.error.HTTPError as error:
            if not binary or error.code not in (301, 302, 303, 307, 308):
                error.close()
                raise GateFailure("github_api_http_failure") from None
            location = error.headers.get("Location", "")
            error.close()
            parsed = urllib.parse.urlsplit(location)
            require(parsed.scheme == "https" and not parsed.username and not parsed.password, "artifact_redirect")
            host = parsed.hostname or ""
            require(host.endswith((".blob.core.windows.net", ".actions.githubusercontent.com",
                                   ".githubusercontent.com")), "artifact_redirect_host")
            # Signed artifact download: never forward the API Authorization header.
            response = urllib.request.build_opener(NoRedirect).open(location, timeout=60)
        with response:
            data = response.read(MAX_DOWNLOAD + 1)
        require(len(data) <= MAX_DOWNLOAD, "github_response_oversized")
        return data if binary else parse_json(data)

    def pages(self, path, key):
        out = []
        for page in range(1, 101):
            value = self.fetch(f"{path}?per_page=100&page={page}")
            items = value[key]
            require(isinstance(items, list), "api_list")
            out.extend(items)
            if len(items) < 100:
                require(len(out) == value["total_count"], "api_list_incomplete")
                return out
        raise GateFailure("api_list_excessive")


def check_run(run, repository, run_id, attempt, source, event, base):
    require(run["id"] == run_id and run["run_attempt"] == attempt, "run_identity")
    require(run["repository"]["full_name"] == repository and run["head_sha"] == source, "run_source")
    require(run["event"] == event, "run_event")
    require(run["path"] in (".github/workflows/pr.yml", ".github/workflows/nightly.yml"), "run_workflow")
    if event == "pull_request":
        matches = [p for p in run.get("pull_requests", [])
                   if p["head"]["sha"] == source and p["base"]["sha"] == base]
        require(len(matches) == 1, "run_pull_request_binding")
    else:
        require(event in ("schedule", "workflow_dispatch") and base == source, "run_base")


def check_job(job, gate, run_id, attempt, source):
    require(job["run_id"] == run_id and job["run_attempt"] == attempt, "job_run")
    require(job["head_sha"] == source and job["name"] == JOB_NAMES[gate], "job_source")
    require(job["status"] == "completed" and job["conclusion"] == "success", "job_not_success")
    require("ubuntu-24.04" in job["labels"] and "self-hosted" not in job["labels"], "job_runner")
    steps = [step for step in job["steps"] if step["name"] == STEP_NAMES[gate]]
    require(len(steps) == 1 and steps[0]["status"] == "completed" and
            steps[0]["conclusion"] == "success", "job_step")


def artifact_files(data, artifact, gate, run_id, source):
    require(artifact.get("digest") == "sha256:" + sha(data), "artifact_digest")
    require(artifact.get("expired") is False, "artifact_expired")
    require(artifact["workflow_run"]["id"] == run_id and
            artifact["workflow_run"]["head_sha"] == source, "artifact_run")
    job = JOB_NAMES[gate]
    expected = {job + ".log", job + "-receipt.json"}
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        infos = archive.infolist()
        require(len(infos) == 2 and {item.filename for item in infos} == expected, "artifact_members")
        require(sum(item.file_size for item in infos) <= MAX_DOWNLOAD, "artifact_unpacked_limit")
        for item in infos:
            require((item.external_attr >> 16) & 0o170000 != 0o120000, "artifact_symlink")
        return parse_json(archive.read(job + "-receipt.json")), archive.read(job + ".log")


def check_receipt(receipt, log, gate, run, job):
    require(all(type(receipt.get(key)) is int for key in
                ("run_id", "run_attempt", "exit_code", "log_bytes")), "receipt_integer")
    require(receipt["protocol"] == PROTOCOL and receipt["gate"] == gate and
            receipt["job"] == JOB_NAMES[gate], "receipt_identity")
    require(receipt["repository"] == run["repository"]["full_name"] and
            receipt["source_commit"] == run["head_sha"], "receipt_source")
    require(receipt["run_id"] == run["id"] and receipt["run_attempt"] == run["run_attempt"], "receipt_run")
    require(receipt["exit_code"] == 0, "receipt_exit")
    require(receipt["command"] == COMMANDS[gate].split() and
            receipt["step_script_sha256"] == sha(COMMANDS[gate].encode()), "receipt_command")
    require(receipt["log_bytes"] == len(log) and receipt["log_sha256"] == sha(log), "receipt_log")
    require(receipt["release_authority"] is False and receipt["protected_github_policy"] is False, "receipt_authority")
    start, end = (datetime.fromisoformat(receipt[k].replace("Z", "+00:00"))
                  for k in ("started_at", "finished_at"))
    job_start, job_end = (datetime.fromisoformat(job[k].replace("Z", "+00:00"))
                         for k in ("started_at", "completed_at"))
    require(job_start <= start <= end <= job_end, "receipt_time")
    # Structured commands may return shell success but report a failed gate.
    if gate not in ("gate-fmt", "gate-clippy", "gate-unit"):
        report = parse_json(log)
        expected_status = {
            "gate-evidence": "self_test_pass",
            "gate-dependencies": "tools_ran",
        }.get(gate, "pass")
        require(type(report.get("exit_code")) is int and report["exit_code"] == 0 and
                report.get("status") == expected_status, "report_failure")
        if gate.removeprefix("gate-") in SUITES:
            result = report["details"]["receipt"]
            require(result["protocol"] == "oo.ci.capability-suite/v1" and
                    result["suite"] == gate.removeprefix("gate-") and result["status"] == "passed" and
                    result["tests_executed"] > 0 and result["cleanup_complete"] is True and
                    result["acceptance_closed"] is False, "capability_receipt")
            require(result["commands"] and all(c["exit_code"] == 0 for c in result["commands"]), "capability_commands")


def verify(base):
    repository = os.environ["GITHUB_REPOSITORY"]
    run_id, attempt = int(os.environ["GITHUB_RUN_ID"]), int(os.environ["GITHUB_RUN_ATTEMPT"])
    source = git("rev-parse", "HEAD")
    require(re.fullmatch("[0-9a-f]{40}", base) is not None, "base_shape")
    require(not git("status", "--porcelain"), "source_dirty")
    event = os.environ["GITHUB_EVENT_NAME"]
    payload = parse_json(Path(os.environ["GITHUB_EVENT_PATH"]).read_bytes())
    if event == "pull_request":
        require(payload["pull_request"]["base"]["sha"] == base and
                payload["pull_request"]["head"]["sha"] == source, "event_binding")
    else:
        require(base == source, "event_base")
    baseline = yaml_document(git("show", base + ":ci/baseline/gates-baseline.yaml"))
    active_baseline = yaml_document(git("show", base + ":ci/gates.yaml"))
    candidate = yaml_document((ROOT / "ci/gates.yaml").read_text())
    fork = git("merge-base", base, source)
    changed = git("diff", "--name-only", fork, source).splitlines()
    expected = policy_jobs(baseline, candidate, changed)
    require(policy_jobs(active_baseline, candidate, changed) == expected, "policy_active_mismatch")
    api = Github(repository, os.environ.get("GITHUB_TOKEN", ""))
    run = api.fetch(f"/actions/runs/{run_id}")
    check_run(run, repository, run_id, attempt, source, event, base)
    jobs = api.pages(f"/actions/runs/{run_id}/attempts/{attempt}/jobs", "jobs")
    artifacts = api.pages(f"/actions/runs/{run_id}/artifacts", "artifacts")
    proofs = []
    workflow = yaml_document((ROOT / run["path"]).read_text())
    for gate in sorted(expected):
        found = [job for job in jobs if job["name"] == JOB_NAMES[gate]]
        require(len(found) == 1, "job_missing_or_duplicate")
        job = found[0]
        check_job(job, gate, run_id, attempt, source)
        steps = [s for s in workflow["jobs"][JOB_NAMES[gate]]["steps"] if s.get("id") == "record"]
        require(len(steps) == 1, "workflow_command")
        check_step(steps[0], gate)
        name = f"{JOB_NAMES[gate]}-{run_id}-{attempt}"
        found = [artifact for artifact in artifacts if artifact["name"] == name]
        require(len(found) == 1, "artifact_missing_or_duplicate")
        artifact = found[0]
        data = api.fetch(f"/actions/artifacts/{artifact['id']}/zip", binary=True)
        receipt, log = artifact_files(data, artifact, gate, run_id, source)
        check_receipt(receipt, log, gate, run, job)
        proofs.append({"gate": gate, "job_id": job["id"], "artifact_id": artifact["id"],
                       "artifact_sha256": sha(data), "log_sha256": sha(log)})
    return {
        "protocol": "oo.github-gates/v1", "status": "passed", "exit_code": 0,
        "source_commit": source, "base_commit": base, "run_id": run_id, "run_attempt": attempt,
        "authenticated_provenance": "github-api", "proofs": proofs,
        "release_authority": False, "protected_github_policy": False, "acceptance_closed": [],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    subs = parser.add_subparsers(dest="mode", required=True)
    rec = subs.add_parser("record")
    rec.add_argument("--gate", choices=COMMANDS, required=True)
    for name in ("log", "output", "started", "finished"):
        rec.add_argument("--" + name, required=True)
    rec.add_argument("--exit-code", type=int, required=True)
    check = subs.add_parser("verify")
    check.add_argument("--base", required=True)
    args = parser.parse_args()
    try:
        if args.mode == "record":
            record(args.gate, args.log, args.output, args.exit_code, args.started, args.finished)
        else:
            print(json.dumps(verify(args.base), sort_keys=True))
        return 0
    except GateFailure as error:
        code = str(error)
    except Exception:
        code = "verification_error"
    # Never echo API URLs with signed queries, credentials, raw logs or driver errors.
    print(json.dumps({"status": "failed", "exit_code": 1, "error": code,
                      "release_authority": False, "protected_github_policy": False}))
    return 1


if __name__ == "__main__":
    sys.exit(main())
