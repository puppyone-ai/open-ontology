#!/usr/bin/env python3
"""Doris 3.x C4/C5 capture cycle for the owned FE.

This does not rebuild the pipeline or the reader ELF sealed in the preview closure.
It does not call launch-closure and it does not delete volumes.
"""

import hashlib
import json
import os
import subprocess
import time
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
REPO = ROOT.parents[2]
EVIDENCE = REPO / "evidence/5106fed0-5bcd-4aaf-8df7-838c95a7d7e2/DL01/doris-product/capture-live"
SECRETS = Path("/tmp/oo-dl01w1-43-secrets")
LOCK = "/tmp/oo-dl01-wave1-resources.lock"
IMAGE = "docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
NETWORK = "oo-dl01w1-43-net"
FE = "oo-dl01w1-43-fe"
BE = "oo-dl01w1-43-be"
BUILDER = "oo-dl01w1-43-capture-rust"
FE_IP = "172.16.43.10"
PORT = "9030"
OWNER = "DL01-W1-43"


def run(cmd, timeout=180):
    return subprocess.run(cmd, text=True, capture_output=True, timeout=timeout)


def write(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def owned(name):
    inspected = run(["docker", "inspect", "-f", "{{index .Config.Labels \"oo.owner\"}}", name])
    return inspected.returncode == 0 and inspected.stdout.strip() == OWNER


def capacity(extra_cpu, extra_mem):
    ids = [line for line in run(["docker", "ps", "-q"]).stdout.split() if line]
    cpu = 0.0
    mem = 0
    for container in ids:
        nano, memory = run(
            ["docker", "inspect", "-f", "{{.HostConfig.NanoCpus}} {{.HostConfig.Memory}}", container]
        ).stdout.split()
        nano = int(nano)
        memory = int(memory)
        cpu += 1.0 if nano == 0 else nano / 1_000_000_000
        mem += memory
    builders = run(["docker", "ps", "-q", "--filter", "label=oo.kind=rust-builder"]).stdout.split()
    host_rust = 0
    listed = run(["ps", "-ax", "-o", "command="])
    for line in listed.stdout.splitlines():
        parts = line.strip().split()
        base = os.path.basename(parts[0]) if parts else ""
        if base in {"rustc", "cargo", "rustdoc"}:
            host_rust += 1
    total = int(run(["docker", "info", "-f", "{{.MemTotal}}"]).stdout.strip() or "0")
    return {
        "cpu_after": cpu + extra_cpu,
        "global_rust": len(builders) + host_rust,
        "headroom_bytes": total - mem - extra_mem,
        "own_rust_builders": len(
            run([
                "docker", "ps", "-q",
                "--filter", "label=oo.owner=DL01-W1-43",
                "--filter", "label=oo.kind=rust-builder",
            ]).stdout.split()
        ),
    }


def capacity_ok(snap):
    return (
        snap["own_rust_builders"] == 0
        and snap["global_rust"] < 10
        and snap["cpu_after"] <= 16
        and snap["headroom_bytes"] > 10 * 1024 ** 3
    )


def admit(name, kind, extra_cpu, extra_mem, command):
    import fcntl

    evidence = EVIDENCE / "capacity"
    started_at = time.time() + 1800
    while True:
        lock = open(LOCK, "a")
        fcntl.flock(lock.fileno(), fcntl.LOCK_EX)
        try:
            snap = capacity(extra_cpu, extra_mem)
            snap["kind"] = kind
            write(evidence / f"{name}.json", json.dumps(snap, indent=2) + "\n")
            if capacity_ok(snap):
                started = run(command, timeout=180)
                write(evidence / f"{name}.stdout", started.stdout)
                write(evidence / f"{name}.stderr", started.stderr)
                write(evidence / f"{name}.exit", f"{started.returncode}\n")
                return started.returncode
            if time.time() > started_at:
                write(evidence / f"{name}.blocked", "capacity\n")
                return 1
        finally:
            fcntl.flock(lock.fileno(), fcntl.LOCK_UN)
            lock.close()
        time.sleep(5)


def build_reader():
    binary = EVIDENCE / "build" / "doris-reader"
    log = EVIDENCE / "build" / "build.log"
    if os.environ.get("OO_CAPTURE_REBUILD") != "1" and binary.is_file() and log.is_file() and "CAPTURE_BUILD_OK" in log.read_text(encoding="utf-8"):
        write(EVIDENCE / "build" / "rebuild.skip", "existing CAPTURE_BUILD_OK\n")
        return 0
    if owned(BUILDER):
        state = run(["docker", "inspect", "-f", "{{.State.Status}}", BUILDER]).stdout.strip()
        if state == "running":
            return 1
        run(["docker", "rm", "-f", BUILDER])
    script = """#!/bin/bash
set -eu
export PATH="/usr/local/cargo/bin:${PATH}"
{
  rustc --version | tee /out/rustc.txt
  grep -q 'rustc 1.94.0' /out/rustc.txt
  if ! rustup component list --installed | grep -q '^clippy'; then
    rustup component add clippy
  fi
  cd /src
  cargo test --locked --offline --jobs 1 -p doris-reader -- --test-threads=1
  cargo clippy --locked --offline --jobs 1 -p doris-reader --all-targets -- -D warnings
  cargo build --locked --offline --jobs 1 -p doris-reader --bin doris-reader
  cp /target/doris-reader-capture/debug/doris-reader /out/doris-reader
  sha256sum /out/doris-reader | tee /out/doris-reader.sha256
  echo CAPTURE_BUILD_OK
} > /out/build.log 2>&1
"""
    (EVIDENCE / "build").mkdir(parents=True, exist_ok=True)
    script_path = EVIDENCE / "build.sh"
    script_path.write_text(script, encoding="utf-8")
    script_path.chmod(0o755)
    command = [
        "docker", "run", "-d", "--name", BUILDER,
        "--platform", "linux/arm64", "--pull", "never", "--init",
        "--user", "0:0",
        "--security-opt", "no-new-privileges", "--cap-drop", "ALL",
        "--memory", "6g", "--memory-swap", "6g", "--cpus", "1",
        "--pids-limit", "256",
        "--label", "oo.project=open-ontology",
        "--label", f"oo.owner={OWNER}",
        "--label", "oo.kind=rust-builder",
        "--label", "oo.task=DL01-W1-43",
        "--mount", f"type=bind,source={ROOT},target=/src,readonly",
        "--mount", f"type=bind,source={EVIDENCE / 'build'},target=/out",
        "--mount", f"type=bind,source={script_path},target=/build.sh,readonly",
        "--mount", "type=volume,source=oo-dl01w1-43-cargo-registry,target=/usr/local/cargo/registry",
        "--mount", "type=volume,source=oo-dl01w1-43-closure-target,target=/target",
        "-e", "CARGO_TARGET_DIR=/target/doris-reader-capture",
        "-e", "CARGO_NET_OFFLINE=true",
        "--entrypoint", "bash",
        IMAGE, "/build.sh",
    ]
    started = 1
    for _ in range(8):
        marker = EVIDENCE / "build" / ".mount"
        marker.write_text("ok\n", encoding="utf-8")
        started = admit("capture-build", "rust-builder", 1.0, 6 * 1024 ** 3, command)
        if started == 0:
            break
        err_path = EVIDENCE / "capacity" / "capture-build.stderr"
        err_text = err_path.read_text(encoding="utf-8") if err_path.is_file() else ""
        if "does not exist" not in err_text:
            return started
        time.sleep(2)
    if started != 0:
        return started
    waited = run(["docker", "wait", BUILDER], timeout=7200)
    write(EVIDENCE / "build" / "wait.exit", waited.stdout)
    logs = run(["docker", "logs", BUILDER], timeout=180)
    write(EVIDENCE / "build" / "docker.stdout", logs.stdout)
    write(EVIDENCE / "build" / "docker.stderr", logs.stderr)
    write(EVIDENCE / "build" / "build.exit", waited.stdout if waited.stdout.endswith("\n") else waited.stdout + "\n")
    run(["docker", "rm", "-f", BUILDER])
    return int(waited.stdout.strip() or "1")


def start_source():
    started = []
    for name in (FE, BE):
        if not owned(name):
            write(EVIDENCE / f"{name}.owner", "not-owned\n")
            return 1
        state = run(["docker", "inspect", "-f", "{{.State.Status}}", name]).stdout.strip()
        write(EVIDENCE / f"{name}.before", state + "\n")
        if state != "running":
            code = admit(f"start-{name}", "source", 0.45, 1536 * 1024 ** 2 if name == FE else 2304 * 1024 ** 2, ["docker", "start", name])
            if code != 0:
                return code
            started.append(name)
    deadline = time.time() + 180
    while time.time() < deadline:
        probe = run([
            "docker", "exec", "oo-dl01w1-43-runtime", "bash", "-lc",
            f"timeout 2 bash -c 'cat < /dev/null > /dev/tcp/{FE_IP}/{PORT}'",
        ], timeout=20)
        if probe.returncode == 0:
            write(EVIDENCE / "source-port.exit", "0\n")
            return wait_backend()
        time.sleep(3)
    write(EVIDENCE / "source-port.exit", "1\n")
    return 1


def wait_backend():
    deadline = time.time() + 600
    last = 1
    while time.time() < deadline:
        last = tick(1000, "boot")
        err_path = EVIDENCE / "raw" / "boot.stderr"
        err_text = err_path.read_text(encoding="utf-8") if err_path.is_file() else ""
        if last == 0:
            write(EVIDENCE / "backend-ready.exit", "0\n")
            return 0
        write(EVIDENCE / "backend-ready.stderr", err_text)
        time.sleep(5)
    write(EVIDENCE / "backend-ready.exit", f"{last}\n")
    return last


def run_capture(kind_args, name):
    command = [
        "docker", "run", "-d", "--name", f"oo-dl01w1-43-{name}",
        "--network", NETWORK, "--pull", "never",
        "--user", "0:0",
        "--memory", "512m", "--cpus", "0.25",
        "--label", f"oo.owner={OWNER}",
        "--label", "oo.kind=capture-runner",
        "--label", "oo.task=DL01-W1-43",
        "--mount", f"type=bind,source={EVIDENCE / 'build' / 'doris-reader'},target=/opt/doris-reader,readonly",
        "--mount", f"type=bind,source={EVIDENCE},target=/evidence",
        "--mount", f"type=bind,source={SECRETS},target=/secrets,readonly",
        "--mount", f"type=bind,source={ROOT / 'sql'},target=/sql,readonly",
        "--entrypoint", "/opt/doris-reader",
        IMAGE, *kind_args,
    ]
    if owned(f"oo-dl01w1-43-{name}"):
        run(["docker", "rm", "-f", f"oo-dl01w1-43-{name}"])
    started = admit(name, "capture-runner", 0.25, 512 * 1024 ** 2, command)
    if started != 0:
        return started
    waited = run(["docker", "wait", f"oo-dl01w1-43-{name}"], timeout=180)
    logs = run(["docker", "logs", f"oo-dl01w1-43-{name}"])
    write(EVIDENCE / "raw" / f"{name}.stdout", logs.stdout)
    write(EVIDENCE / "raw" / f"{name}.stderr", logs.stderr)
    write(EVIDENCE / "raw" / f"{name}.exit", waited.stdout if waited.stdout.endswith("\n") else waited.stdout + "\n")
    run(["docker", "rm", "-f", f"oo-dl01w1-43-{name}"])
    return int(waited.stdout.strip() or "1")


def tick(every, label):
    return run_capture(
        ["capture-tick", "/evidence", "/secrets", FE_IP, PORT, str(every), label],
        label,
    )


def dba(file_name, name):
    return run_capture(
        ["capture-dba", "/evidence", "/secrets", FE_IP, PORT, f"/sql/{file_name}"],
        name,
    )


def sha256_tree(directory):
    lines = []
    for path in sorted(directory.rglob("*")):
        if path.is_file() and path.name != "SHA256SUMS":
            digest = hashlib.sha256(path.read_bytes()).hexdigest()
            lines.append(f"{digest}  {path.relative_to(directory).as_posix()}")
    write(directory / "SHA256SUMS", "\n".join(lines) + ("\n" if lines else ""))


TABLES = ("dup_events", "agg_qty", "uniq_acct", "dec_exact", "part_day", "str_key", "nz_obs")
WRITES = {"INSERT", "UPDATE", "DELETE", "ALTER", "DROP", "CREATE", "TRUNCATE", "GRANT"}


def load_json(name):
    return json.loads((EVIDENCE / name).read_text(encoding="utf-8"))


def table_rows(doc, name):
    for item in doc["tables"]:
        if item["name"] == name:
            return item["rows"]
    raise KeyError(name)


def multiset_diff(before, after):
    left = Counter(before)
    right = Counter(after)
    removed = []
    added = []
    for line, count in left.items():
        removed.extend([line] * max(0, count - right.get(line, 0)))
    for line, count in right.items():
        added.extend([line] * max(0, count - left.get(line, 0)))
    removed.sort()
    added.sort()
    return removed, added


def versions(doc):
    found = {}
    for row in doc.get("partitions") or []:
        if not isinstance(row, dict):
            continue
        lowered = {str(key).lower(): value for key, value in row.items()}
        name = lowered.get("partitionname")
        if name is None:
            continue
        found[str(name)] = None if lowered.get("visibleversion") is None else str(lowered.get("visibleversion"))
    return found


def redact(text):
    if not SECRETS.is_dir():
        return text
    for path in SECRETS.iterdir():
        if not path.is_file():
            continue
        secret = path.read_text(encoding="utf-8").strip()
        if len(secret) >= 8:
            text = text.replace(secret, "[redacted]")
    return text


def fe_audit_bytes():
    probed = run([
        "docker", "exec", FE, "bash", "-lc",
        "wc -c /opt/apache-doris/fe/log/fe.audit.log",
    ], timeout=30)
    token = probed.stdout.strip().split()
    if probed.returncode != 0 or not token or not token[0].isdigit():
        write(EVIDENCE / "fe-audit-offset.stderr", probed.stderr)
        return None
    return int(token[0])


def audit_field(line, key):
    needle = key + "="
    for part in line.split("|"):
        if part.startswith(needle):
            return part[len(needle):]
    return ""


def audit_stats(text):
    counts = {}
    reader_writes = []
    for line in text.splitlines():
        if "User=" not in line or "Stmt=" not in line:
            continue
        user = audit_field(line, "User") or "unknown"
        stmt = line.split("Stmt=", 1)[-1].strip()
        head = stmt.split()[0].upper() if stmt else ""
        bucket = counts.setdefault(user, {})
        bucket[head] = bucket.get(head, 0) + 1
        if user == "oo_reader" and head in WRITES:
            reader_writes.append(head)
    return {"by_user_statement": counts, "oo_reader_write_heads": reader_writes}


def collect_fe_audit(start):
    if start is None:
        write(EVIDENCE / "fe-audit.exit", "1\n")
        return {"collected": False, "oo_reader_write_heads": ["offset_missing"]}
    raw = run([
        "docker", "exec", FE, "bash", "-lc",
        f"tail -c +{start + 1} /opt/apache-doris/fe/log/fe.audit.log",
    ], timeout=60)
    text = redact(raw.stdout)
    write(EVIDENCE / "fe.audit.cycle.redacted.log", text)
    write(EVIDENCE / "fe-audit.exit", f"{raw.returncode}\n")
    stats = audit_stats(text)
    stats["collected"] = raw.returncode == 0
    stats["start_offset"] = start
    stats["bytes"] = len(raw.stdout.encode("utf-8"))
    write(EVIDENCE / "fe-audit-stats.json", json.dumps(stats, indent=2, sort_keys=True) + "\n")
    return stats


def verify_cycle():
    problems = []
    diffs = {}
    ticks = {}
    for label in ("a1", "a2", "b1", "b2", "b3"):
        path = EVIDENCE / f"{label}.json"
        if not path.is_file():
            problems.append(f"missing {label}.json")
            continue
        ticks[label] = load_json(f"{label}.json")
        if ticks[label].get("product_reader_write_attempts") != 0:
            problems.append(f"{label} reader writes")
        if ticks[label].get("source_acceptance") is not False:
            problems.append(f"{label} source_acceptance")
        if "doris-3." not in str(ticks[label].get("backend_version", "")) and "3.1." not in str(ticks[label].get("backend_version", "")):
            problems.append(f"{label} backend version")
        for name in TABLES:
            arrow = EVIDENCE / f"{label}-{name}.arrow"
            if not arrow.is_file() or arrow.stat().st_size < 16:
                problems.append(f"missing arrow {label}-{name}")
    for label in ("a2early", "b2early"):
        if (EVIDENCE / f"{label}.json").exists():
            problems.append(f"{label} wrote a snapshot")
    period_ms = {}
    trusted = False
    if set(ticks) >= {"a1", "a2", "b1", "b2", "b3"}:
        for name in TABLES:
            change = {}
            for left, right in (("a1", "a2"), ("a2", "b1"), ("b1", "b2"), ("b2", "b3")):
                removed, added = multiset_diff(table_rows(ticks[left], name), table_rows(ticks[right], name))
                change[f"{left}_to_{right}"] = {"removed": removed, "added": added}
            diffs[name] = change
        for name in ("dup_events", "agg_qty", "dec_exact", "str_key"):
            for edge in diffs[name].values():
                if edge["removed"] or edge["added"]:
                    problems.append(f"{name} changed")
        part = diffs["part_day"]["a1_to_a2"]
        if part["removed"] or len(part["added"]) != 1 or "2026-03-20" not in part["added"][0] or "east" not in part["added"][0]:
            problems.append("part_day insert")
        if diffs["part_day"]["a2_to_b1"]["removed"] or diffs["part_day"]["a2_to_b1"]["added"]:
            problems.append("part_day drifted before the second period")
        if diffs["part_day"]["b1_to_b2"]["removed"] or diffs["part_day"]["b1_to_b2"]["added"]:
            problems.append("part_day changed in cycle 2")
        if diffs["part_day"]["b2_to_b3"]["removed"] or diffs["part_day"]["b2_to_b3"]["added"]:
            problems.append("part_day replay")
        nz = diffs["nz_obs"]["a1_to_a2"]
        if nz["added"] or len(nz["removed"]) != 1 or not nz["removed"][0].startswith("3\u001f"):
            problems.append("nz_obs delete")
        for edge_name in ("a2_to_b1", "b1_to_b2", "b2_to_b3"):
            edge = diffs["nz_obs"][edge_name]
            if edge["removed"] or edge["added"]:
                problems.append(f"nz_obs resurrected or drifted {edge_name}")
            if any(line.startswith("3\u001f") for line in table_rows(ticks["b3"], "nz_obs")):
                problems.append("nz_obs id 3 still present")
        uniq = diffs["uniq_acct"]["a1_to_a2"]
        if not any(line.startswith("5\u001f") for line in uniq["added"]):
            problems.append("uniq insert")
        if not any("cycle" in line and "cycle2" not in line for line in uniq["added"]):
            problems.append("uniq update")
        if not uniq["removed"]:
            problems.append("uniq old row")
        cycle2 = diffs["uniq_acct"]["b1_to_b2"]
        if not any("cycle2" in line for line in cycle2["added"]):
            problems.append("uniq cycle2")
        if diffs["uniq_acct"]["b2_to_b3"]["removed"] or diffs["uniq_acct"]["b2_to_b3"]["added"]:
            problems.append("uniq replay")
        if diffs["uniq_acct"]["a2_to_b1"]["removed"] or diffs["uniq_acct"]["a2_to_b1"]["added"]:
            problems.append("uniq drifted before the second period")
        for later, earlier, period in (("a2", "a1", 3000), ("b2", "b1", 7000), ("b3", "b2", 7000)):
            delta = int(ticks[later]["finished_unix_ms"]) - int(ticks[earlier]["finished_unix_ms"])
            period_ms[f"{earlier}_to_{later}"] = delta
            if delta < period:
                problems.append(f"period {earlier} to {later} was {delta}")
        before = versions(ticks["a1"])
        after = versions(ticks["a2"])
        trusted = bool(before) and any(
            before.get(name) != after.get(name) and "202603" in name for name in before
        )
        if not trusted:
            problems.append("partition visible version did not move; C4 metadata is untrusted")
    for path in sorted(EVIDENCE.glob("reader-audit-*.jsonl")):
        for line in path.read_text(encoding="utf-8").splitlines():
            if not line.strip():
                continue
            item = json.loads(line)
            sql = item.get("sql") or ""
            head = sql.split()[0].upper() if sql.split() else ""
            if item.get("user") == "oo_reader" and head in WRITES:
                problems.append(f"client audit write {path.name}")
    body = {
        "c4_metadata_trusted": trusted,
        "diffs": diffs,
        "partition_versions_a1": versions(ticks["a1"]) if "a1" in ticks else {},
        "partition_versions_a2": versions(ticks["a2"]) if "a2" in ticks else {},
        "period_ms": period_ms,
        "problems": problems,
        "route_applied_part_day": "C4" if trusted else "C5_metadata_untrusted",
    }
    if "a1" in ticks and "a2" in ticks:
        body["partition_version_movement"] = {
            name: [versions(ticks["a1"]).get(name), versions(ticks["a2"]).get(name)]
            for name in sorted(set(versions(ticks["a1"])) | set(versions(ticks["a2"])))
        }
    write(EVIDENCE / "diff-check.json", json.dumps(body, indent=2, sort_keys=True) + "\n")
    return body


def write_reports(build_exit, cycle, checked=None):
    if checked is None:
        checked = {}
    build_log = EVIDENCE / "build" / "build.log"
    log_text = build_log.read_text(encoding="utf-8") if build_log.is_file() else ""
    problems = checked.get("problems", [])
    route_applied = checked.get("route_applied_part_day", "not_checked")
    ran = "a1" in cycle
    cycle_fact = (
        "oo_reader issued no DML. oo_dba ran the two fixture scripts in separate audits. "
        "The sealed generation-1 PG snapshot and fixtures/oracle.json were not rewritten. "
        "Tick Arrow files are stream IPC from this reader. The shared Loader did not apply them.\n\n"
        if ran
        else "The live cycle did not run. No DBA statement was sent and no tick Arrow was written.\n\n"
    )
    report = (
        "# DL01-W1-43 Doris capture cycle\n\n"
        "Preview closure commits 66c7d5bd24bdcf894976da9508cc963356079e06 and "
        "84bae33fd281e1bd29660b0f3de0bb6361fadda0 stay in place. "
        "This packet does not replace that preview and does not sign the full source.\n\n"
        f"Reader build exit: {build_exit}\n\n"
        f"Cycle exits: {json.dumps(cycle, sort_keys=True)}\n\n"
        f"part_day route applied from partition VisibleVersion: {route_applied}. "
        "The declared route is C4 only when the touched partition version moves. "
        "The other six approved tables are explicit C5 with DELETES_LAGGED. "
        "Doris 3.x has no published row binlog, so C1 was refused before any read. "
        "Each table read is S0. There is no cross-table snapshot token, so the set is B. "
        "There is no cursor, so equal-cursor, long-transaction late commit, and position expiry are not applicable.\n\n"
        f"{cycle_fact}"
        f"Diff problems: {json.dumps(problems, ensure_ascii=False)}\n\n"
        "source_acceptance remains false. Full source sign-off remains 0/10 for root. "
        "Historical preview stays 6/10 and is not this sign-off.\n"
    )
    findings = (
        "# Findings\n\n"
        "The owned reader now refuses an unpublished binlog, classifies part_day as C4, and classifies "
        "the other approved tables as explicit C5. A tick with the same every_ms returns exit 3 until the "
        "configured period has elapsed. Two periods, 3000ms and 7000ms, are the user configs for this lab.\n\n"
        "Platform jobs lease, Loader Batch apply, Self-build S0–S10, and an external Agent session are not "
        "implemented by this source. The requests are in HANDOFF.md. Production DBA approval, owner approval, "
        "and EULA stay pending. The lab account oo_dba is the fixture identity this task already created.\n\n"
        f"Build log contains CAPTURE_BUILD_OK: {'CAPTURE_BUILD_OK' in log_text}. "
        f"Build log contains Compiling: {'Compiling ' in log_text}. "
        f"part_day route applied: {route_applied}. "
        f"Diff problem count: {len(problems)}.\n"
    )
    handoff = (
        "# Handoff\n\n"
        "Shared owners, not this source, still have to implement the following. "
        "Inputs are the tick JSON files in this directory. Do not treat this packet as those interfaces.\n\n"
        "1. Loader / pg-builder, currently waiting on 05. Input: one capture tick JSON. "
        "C4 part_day must replace one partition in a single Agent DB transaction. "
        "C5 tables must write a shadow diff and tombstones with generation and fence. "
        "Output: an apply receipt. Error today: this source has no loader CLI and did not call pg-builder. "
        "Test: after apply, an independent PG count and full cell compare match the tick rows, including the "
        "physical delete of nz_obs id 3, which must stay deleted.\n\n"
        "2. Platform scheduler, SP04 jobs lease. Input: two configs, every 3000ms and every 7000ms, "
        "with non-overlap. Output: lease receipts and a change visible on the later period. "
        "This packet's ticks are the source driver, not that lease.\n\n"
        "3. Self-build control 13ad15b4. Input: the sealed catalog and this capture tick. "
        "Output: S0–S10 stage records and strict OOS YAML. Known gap from the 18 review: cancel, 401, "
        "timeout, and noReady. This source did not hand-write YAML.\n\n"
        "4. External Agent. Gateway interface fixed at ac896 only. Input: the role pack manifest, a hidden "
        "column, a cancel, and a question after the capture change. Output: query_id audit matching an "
        "independent expected value. This source did not send a model dialogue and did not query PG as the agent.\n\n"
        "Preview ELF hashes in the closure-run were not replaced. FE and BE are stopped again after the cycle "
        "when this script started them. Volumes were not removed.\n"
    )
    write(EVIDENCE / "REPORT.md", report)
    write(EVIDENCE / "FINDINGS.md", findings)
    write(EVIDENCE / "HANDOFF.md", handoff)


def main():
    EVIDENCE.mkdir(parents=True, exist_ok=True)
    if not (SECRETS / "reader.password").is_file() or not (SECRETS / "dba.password").is_file():
        write(EVIDENCE / "secrets.exit", "1\n")
        write_reports(1, {"secrets": "missing"})
        sha256_tree(EVIDENCE)
        return 1
    build_exit = build_reader()
    if build_exit != 0:
        write_reports(build_exit, {"build": build_exit})
        sha256_tree(EVIDENCE)
        return build_exit
    source = start_source()
    cycle = {"source": source}
    if source != 0:
        write_reports(build_exit, cycle)
        sha256_tree(EVIDENCE)
        return source
    offset = fe_audit_bytes()
    write(EVIDENCE / "fe-audit-offset.txt", f"{offset}\n")
    try:
        cycle["a1"] = tick(3000, "a1")
        cycle["a2_early"] = tick(3000, "a2early")
        cycle["dba1"] = dba("capture-cycle.sql", "dba1")
        time.sleep(3.1)
        cycle["a2"] = tick(3000, "a2")
        cycle["b1"] = tick(7000, "b1")
        cycle["b2_early"] = tick(7000, "b2early")
        cycle["dba2"] = dba("capture-cycle-2.sql", "dba2")
        time.sleep(7.1)
        cycle["b2"] = tick(7000, "b2")
        if cycle["b2"] == 0:
            time.sleep(7.1)
            cycle["b3"] = tick(7000, "b3")
        else:
            cycle["b3"] = "skipped"
        cycle["fe_audit"] = collect_fe_audit(offset)
    except Exception as exc:
        cycle["error"] = f"{type(exc).__name__}: {exc}"[:500]
    finally:
        for name in (FE, BE):
            if owned(name):
                stopped = run(["docker", "stop", name], timeout=120)
                write(EVIDENCE / f"{name}.stop.exit", f"{stopped.returncode}\n")
    write(EVIDENCE / "cycle.json", json.dumps(cycle, indent=2, sort_keys=True) + "\n")
    checked = verify_cycle()
    reader_writes = cycle.get("fe_audit", {}).get("oo_reader_write_heads") or []
    if reader_writes:
        checked["problems"].append("fe audit oo_reader writes")
        checked["fe_reader_writes"] = reader_writes
        write(EVIDENCE / "diff-check.json", json.dumps(checked, indent=2, sort_keys=True) + "\n")
    write_reports(build_exit, cycle, checked)
    sha256_tree(EVIDENCE)
    needed = ("a1", "a2_early", "dba1", "a2", "b1", "b2_early", "dba2", "b2", "b3")
    if any(cycle.get(name) not in (0, 3) for name in needed):
        return 1
    if cycle.get("a2_early") != 3 or cycle.get("b2_early") != 3:
        return 1
    if any(cycle.get(name) != 0 for name in ("a1", "dba1", "a2", "b1", "dba2", "b2", "b3")):
        return 1
    if checked.get("problems"):
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
