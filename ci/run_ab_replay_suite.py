#!/usr/bin/env python3
"""Owned A/B replay gate (oo-eval and benchmarks/ab); success is not acceptance closure.

Runs the oo-eval Rust tests, the A/B harness Python tests, and a replay of the
committed recorded responses whose report must equal the committed report byte
for byte. No model endpoint or key is supplied and no network is needed: the
replay adapter only reads hash-pinned fixtures and recorded PG transcripts.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
TOOLS = ("query-result-adapter", "question-bank", "result-comparator", "sql-runner",
         "trial-statistics")
PYTHON_MODULES = ("test_ab", "test_live", "test_logistics", "test_numeric")
# Recorded on the hosted ubuntu-24.04 runner with rustc 1.98.1: the report binds
# the SHA-256 of each benchmark binary, so it is specific to that build host.
EXPECTED = ROOT / "benchmarks/ab/fixtures/replay-report.linux-amd64.json"
SCOPE = {
    "acceptance": [],
    "covered": ["oo-eval CLI argument contract and scorer wrapper (cargo test -p oo-eval)",
                "A/B harness scoring, health gate, prompt/hash/model drift and live-capture "
                "adapter unit tests with fake transports",
                "recorded-response replay of the demo subset reproducing the committed report"],
    "not_covered": ["any model call or live capture", "live PostgreSQL or Gateway enforcement",
                    "public benchmark E (gate-public-e, REQ-EVAL-04, AT-EVAL-04)",
                    "nightly benchmark slice (gate-nightly-e)", "headline accuracy"],
}


def selected_tests(text, kind):
    if kind == "rust":
        rows = re.findall(r"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;", text)
        executed = [row for row in rows if int(row[0]) or int(row[1]) or int(row[2])]
        if not executed or any(int(f) or int(i) for _, f, i in rows):
            raise ValueError("missing, empty, failed or ignored Rust test selection")
        return sum(int(row[0]) for row in executed)
    rows = re.findall(r"Ran (\d+) tests? in ", text)
    if not rows or int(rows[-1]) == 0 or not re.search(r"^OK$", text, re.M):
        raise ValueError("missing, empty, failed or skipped Python test selection")
    return int(rows[-1])


def inventory(output):
    cargo = ["cargo", "build", "--offline", "--locked", "--manifest-path"]
    commands = [("build-" + tool, cargo + [f"benchmarks/{tool}/Cargo.toml"], None, ROOT,
                 {"CARGO_TARGET_DIR": str(ROOT / "benchmarks" / tool / "target")})
                for tool in TOOLS]
    commands.append(("oo-eval", ["cargo", "test", "--offline", "--locked", "-p", "oo-eval"],
                     "rust", ROOT, {}))
    commands.append(("ab-python", [sys.executable, "-B", "-m", "unittest", "-v", *PYTHON_MODULES],
                     "python", ROOT / "benchmarks/ab", {}))
    commands.append(("replay", ["cargo", "run", "--offline", "--locked", "-q", "-p", "oo-eval",
                                "--", "ab", "--arms", "c0,c3", "--dataset", "demo",
                                "--output", str(output.relative_to(ROOT))], None, ROOT, {}))
    return commands


def compare(produced, expected):
    """Byte-for-byte replay check; returns the receipt section, raises on drift."""
    data = produced.read_bytes()
    want = expected.read_bytes() if expected.is_file() else None
    section = {"expected": str(expected.relative_to(ROOT)),
               "expected_sha256": hashlib.sha256(want).hexdigest() if want is not None else None,
               "produced_sha256": hashlib.sha256(data).hexdigest(),
               "byte_identical": data == want}
    report = json.loads(data)
    section["adapter"] = report.get("adapter")
    section["model_measured"] = report.get("model_measured")
    if report.get("adapter") != "recorded-responses" or report.get("model_measured") is not False:
        raise ValueError("replay was not a recorded-response replay")
    if data != want:
        # Public fixture output only; kept so a hosted run can be inspected.
        section["produced_report"] = data.decode()
    return section


def run_command(command, cwd, env, log, timeout):
    with log.open("w") as stream:
        process = subprocess.Popen(command, cwd=cwd, env=env, stdout=stream,
                                   stderr=subprocess.STDOUT, start_new_session=True)
        try:
            return process.wait(timeout=timeout), False
        except (subprocess.TimeoutExpired, KeyboardInterrupt):
            os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait(timeout=5)
            return process.returncode, True


def interrupted(signum, frame):
    raise KeyboardInterrupt


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--suite", choices=("ab-replay",), required=True)
    parser.add_argument("--output", type=Path, required=True, help="JSON receipt path; logs are stored beside it")
    args = parser.parse_args(argv)
    output = args.output.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    logs = Path(tempfile.mkdtemp(prefix=output.stem + "-logs-", dir=output.parent))
    # The CLI only writes a new directory under this worktree's ignored evidence/.
    (ROOT / "evidence").mkdir(exist_ok=True)
    replay = ROOT / "evidence" / f"ab-replay-gate-{os.getpid()}-{time.time_ns()}"
    receipt = {"protocol": "oo.ci.capability-suite/v1", "suite": args.suite,
               "acceptance_closed": False, "scope": SCOPE, "commands": [],
               "tests_executed": 0, "cleanup_complete": False, "model_calls": 0,
               "model_endpoint_supplied": False, "customer_services_touched": False,
               "docker_used": False, "status": "failed"}
    deadline = time.monotonic() + 1080
    # No model endpoint, key or profile reaches the harness.
    env = {k: v for k, v in os.environ.items()
           if not k.startswith(("PG", "OO_", "OPENAI", "ANTHROPIC", "MODEL"))}
    env.pop("PYTHONOPTIMIZE", None)
    env.update(CARGO_BUILD_JOBS="2", PYTHONDONTWRITEBYTECODE="1")
    previous_term = signal.signal(signal.SIGTERM, interrupted)
    try:
        if not shutil.which("cargo"):
            raise ValueError("cargo prerequisite missing")
        for label, command, kind, cwd, extra in inventory(replay):
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise ValueError("suite time budget exhausted")
            log = logs / (label + ".log")
            started = time.monotonic()
            code, timed_out = run_command(command, cwd, env | extra, log, remaining)
            record = {"id": label, "argv": command, "exit_code": code,
                      "timed_out": timed_out, "seconds": round(time.monotonic() - started, 3),
                      "log": str(log)}
            receipt["commands"].append(record)
            text = log.read_text(errors="replace")
            record["log_sha256"] = hashlib.sha256(text.encode()).hexdigest()
            if code or timed_out:
                record["failure_tail"] = text.splitlines()[-80:]
                print(f"--- {label} log tail ---\n" + "\n".join(record["failure_tail"]), file=sys.stderr)
                raise ValueError("command failed: " + label)
            if kind:
                record["passed_tests"] = selected_tests(text, kind)
                receipt["tests_executed"] += record["passed_tests"]
        receipt["replay"] = compare(replay / "report.json", EXPECTED)
        if not receipt["replay"]["byte_identical"]:
            raise ValueError("replay report differs from the committed report")
        receipt["tests_executed"] += 1
        receipt["status"] = "passed"
    except KeyboardInterrupt:
        receipt["error"] = "suite interrupted"
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        receipt["error"] = type(error).__name__ + ": " + (
            str(error) if isinstance(error, ValueError) else "prerequisite or subprocess failure")
    finally:
        if replay.exists():
            shutil.rmtree(replay)
        receipt["cleanup_complete"] = not replay.exists()
        if not receipt["cleanup_complete"]:
            receipt["status"] = "failed"
        output.write_text(json.dumps(receipt, indent=2) + "\n")
        signal.signal(signal.SIGTERM, previous_term)
    print(json.dumps({"suite": args.suite, "status": receipt["status"], "receipt": str(output)}))
    return 0 if receipt["status"] == "passed" else 1


if __name__ == "__main__":
    sys.exit(main())
