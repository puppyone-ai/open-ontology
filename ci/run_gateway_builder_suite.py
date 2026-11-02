#!/usr/bin/env python3
"""Bounded local fixture gates; success is not acceptance closure."""
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
RUST_TARGETS = (
    "structure_draft_cli", "reviewed_effective", "ontology_artifact_diff",
    "role_pack", "role_pack_cli", "role_pack_bundle",
)
PYTHON_MODULES = tuple("test_library_" + name for name in (
    "selfbuild", "semantics", "assets", "metrics", "data_only", "role",
    "runtime", "build", "control",
))
SCOPE = {
    "security-b": {
        "acceptance": ["AT-DB-01", "AT-GW-01"],
        "covered": ["owned PG17 column denial and NOINHERIT positive/negative controls",
                    "database read-only write refusal and no side effects",
                    "real Gateway/CLI publication mismatch, revocation and token denial"],
        "not_covered": ["exhaustive role/table/column/region truth matrix and ACL/RLS dump",
                        "complete R1-R9 attack corpus", "public/customer runtime acceptance"],
    },
    "ontology-d": {
        "acceptance": ["AT-OOS-01"],
        "covered": ["synthetic 71-table Self-build with actual Rust compiler",
                    "curated/stale review and artifact digest regression",
                    "YAML roundtrip, role-pack privacy and semantic/data-only cache regression"],
        "not_covered": ["live source/model Self-build", "business-owner certification",
                        "full AT-OOS-01 acceptance evidence and public publication"],
    },
}


def selected_tests(text, kind):
    if kind == "rust":
        rows = re.findall(r"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;", text)
        if not rows or any(int(p) == 0 or int(f) or int(i) for p, f, i in rows):
            raise ValueError("missing, empty, failed or ignored Rust test selection")
        return sum(int(row[0]) for row in rows)
    rows = re.findall(r"Ran (\d+) tests? in ", text)
    if not rows or int(rows[-1]) == 0 or not re.search(r"^OK$", text, re.M):
        raise ValueError("missing, empty, failed or skipped Python test selection")
    return int(rows[-1])


def pg_bin():
    explicit = os.environ.get("OO_CI_PG_BIN")
    candidates = [Path(explicit)] if explicit else (
        ([Path(shutil.which("pg_ctl")).parent] if shutil.which("pg_ctl") else []) +
        [Path("/opt/homebrew/opt/postgresql@17/bin"), Path("/usr/local/opt/postgresql@17/bin")])
    for path in candidates:
        if all(os.access(path / name, os.X_OK) for name in ("postgres", "pg_ctl", "initdb", "psql")):
            version = subprocess.check_output([str(path / "postgres"), "--version"], text=True, timeout=10)
            if re.search(r"PostgreSQL\) 17\.", version):
                return path.resolve()
    raise ValueError("PG17 required: set OO_CI_PG_BIN, PATH, or install local Homebrew PG17")


def inventory(suite, scratch):
    cargo = ["cargo", "test", "--offline", "--locked", "-p", "oo-builder"]
    if suite == "security-b":
        return [("owned-pg17", [sys.executable, str(ROOT / "crates/oo-gateway/tests/f22_owned_pg.py"),
                                str(scratch)], "rust", ROOT)]
    commands = [("builder-binaries", ["cargo", "build", "--offline", "--locked", "-p",
                                     "oo-builder", "--bins"], None, ROOT)]
    for target in RUST_TARGETS:
        commands.append((target, cargo + ["--test", target, "--", "--test-threads=1"], "rust", ROOT))
    commands.append(("library", [sys.executable, "-m", "unittest", "-v", *PYTHON_MODULES],
                     "python", ROOT / "tools/pg-builder/scripts"))
    return commands


def redact(text, secrets):
    for secret in sorted(set(secrets), key=len, reverse=True):
        if secret:
            text = text.replace(secret, "[REDACTED]")
    return re.sub(r"(?i)(postgres(?:ql)?://)[^\s]+", r"\1[REDACTED]", text)


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


def cleanup_owned(scratch, binary):
    """Only stop clusters created beneath this invocation's private directory."""
    actions = []
    for data in scratch.glob("f22-owned-*/pg"):
        pidfile = data / "postmaster.pid"
        if pidfile.exists():
            lines = pidfile.read_text().splitlines()
            if len(lines) < 2 or Path(lines[1]).resolve() != data.resolve():
                raise ValueError("owned PG data path mismatch; refusing stop")
            stopped = subprocess.run([str(binary / "pg_ctl"), "-D", str(data), "-m",
                                      "immediate", "-w", "-t", "15", "stop"],
                                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=20)
            actions.append({"action": "stop_owned_pg", "exit_code": stopped.returncode})
            if stopped.returncode or pidfile.exists():
                raise ValueError("owned PG cleanup failed")
    return actions


