#!/usr/bin/env python3
"""Run pinned real PG17 pipeline tests; never connects to an existing database.

AT-PL-01 capability evidence only: synthetic target-side replay, rollback,
multipart and continuity checks do not close source/replica L0/L_end acceptance.
Each Rust test gets a new loopback-only cluster and private temporary directory.
The fixtures require synthetic postgres/fixture roles with trust authentication;
no inherited database credentials or customer endpoints are passed to them.
"""

import sys
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import signal
import socket
import subprocess
import tempfile
import time


ROOT = Path(__file__).resolve().parents[1]
# (integration target, exact test, required environment prefix, capabilities)
LIFECYCLE = (
    "native_c5_pg17", "pg17_c5_lifecycle", "OO_C5",
    ["transaction_rollback", "receipt_replay", "stale_generation_rejection",
     "incomplete_scan_rejection", "delete_reinsert"],
)
MULTIPART = (
    "native_c5_multipart_initialize_pg17",
    "retained_parts_hydrate_through_native_state_without_source_access",
    "OO_C5_INITIALIZER", ["retained_arrow_multipart", "hydration_replay"],
)
PUBLISHED = (
    "native_c5_published_baseline_pg17",
    "single_and_reordered_multipart_share_real_source_watermark",
    "OO_C5_PUBLISHED", ["shared_watermark_continuity", "reordered_multipart"],
)
EMPTY = (
    "native_c5_empty_baseline_pg17",
    "empty_only_keeps_baseline_and_requires_current_source_proof",
    "OO_C5_EMPTY", ["empty_baseline", "current_source_proof"],
)
MERGED = (
    "native_c5_merge_pg17", "merged_migrations_fresh_and_upgrade", "OO_C5",
    ["fresh_install", "upgrade"],
)
SUITES = {
    "integration-a": (LIFECYCLE, MULTIPART, PUBLISHED),
    "nightly-a-m": (LIFECYCLE, MULTIPART, PUBLISHED, EMPTY, MERGED),
}
SUITE_SECONDS = 840


class SuiteFailure(RuntimeError):
    """Safe diagnostic authored by this runner, never a driver exception."""


def isolated_env():
    # Allowlist rather than trying to enumerate customer DSNs and secrets.
    names = ("PATH", "HOME", "USER", "LOGNAME", "LANG", "LC_ALL", "SYSTEMROOT",
             "CARGO_HOME", "RUSTUP_HOME", "CARGO_TARGET_DIR")
    env = {key: os.environ[key] for key in names if key in os.environ}
    env.update(CARGO_INCREMENTAL="0", CARGO_BUILD_JOBS="2", CARGO_TERM_COLOR="never",
               RUST_BACKTRACE="0")
    return env


def discover_pg(env):
    override = os.environ.get("OO_CI_PG_BIN")
    if override:
        candidates = [Path(override)]
    else:
        found = shutil.which("postgres", path=env.get("PATH"))
        candidates = ([Path(found).parent] if found else []) + [
            Path("/opt/homebrew/opt/postgresql@17/bin"),
            Path("/usr/local/opt/postgresql@17/bin"),
        ]
    for directory in candidates:
        if not all((directory / name).is_file() and os.access(directory / name, os.X_OK)
                   for name in ("postgres", "initdb", "pg_ctl")):
            continue
        version = subprocess.run(
            [str(directory / "postgres"), "--version"], env=env,
            capture_output=True, text=True, timeout=10, check=True,
        ).stdout.strip()
        if re.search(r"\b17\.\d+", version):
            return directory.resolve(), version
    raise SuiteFailure("PG17 binaries unavailable; set OO_CI_PG_BIN or PATH")


def cargo_command(target, exact):
    return ["cargo", "test", "--locked", "-p", "oo-pipeline", "--test", target,
            "--", "--ignored", "--exact", exact, "--test-threads=1"]


def verify_test_output(text, exact):
    # Cargo/libtest exits zero for a misspelled filter: that is NOT a pass.
    summary = re.findall(
        r"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;", text)
    if summary != [("1", "0", "0")] or not re.search(
            rf"^test {re.escape(exact)} \.\.\. ok$", text, re.MULTILINE):
        raise SuiteFailure("exact test did not execute and pass exactly once")


