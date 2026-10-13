#!/usr/bin/env python3
"""Bounded FREEPDB1 proof and Arrow input preparation.

Does not call product.main. Does not write previewdb, drop a database, or
recreate the Oracle volume. Starts the existing container, proves the root
session fails closed, proves FREEPDB1, then stops the container again.
"""

from __future__ import annotations

import fcntl
import json
import os
import subprocess
import sys
import time
from pathlib import Path

import product

ROOT = product.ROOT
REPAIR = product.REPAIR_EVIDENCE
ORACLE = product.ORACLE_NAME
LOCK = Path("/tmp/oo-dl01-wave1-resources.lock")
NEED = 6 * 1024 * 1024 * 1024
HEADROOM_FLOOR = 10 * 1024 * 1024 * 1024
DECODE = ROOT / "evidence/992aa5d3-6848-4dd9-aff1-d4d31a099983/DL01/driver/linux/p2-compile-12/bin/oo-oracle-p2-decode"
GOLD = ROOT / "tools/source-driver-worker/oracle-child/fixtures/gold.json"
CHILD_IMAGE = "oo-dl01w1-04-p2-child:local"
NOT_GENUINE = product.EVIDENCE / "NOT-GENUINE-IMPORT.txt"


def run(args: list[str], *, stdin: bytes | None = None, timeout: int = 60) -> subprocess.CompletedProcess:
    return subprocess.run(args, input=stdin, text=False, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=timeout)


def write_not_genuine() -> None:
    product.EVIDENCE.mkdir(parents=True, exist_ok=True)
    NOT_GENUINE.write_text(
        "These product-loop files are the preserved seeded loop.\n"
        "They are not a genuine imported-source success.\n"
        "load_rows inserted catalog.GOLD.\n"
        "input_digest a6236974c0d71bfc372587e84e39d4dcf93aa994005700157854294abed47aa3 "
        "is sha256 of the literal oracle-asof-scn-2175138-rows-5.\n"
        "The live _oo.full_import_run stayed state=open, sealed_at null, admits_import false.\n"
        "DATE and RAW were stored as text. N1 was stored as int2.\n"
        "confirm_oracle printed ORA-00942 and exit 0 because the session was not in FREEPDB1.\n"
        "pg-builder binary 38c8777bd92e3ff3e1b4ca0e01b28d2e95bb82f9e9f0941e3106910368c5e341 "
        "was compiled from a mounted sealed.rs, not from commit e4f9131c6c3634e30f5f73b6b3db19ab7aa06f7d.\n"
        "Fresh, cache, unknown cost, 401, timeout, preview match, and sqlstate 57014 describe that seeded table.\n"
        "Native P2 resume-5 is a bounded component. It is not whole-source acceptance.\n"
    )


def capacity(extra: int, name: str) -> None:
    info = run(["docker", "info", "--format", "{{.NCPU}} {{.MemTotal}}"])
    parts = info.stdout.decode().split()
    if info.returncode != 0 or len(parts) < 2:
        raise SystemExit("docker info failed")
    ncpu = int(parts[0])
    mem = int(parts[1])
    reserved = 0
    ids = run(["docker", "ps", "-q"]).stdout.decode().split()
    for cid in ids:
        one = run(["docker", "inspect", cid])
        if one.returncode != 0:
            raise SystemExit(f"inspect failed {cid}")
        data = json.loads(one.stdout)[0]
        reserved += int(data["HostConfig"]["Memory"] or 0)
    headroom = mem - reserved
    REPAIR.mkdir(parents=True, exist_ok=True)
    (REPAIR / name).write_text(
        f"ncpu={ncpu} mem={mem} reserved={reserved} headroom={headroom} extra={extra}\n"
    )
    if ncpu < 16 or headroom - extra <= HEADROOM_FLOOR:
        raise SystemExit(f"capacity refused headroom={headroom} extra={extra}")


def oracle_inspect() -> dict:
    proc = run(["docker", "inspect", ORACLE])
    if proc.returncode != 0:
        raise SystemExit("oracle container is absent")
    data = json.loads(proc.stdout)[0]
    labels = (data.get("Config") or {}).get("Labels") or {}
    if labels.get("oo.owner") != "dl01w1-04" or labels.get("oo.session") != product.OWNER:
        raise SystemExit("refusing foreign oracle container")
    mounts = [mount.get("Name") for mount in data.get("Mounts") or []]
    if "oo-dl01w1-04-p2-oradata" not in mounts:
        raise SystemExit("oracle data volume is not the retained oradata volume")
    return data


def start_oracle() -> None:
    fd = os.open(LOCK, os.O_CREAT | os.O_RDWR, 0o644)
    try:
        fcntl.lockf(fd, fcntl.LOCK_EX)
        data = oracle_inspect()
        if data["State"]["Running"]:
            raise SystemExit("oracle is already running; not adopting it")
        if int(data["HostConfig"]["Memory"] or 0) != NEED:
            raise SystemExit("oracle memory limit is not 6GiB")
        capacity(NEED, "capacity-oracle.txt")
        started = run(["docker", "start", ORACLE], timeout=60)
        (REPAIR / "oracle-start.stdout").write_bytes(started.stdout)
        (REPAIR / "oracle-start.stderr").write_bytes(started.stderr)
        (REPAIR / "oracle-start.exit").write_text(f"{started.returncode}\n")
        if started.returncode != 0:
            raise SystemExit("oracle start failed")
    finally:
        os.close(fd)