def interrupted(signum, frame):
    raise KeyboardInterrupt


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--suite", choices=tuple(SCOPE), required=True)
    parser.add_argument("--output", type=Path, required=True, help="JSON receipt path; logs are stored beside it")
    args = parser.parse_args(argv)
    output = args.output.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    logs = Path(tempfile.mkdtemp(prefix=output.stem + "-logs-", dir=output.parent))
    scratch = Path(tempfile.mkdtemp(prefix="oo-ci-owned-"))
    scratch.chmod(0o700)
    receipt = {"protocol": "oo.ci.capability-suite/v1", "suite": args.suite,
               "acceptance_closed": False, "scope": SCOPE[args.suite], "commands": [],
               "tests_executed": 0, "cleanup_complete": False,
               "customer_services_touched": False, "docker_used": False, "status": "failed"}
    # Reserve time for termination, owned-PG shutdown and receipt writing inside 15 minutes.
    deadline = time.monotonic() + 840
    env = {k: v for k, v in os.environ.items() if not k.startswith(("PG", "OO_"))}
    env.pop("PYTHONOPTIMIZE", None)
    target = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")).resolve()
    env.update(CARGO_TARGET_DIR=str(target), CARGO_BUILD_JOBS="1",
               OO_LIBRARY_BUILDER_BIN=str(target / "debug/oo-builder"),
               PYTHONDONTWRITEBYTECODE="1")
    binary = None
    previous_term = signal.signal(signal.SIGTERM, interrupted)
    try:
        if not shutil.which("cargo"):
            raise ValueError("cargo prerequisite missing")
        if args.suite == "security-b":
            binary = pg_bin()
            env["OO_CI_PG_BIN"] = str(binary)
            if not shutil.which("openssl"):
                raise ValueError("openssl prerequisite missing")
            if os.geteuid() == 0:
                raise ValueError("owned initdb must run as an unprivileged user")
        for label, command, kind, cwd in inventory(args.suite, scratch):
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise ValueError("suite time budget exhausted")
            raw = scratch / (label + ".log")
            started = time.monotonic()
            code, timed_out = run_command(command, cwd, env, raw, remaining)
            record = {"id": label, "argv": command, "exit_code": code,
                      "timed_out": timed_out, "seconds": round(time.monotonic() - started, 3),
                      "log": str(logs / raw.name)}
            receipt["commands"].append(record)
            if code or timed_out:
                raise ValueError("command failed: " + label)
            if kind:
                record["passed_tests"] = selected_tests(raw.read_text(errors="replace"), kind)
                receipt["tests_executed"] += record["passed_tests"]
        if receipt["tests_executed"] == 0:
            raise ValueError("suite selected zero tests")
        receipt["status"] = "passed"
    except KeyboardInterrupt:
        receipt["error"] = "suite interrupted"
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        # Do not serialize exception output, command environment, or fixture credentials.
        receipt["error"] = type(error).__name__ + ": " + (
            str(error) if isinstance(error, ValueError) else "prerequisite or subprocess failure")
    finally:
        try:
            receipt["cleanup"] = cleanup_owned(scratch, binary) if binary else []
            receipt["owned_clusters_stopped"] = True
        except (ValueError, OSError, subprocess.SubprocessError):
            receipt["status"] = "failed"
            receipt["owned_clusters_stopped"] = False
            receipt["cleanup_error"] = "owned cluster cleanup failed; private scratch retained"
        secrets = [p.read_text().strip() for pattern in ("**/password", "**/*.token", "**/cli-token")
                   for p in scratch.glob(pattern) if p.is_file()]
        for record in receipt["commands"]:
            raw = scratch / Path(record["log"]).name
            text = redact(raw.read_text(errors="replace"), secrets)
            Path(record["log"]).write_text(text)
            record["log_sha256"] = hashlib.sha256(text.encode()).hexdigest()
        receipt["fixture_evidence"] = []
        for fixture in scratch.glob("f22-owned-*"):
            for name in ("setup.log", "cli-build.log", "test.log", "result.json", "cli-binary.json"):
                source = fixture / name
                if source.is_file():
                    destination = logs / ("f22-" + name)
                    text = redact(source.read_text(errors="replace"), secrets)
                    destination.write_text(text)
                    receipt["fixture_evidence"].append({
                        "path": str(destination), "sha256": hashlib.sha256(text.encode()).hexdigest()})
        # Preserve only explicitly public probe evidence, never PG data, keys or token files.
        for path in scratch.glob("f22-owned-*/database-probes.json"):
            destination = logs / "database-probes.json"
            destination.write_text(path.read_text())
            receipt["database_probes"] = str(destination)
        if receipt["owned_clusters_stopped"]:
            shutil.rmtree(scratch)
            receipt["cleanup_complete"] = True
        else:
            receipt["private_cleanup_path"] = str(scratch)
        output.write_text(json.dumps(receipt, indent=2) + "\n")
        signal.signal(signal.SIGTERM, previous_term)
    print(json.dumps({"suite": args.suite, "status": receipt["status"], "receipt": str(output)}))
    return 0 if receipt["status"] == "passed" else 1


if __name__ == "__main__":
    sys.exit(main())
