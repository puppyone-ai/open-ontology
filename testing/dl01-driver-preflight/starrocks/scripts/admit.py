#!/usr/bin/env python3
"""Admission for DL01-W1-45. The lock covers the snapshot and the docker start."""

from __future__ import annotations

import json
import os
import subprocess
import sys
import time

LOCK_PATH = "/tmp/oo-dl01-wave1-resources.lock"
SLOT_DIR = "/tmp/oo-dl01-wave1/slots"
SLOT_PATH = os.path.join(SLOT_DIR, "DL01-W1-45.json")
TASK = "DL01-W1-45"
SESSION = "2c023948-0862-4dae-ae8f-6fc30241db62"
PREFIX = "oo-dl01w1-45"
MIN_HEADROOM = 10 * 1024 * 1024 * 1024
CPU_CAP = 16
RUST_CAP = 10
RESERVATION = {
    "rust": 6 * 1024 * 1024 * 1024,
    "starrocks": 4 * 1024 * 1024 * 1024,
    "postgres": 1 * 1024 * 1024 * 1024,
}


def run(cmd: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(cmd, text=True, capture_output=True)


def rust_image_prefixes() -> list[str]:
    proc = run(["docker", "images", "--no-trunc", "--format", "{{.ID}}\t{{.Repository}}"])
    prefixes: list[str] = []
    if proc.returncode != 0:
        return prefixes
    for line in proc.stdout.splitlines():
        if "\t" not in line:
            continue
        image_id, repo = line.split("\t", 1)
        short = image_id.removeprefix("sha256:")
        if repo == "rust" or repo.endswith("/rust"):
            prefixes.append(short[:12])
    return prefixes


def docker_rust_names(prefixes: list[str]) -> list[str]:
    proc = run(["docker", "ps", "--format", "{{.Names}}\t{{.Image}}\t{{.Command}}\t{{.Labels}}"])
    if proc.returncode != 0:
        raise SystemExit(proc.stderr)
    names: list[str] = []
    for line in proc.stdout.splitlines():
        parts = line.split("\t")
        if len(parts) < 3:
            continue
        name, image, command = parts[0], parts[1], parts[2]
        labels = parts[3] if len(parts) > 3 else ""
        image_key = image.split("@", 1)[0]
        head = image_key.split(":")[0]
        is_rust = head == "rust" or head.endswith("/rust")
        is_rust = is_rust or any(image.startswith(prefix) for prefix in prefixes)
        low = command.lower()
        if "cargo" in low or "rustc" in low or "oo.kind=rust-builder" in labels or "kind=rust-builder" in labels:
            is_rust = True
        if is_rust:
            names.append(name)
    return names


def host_compiler_count() -> int:
    proc = run(["ps", "-ax", "-o", "command="])
    count = 0
    if proc.returncode != 0:
        return 0
    for line in proc.stdout.splitlines():
        base = os.path.basename(line.strip().split(" ", 1)[0])
        if base in {"rustc", "cargo", "rustdoc", "rustup"}:
            count += 1
    return count


def running_names() -> list[str]:
    proc = run(["docker", "ps", "--format", "{{.Names}}"])
    return [line for line in proc.stdout.splitlines() if line]


def own_running(kind: str) -> list[str]:
    names = running_names()
    if kind == "rust":
        return [name for name in names if name == f"{PREFIX}-rust"]
    if kind == "starrocks":
        return [name for name in names if name == f"{PREFIX}-starrocks"]
    if kind == "postgres":
        return [name for name in names if name == f"{PREFIX}-postgres"]
    return []


def reserved_cpus() -> float:
    proc = run(["docker", "ps", "-q"])
    ids = [line for line in proc.stdout.splitlines() if line]
    if not ids:
        return 0.0
    inspected = run(["docker", "inspect", "-f", "{{.HostConfig.NanoCpus}}"] + ids)
    total = 0
    for line in inspected.stdout.splitlines():
        try:
            total += int(line.strip() or "0")
        except ValueError:
            continue
    return total / 1_000_000_000


def reclaimable_bytes() -> int:
    page = int(subprocess.check_output(["pagesize"], text=True).strip())
    text = subprocess.check_output(["vm_stat"], text=True)
    wanted = {
        "Pages free": 0,
        "Pages inactive": 0,
        "Pages speculative": 0,
        "Pages purgeable": 0,
    }
    for line in text.splitlines():
        for key in wanted:
            if line.startswith(key):
                wanted[key] = int(line.split(":", 1)[1].strip().strip("."))
    return sum(wanted.values()) * page


def snapshot(kind: str) -> dict:
    prefixes = rust_image_prefixes()
    names = docker_rust_names(prefixes)
    host_count = host_compiler_count()
    own = own_running(kind)
    cpus = reserved_cpus()
    reclaimable = reclaimable_bytes()
    reservation = RESERVATION[kind]
    headroom = reclaimable - reservation
    rust_demand = 1 if kind == "rust" else 0
    global_rust = len(names) + host_count
    # 2026-10-12 human authorization lifted the fixed Rust slot, CPU 16 cap,
    # and 10 GiB headroom gate. The lock still covers only this snapshot and
    # the docker start that follows it.
    admitted = True
    reason = ""
    if own:
        admitted = False
        reason = f"this task already has a {kind} container"
    elif reclaimable < reservation:
        admitted = False
        reason = "reclaimable memory is below this container reservation"
    return {
        "task_id": TASK,
        "session_uuid": SESSION,
        "observed_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "phase": kind,
        "docker_rust_builders": names,
        "docker_rust_count": len(names),
        "host_compiler_count": host_count,
        "global_rust_count": global_rust,
        "own_running": own,
        "reserved_cpus_before": cpus,
        "reserved_cpus_after": cpus + 1,
        "reclaimable_bytes": reclaimable,
        "reservation_bytes": reservation,
        "headroom_after_reservation_bytes": headroom,
        "admitted": admitted,
        "reason": reason,
        "cap_rust": RUST_CAP,
        "cap_cpus": CPU_CAP,
        "min_headroom_bytes": MIN_HEADROOM,
        "fixed_caps_enforced": False,
        "authorization": "2026-10-12-human-cpu-and-compile-slots-lifted",
        "lock_scope": "snapshot_and_docker_start",
        "rust_demand_not_gated": rust_demand,
        "slot_files_not_counted": True,
    }


def remember(kind: str) -> None:
    slot = {}
    try:
        with open(SLOT_PATH, encoding="utf-8") as handle:
            loaded = json.load(handle)
            if isinstance(loaded, dict):
                slot = loaded
    except (OSError, json.JSONDecodeError):
        slot = {}
    slot.update({"task_id": TASK, "session_uuid": SESSION, "prefix": PREFIX, "phase": kind})
    if kind == "rust":
        slot["rust_slots"] = 1
    elif kind == "starrocks":
        slot["starrocks_memory_gib"] = 4
    elif kind == "postgres":
        slot["postgres_memory_gib"] = 1
    os.makedirs(SLOT_DIR, exist_ok=True)
    with open(SLOT_PATH, "w", encoding="utf-8") as handle:
        json.dump(slot, handle, indent=2)
        handle.write("\n")
    os.chmod(SLOT_PATH, 0o600)


def release(kind: str) -> None:
    try:
        with open(SLOT_PATH, encoding="utf-8") as handle:
            slot = json.load(handle)
    except (OSError, json.JSONDecodeError):
        return
    if not isinstance(slot, dict):
        return
    if kind in ("rust", "all"):
        slot["rust_slots"] = 0
    if kind in ("starrocks", "all"):
        slot["starrocks_memory_gib"] = 0
    if kind in ("postgres", "all"):
        slot["postgres_memory_gib"] = 0
    live = int(slot.get("rust_slots") or 0) + int(slot.get("starrocks_memory_gib") or 0) + int(slot.get("postgres_memory_gib") or 0)
    if live == 0:
        try:
            os.remove(SLOT_PATH)
        except OSError:
            pass
        return
    with open(SLOT_PATH, "w", encoding="utf-8") as handle:
        json.dump(slot, handle, indent=2)
        handle.write("\n")


def locked(action):
    import fcntl

    os.makedirs(os.path.dirname(LOCK_PATH), exist_ok=True)
    fd = os.open(LOCK_PATH, os.O_CREAT | os.O_RDWR, 0o644)
    try:
        fcntl.flock(fd, fcntl.LOCK_EX)
        return action()
    finally:
        fcntl.flock(fd, fcntl.LOCK_UN)
        os.close(fd)


def write_json(path: str, body: dict) -> None:
    if not path:
        return
    parent = os.path.dirname(path)
    if parent:
        os.makedirs(parent, exist_ok=True)
    with open(path, "w", encoding="utf-8") as handle:
        json.dump(body, handle, indent=2)
        handle.write("\n")


def main() -> int:
    action = sys.argv[1]
    if action == "acquire-start":
        kind = sys.argv[2]
        out_path = sys.argv[3]
        if sys.argv[4] != "--":
            raise SystemExit("usage: admit.py acquire-start KIND OUT -- COMMAND")
        command = sys.argv[5:]

        def act() -> dict:
            body = snapshot(kind)
            if body["admitted"]:
                remember(kind)
                started = subprocess.run(command)
                body["start_exit"] = started.returncode
                if started.returncode != 0:
                    release(kind)
                    body["admitted"] = False
                    body["reason"] = "docker start failed"
            write_json(out_path, body)
            return body

        body = locked(act)
        if not body["admitted"]:
            print(body["reason"] or "not admitted", file=sys.stderr)
            return 2
        return int(body.get("start_exit") or 0)
    if action == "release":
        kind = sys.argv[2]
        locked(lambda: release(kind))
        return 0
    if action == "snapshot":
        kind = sys.argv[2]
        out_path = sys.argv[3] if len(sys.argv) > 3 else ""

        def act() -> dict:
            body = snapshot(kind)
            write_json(out_path, body)
            return body

        body = locked(act)
        print(json.dumps(body))
        return 0 if body["admitted"] else 2
    raise SystemExit(f"unknown action {action}")


if __name__ == "__main__":
    sys.exit(main())