def wait_oracle() -> None:
    sql = b"WHENEVER SQLERROR EXIT FAILURE\nSELECT status FROM v$instance;\nEXIT\n"
    last = None
    for _ in range(40):
        last = run(
            ["docker", "exec", "-i", ORACLE, "sqlplus", "-s", "/", "as", "sysdba"],
            stdin=sql,
            timeout=20,
        )
        text = last.stdout.decode(errors="replace")
        if last.returncode == 0 and "ORA-" not in text and "OPEN" in text:
            (REPAIR / "oracle-open.stdout").write_bytes(last.stdout)
            (REPAIR / "oracle-open.exit").write_text("0\n")
            return
        time.sleep(5)
    if last is not None:
        (REPAIR / "oracle-open.stdout").write_bytes(last.stdout)
        (REPAIR / "oracle-open.stderr").write_bytes(last.stderr)
        (REPAIR / "oracle-open.exit").write_text(f"{last.returncode}\n")
    raise SystemExit("oracle did not open")


def stop_oracle() -> None:
    stopped = run(["docker", "stop", "-t", "20", ORACLE], timeout=40)
    (REPAIR / "oracle-stop.stdout").write_bytes(stopped.stdout)
    (REPAIR / "oracle-stop.stderr").write_bytes(stopped.stderr)
    (REPAIR / "oracle-stop.exit").write_text(f"{stopped.returncode}\n")
    state = run(["docker", "inspect", "-f", "{{.State.Status}} {{.State.Running}}", ORACLE])
    (REPAIR / "oracle-after-stop.txt").write_bytes(state.stdout)
    if stopped.returncode != 0 or state.stdout.decode().strip() != "exited false":
        raise SystemExit("oracle did not return to exited")


def decode_arrow() -> None:
    if not DECODE.is_file() or not GOLD.is_file() or not product.ARROW_BATCH.is_file():
        raise SystemExit("decode inputs are absent")
    out = REPAIR / "decode"
    out.mkdir(parents=True, exist_ok=True)
    fd = os.open(LOCK, os.O_CREAT | os.O_RDWR, 0o644)
    try:
        fcntl.lockf(fd, fcntl.LOCK_EX)
        capacity(512 * 1024 * 1024, "capacity-decode.txt")
        image = run(["docker", "image", "inspect", CHILD_IMAGE, "--format", "{{.Id}} {{.Os}}/{{.Architecture}}"])
        (REPAIR / "decode-image.txt").write_bytes(image.stdout or image.stderr)
        if image.returncode != 0:
            raise SystemExit("child image is absent; not pulling")
    finally:
        os.close(fd)
    cmd = [
        "docker", "run", "--rm", "--name", "oo-dl01w1-04-p2-decode-repair",
        "--network", "none", "--platform", "linux/arm64", "--pull", "never",
        "--memory", "512m", "--cpus", "0.5",
        "--cap-drop", "ALL", "--security-opt", "no-new-privileges",
        "--user", "0:0",
        "--entrypoint", "/decode",
        "-v", f"{DECODE}:/decode:ro",
        "-v", f"{product.ARROW_BATCH.parent}:/arrow:ro",
        "-v", f"{GOLD.parent}:/gold:ro",
        "-v", f"{out}:/out",
        *product.LABELS,
        "--label", "oo.kind=oracle-decode",
        CHILD_IMAGE,
        "--arrow", "/arrow/batch.arrow",
        "--gold", "/gold/gold.json",
        "--out", "/out/decode.json",
    ]
    proc = run(cmd, timeout=90)
    (REPAIR / "decode.stdout").write_bytes(proc.stdout)
    (REPAIR / "decode.stderr").write_bytes(proc.stderr)
    (REPAIR / "decode.exit").write_text(f"{proc.returncode}\n")
    if proc.returncode != 0:
        raise SystemExit(f"decoder exit {proc.returncode}")


def current_only() -> None:
    """Second bounded start. Does not rewrite the SCN proof files."""
    started = False
    try:
        start_oracle()
        started = True
        wait_oracle()
        product.confirm_freepdb1_current()
    finally:
        if started:
            stop_oracle()
    print("DONE freepdb1-current")


def main() -> None:
    if len(sys.argv) > 1 and sys.argv[1] == "current":
        current_only()
        return
    write_not_genuine()
    try:
        product.load_rows("refused-before-oracle")
    except SystemExit as exc:
        if "missing public native Arrow physical import API" not in str(exc):
            raise
        (REPAIR / "load-rows.exit").write_text("refused\n")
    else:
        raise SystemExit("load_rows returned a table")
    held: SystemExit | None = None
    started = False
    try:
        start_oracle()
        started = True
        wait_oracle()
        product.confirm_root_ora_fails()
        product.confirm_pdb_ora_fails()
        product.confirm_oracle()
    except SystemExit as exc:
        held = exc
    finally:
        if started:
            try:
                stop_oracle()
            except SystemExit as exc:
                if held is None:
                    held = exc
    try:
        decode_arrow()
    except SystemExit as exc:
        if held is None:
            held = exc
    if held is not None:
        raise held
    print("DONE repair-oracle")


if __name__ == "__main__":
    main()