class Runner:
    def __init__(self, receipt, evidence):
        self.receipt = receipt
        self.evidence = evidence
        self.env = isolated_env()
        self.deadline = time.monotonic() + SUITE_SECONDS

    def run(self, command, *, env=None, timeout=180, cleanup=False, check=True):
        command = [str(value) for value in command]
        remaining = self.deadline - time.monotonic()
        if not cleanup and remaining <= 0:
            raise SuiteFailure("suite exceeded 840-second execution budget")
        limit = timeout if cleanup else min(timeout, remaining)
        log = self.evidence / f"command-{len(self.receipt['commands']):02d}.log"
        record = {"argv": command, "exit_code": None, "timeout": False,
                  "log": str(log), "timeout_seconds": round(limit, 2)}
        self.receipt["commands"].append(record)
        start = time.monotonic()
        # Files avoid unbounded pipe buffering; stdout contains only our summary.
        with log.open("w") as stream:
            process = subprocess.Popen(command, cwd=ROOT, env=env or self.env,
                                       stdout=stream, stderr=subprocess.STDOUT,
                                       start_new_session=True)
            try:
                process.wait(timeout=limit)
            except subprocess.TimeoutExpired:
                record["timeout"] = True
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()
            except BaseException:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()
                raise
            finally:
                record["exit_code"] = process.returncode
                record["seconds"] = round(time.monotonic() - start, 3)
        if check and (record["timeout"] or process.returncode):
            raise SuiteFailure(f"command {len(self.receipt['commands']) - 1} failed; see private log")
        return log.read_text(errors="replace")

    def scenario(self, pg, spec):
        target, exact, prefix, coverage = spec
        result = {"target": target, "test": exact, "coverage": coverage,
                  "status": "failed", "tests_passed": 0, "tests_executed": 0,
                  "cleanup": False}
        self.receipt["results"].append(result)
        # Short socket paths also work on macOS. mkdtemp gives mode 0700.
        root = Path(tempfile.mkdtemp(prefix="oo-ci-pg-", dir="/tmp"))
        data = root / "data"
        result["cluster"] = str(root)
        env = self.env | {"TMPDIR": str(root), "TMP": str(root), "TEMP": str(root)}
        try:
            # Reserve a fresh loopback port until immediately before startup.
            with socket.socket() as reservation:
                reservation.bind(("127.0.0.1", 0))
                port = reservation.getsockname()[1]
                result["port"] = port
                env.update({f"{prefix}_PG_PORT": str(port), f"{prefix}_OWNED": "1"})
                self.run([pg / "initdb", "-D", data, "-U", "postgres", "-A", "trust",
                          "--no-locale", "--encoding=UTF8"], env=env, timeout=45)
                # Do not allow the default trust rule to reach non-loopback peers.
                (data / "pg_hba.conf").write_text(
                    "local all all trust\nhost all all 127.0.0.1/32 trust\n")
            self.run([pg / "pg_ctl", "-D", data, "-l", root / "server.log",
                      "-o", f"-h 127.0.0.1 -k {root} -p {port} "
                      "-c max_connections=20 -c shared_buffers=32MB "
                      "-c statement_timeout=120000 -c idle_in_transaction_session_timeout=120000",
                      "-w", "-t", "20", "start"], env=env, timeout=30)
            text = self.run(cargo_command(target, exact), env=env, timeout=180, check=False)
            counts = re.findall(r"test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed;", text)
            if len(counts) == 1 and re.search(rf"^test {re.escape(exact)} \.\.\.", text, re.MULTILINE):
                result["tests_executed"] = sum(map(int, counts[0]))
            command = self.receipt["commands"][-1]
            if command["exit_code"] != 0 or command["timeout"]:
                # Owned trust-auth loopback clusters carry no credentials; show
                # the test's own failure lines so CI names the cause.
                lines = [line for line in text.splitlines()
                         if not line.lstrip().startswith(("Compiling", "Downloaded", "Fresh"))]
                print(f"--- {exact} output tail ---\n" + "\n".join(lines[-60:]), file=sys.stderr)
                raise SuiteFailure(f"exact test {exact} failed; see private log")
            verify_test_output(text, exact)
            result.update(status="passed", tests_passed=1)
        finally:
            # pg_ctl targets ONLY our mkdtemp data dir, even after partial startup.
            try:
                if (data / "postmaster.pid").exists():
                    self.run([pg / "pg_ctl", "-D", data, "-m", "immediate",
                              "-w", "-t", "15", "stop"], env=env, timeout=20, cleanup=True)
                server_log = root / "server.log"
                if server_log.exists():
                    shutil.copyfile(server_log, self.evidence / f"{target}-server.log")
                shutil.rmtree(root)
                result["cleanup"] = True
            except Exception:
                result["status"] = "failed"
                raise


