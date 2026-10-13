#!/usr/bin/env python3
"""Admission for DL01-W1-40. The lock covers the snapshot and the docker start."""

from __future__ import annotations

import json
import os
import subprocess
import sys
import time

LOCK_PATH = "/tmp/oo-dl01-wave1-resources.lock"
SLOT_DIR = "/tmp/oo-dl01-wave1/slots"
SLOT_PATH = os.path.join(SLOT_DIR, "DL01-W1-40.json")
TASK = "DL01-W1-40"
SESSION = "4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de"
PREFIX = "oo-dl01w1-40"
MIN_HEADROOM = 10 * 1024 * 1024 * 1024
CPU_CAP = 16
RUST_CAP = 10
RESERVATION = {
    "rust": 6 * 1024 * 1024 * 1024,
    "clickhouse": 2 * 1024 * 1024 * 1024,
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
    proc = run(
        [
            "docker",
            "ps",
            "--format",
            "{{.Names}}\t{{.Image}}\t{{.Command}}\t{{.Labels}}",
        ]
    )
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
        if "cargo" in low or "rustc" in low or "oo.kind=rust-builder" in labels:
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
        stripped = line.strip()
        base = os.path.basename(stripped.split(" ", 1)[0])
        if base in {"rustc", "cargo", "rustdoc", "rustup"}:
            count += 1
    return count


def other_rust_slots() -> int:
    total = 0
    if not os.path.isdir(SLOT_DIR):
        return 0
    for entry in os.listdir(SLOT_DIR):
        path = os.path.join(SLOT_DIR, entry)
        try:
            with open(path, encoding="utf-8") as handle:
                body = json.load(handle)
        except (OSError, json.JSONDecodeError):
            continue
        if body.get("task_id") == TASK:
            continue
        total += int(body.get("rust_slots") or 0)
    return total


def own_slot() -> dict:
    try:
        with open(SLOT_PATH, encoding="utf-8") as handle:
            body = json.load(handle)
    except (OSError, json.JSONDecodeError):
        return {}
    return body if isinstance(body, dict) else {}


def write_slot(slot: dict) -> None:
    os.makedirs(SLOT_DIR, exist_ok=True)
    with open(SLOT_PATH, "w", encoding="utf-8") as handle:
        json.dump(slot, handle, indent=2)
        handle.write("\n")
    os.chmod(SLOT_PATH, 0o600)


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
    own = [name for name in names if name.startswith(PREFIX)]
    slot = own_slot()
    own_rust_slots = int(slot.get("rust_slots") or 0)
    cpus = reserved_cpus()
    reclaimable = reclaimable_bytes()
    reservation = RESERVATION[kind]
    headroom = reclaimable - reservation
    rust_demand = 1 if kind == "rust" else 0
    global_rust = len(names) + host_count + other_rust_slots()
    admitted = True
    reason = ""
    if headroom < MIN_HEADROOM:
        admitted = False
        reason = "host memory headroom after reservation is below 10 GiB"
    elif cpus + 1 > CPU_CAP:
        admitted = False
        reason = "reserved CPUs would exceed 16"
    elif kind == "rust" and (global_rust + rust_demand > RUST_CAP or len(own) >= 1):
        admitted = False
        reason = "rust builder cap is full or this task already has a rust container"
    elif kind == "clickhouse" and any(name.startswith(PREFIX) and "clickhouse" in name for name in run(
        ["docker", "ps", "-a", "--format", "{{.Names}}"]
    ).stdout.splitlines()):
        admitted = False
        reason = "this task already has a clickhouse container"
    elif kind == "postgres" and any(
        name.startswith(PREFIX) and name.endswith("-pg")
        for name in run(["docker", "ps", "-a", "--format", "{{.Names}}"]).stdout.splitlines()
    ):
        admitted = False
        reason = "this task already has a postgres container"
    return {
        "task_id": TASK,
        "session_uuid": SESSION,
        "observed_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "phase": kind,
        "docker_rust_builders": names,
        "docker_rust_count": len(names),
        "host_compiler_count": host_count,
        "other_reserved_rust_slots": other_rust_slots(),
        "global_rust_count": global_rust,
        "own_rust_count": len(own),
        "own_rust_slots": own_rust_slots,
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
    }


def remember(kind: str) -> None:
    slot = own_slot()
    slot.update(
        {
            "task_id": TASK,
            "session_uuid": SESSION,
            "prefix": PREFIX,
        }
    )
    if kind == "rust":
        slot["rust_slots"] = 1
    elif kind == "clickhouse":
        slot["database_memory_gib"] = 2
    elif kind == "postgres":
        slot["database_memory_gib"] = 1
    else:
        raise SystemExit(f"unknown kind {kind}")
    slot.setdefault("rust_slots", 0)
    slot.setdefault("database_memory_gib", 0)
    slot["phase"] = kind
    write_slot(slot)


def release(kind: str) -> None:
    slot = own_slot()
    if not slot:
        return
    if kind in ("rust", "all"):
        slot["rust_slots"] = 0
    if kind in ("clickhouse", "postgres", "all"):
        slot["database_memory_gib"] = 0
    if int(slot.get("rust_slots") or 0) == 0 and int(slot.get("database_memory_gib") or 0) == 0:
        try:
            os.remove(SLOT_PATH)
        except OSError:
            pass
        return
    write_slot(slot)


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

        def act() -> dict:
            release(kind)
            return {"released": kind}

        locked(act)
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