def source_info():
    def git(*args):
        return subprocess.check_output(["git", *args], cwd=ROOT, text=True,
                                       timeout=10, stderr=subprocess.DEVNULL).strip()
    files = [Path(__file__), ROOT / "Cargo.lock", ROOT / "Cargo.toml"]
    for directory in ("crates/oo-pipeline", "crates/oo-jobs"):
        files.extend(path for path in (ROOT / directory).rglob("*")
                     if path.is_file() and path.suffix in (".rs", ".sql", ".toml", ".py"))
    return {"head": git("rev-parse", "HEAD"), "dirty": bool(git("status", "--porcelain")),
            "tracked_diff_sha256": hashlib.sha256(git("diff", "HEAD").encode()).hexdigest(),
            "sha256": {str(path.relative_to(ROOT)): hashlib.sha256(path.read_bytes()).hexdigest()
                       for path in sorted(set(files))}}


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--suite", required=True, choices=tuple(SUITES))
    parser.add_argument("--output", required=True, type=Path, help="JSON receipt file")
    args = parser.parse_args(argv)
    output = args.output.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    evidence = Path(tempfile.mkdtemp(prefix=output.stem + "-", dir=output.parent))
    receipt = {"protocol": "oo.ci.capability-suite/v1",
               "suite": args.suite, "status": "failed", "failure": None,
               "acceptance": "AT-PL-01", "acceptance_closed": False,
               "tests_executed": 0, "cleanup_complete": False,
               "evidence_scope": "synthetic target-only owned PG17; no source/replica barrier proof",
               "commands": [], "results": [], "evidence": str(evidence)}
    start = time.monotonic()
    runner = Runner(receipt, evidence)
    previous_handler = signal.getsignal(signal.SIGTERM)

    def terminate(signum, frame):
        raise KeyboardInterrupt

    signal.signal(signal.SIGTERM, terminate)
    try:
        receipt["sha"] = source_info()
        pg, version = discover_pg(runner.env)
        receipt["postgres"] = {"bin": str(pg), "version": version}
        specs = SUITES[args.suite]
        # Compile only the pinned integration binaries, once, before starting PG.
        command = ["cargo", "test", "--locked", "-p", "oo-pipeline", "--no-run"]
        for target, _, _, _ in specs:
            command += ["--test", target]
        runner.run(command, timeout=600)
        for spec in specs:
            runner.scenario(pg, spec)
        if not specs or len(receipt["results"]) != len(specs) or any(
                item["tests_passed"] != 1 or not item["cleanup"] for item in receipt["results"]):
            raise SuiteFailure("missing test execution or cleanup evidence")
        receipt["status"] = "passed"
    except (Exception, KeyboardInterrupt) as exc:
        # Never echo exception text: third-party errors can contain connection URLs.
        receipt["failure"] = {"type": type(exc).__name__,
                              "message": str(exc) if isinstance(exc, SuiteFailure) else
                              "suite failed; inspect private command logs and results"}
    finally:
        signal.signal(signal.SIGTERM, previous_handler)
        receipt["tests_executed"] = sum(item["tests_executed"] for item in receipt["results"])
        receipt["cleanup_complete"] = all(item["cleanup"] for item in receipt["results"])
        receipt["seconds"] = round(time.monotonic() - start, 3)
        output.write_text(json.dumps(receipt, indent=2) + "\n")
        output.chmod(0o600)
    print(f"{args.suite}: {receipt['status']}; receipt: {output}")
    return 0 if receipt["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
