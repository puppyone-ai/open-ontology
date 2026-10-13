#!/usr/bin/env python3
"""Admit or release the W44 slot under the wave resource lock."""

from __future__ import annotations

import json
import os
import subprocess
import sys
import time

LOCK_PATH = "/tmp/oo-dl01-wave1-resources.lock"
SLOT_DIR = "/tmp/oo-dl01-wave1/slots"
SLOT_PATH = os.path.join(SLOT_DIR, "DL01-W1-44.json")
TASK = "DL01-W1-44"
SESSION = "bc6cf794-2d14-4e98-bcb6-f7a9b3533a88"
PREFIX = "oo-dl01w1-44"
CPU_CAP = 16
RUST_CAP = 10
MIN_HEADROOM = 10 * 1024 * 1024 * 1024


def run(cmd: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(cmd, text=True, capture_output=True)


def rust_image_prefixes() -> list[str]:
    proc = run(["docker", "images", "--no-trunc", "--format", "{{.ID}}\t{{.Repository}}"])
    prefixes: list[str] = []
    if proc.returncode != 0:
        return prefixes
    for line in proc.stdout.splitlines():
        image_id, repo = line.split("\t", 1)
        if repo == "rust" or repo.endswith("/rust"):
            prefixes.append(image_id.removeprefix("sha256:")[:12])
    return prefixes


def docker_rust_names(prefixes: list[str]) -> list[str]:
    proc = run(["docker", "ps", "--format", "{{.Names}}\t{{.Image}}\t{{.Command}}"])
    if proc.returncode != 0:
        raise SystemExit(proc.stderr)
    names: list[str] = []
    for line in proc.stdout.splitlines():
        name, image, command = line.split("\t", 2)
        head = image.split("@", 1)[0].split(":")[0]
        is_rust = head == "rust" or head.endswith("/rust") or any(image.startswith(prefix) for prefix in prefixes)
        low = command.lower()
        if "cargo" in low or "rustc" in low:
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
        if stripped.startswith("rustc ") or stripped.startswith("cargo "):
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
    wanted = {"Pages free": 0, "Pages inactive": 0, "Pages speculative": 0, "Pages purgeable": 0}
    for line in text.splitlines():
        for key in wanted:
            if line.startswith(key):
                wanted[key] = int(line.split(":", 1)[1].strip().strip("."))
    return sum(wanted.values()) * page


def snapshot(kind: str) -> dict:
    names = docker_rust_names(rust_image_prefixes())
    host_count = host_compiler_count()
    own = [name for name in names if name.startswith(PREFIX)]
    slot = own_slot()
    own_rust_slots = int(slot.get("rust_slots") or 0)
    cpus = reserved_cpus()
    reclaimable = reclaimable_bytes()
    reservation = 6 * 1024 * 1024 * 1024 if kind == "rust" else 2 * 1024 * 1024 * 1024
    if kind == "pg":
        reservation = 1 * 1024 * 1024 * 1024
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
    elif kind == "rust" and (global_rust + rust_demand > RUST_CAP or own_rust_slots >= 1 or len(own) >= 1):
        admitted = False
        reason = "rust builder cap is full or this task already reserved one"
    elif kind == "pg" and int(slot.get("pg_slots") or 0) >= 1:
        admitted = False
        reason = "this task already reserved one local postgres"
    return {
        "task_id": TASK,
        "session_uuid": SESSION,
        "observed_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "phase": kind,
        "docker_rust_builders": names,
        "host_compiler_count": host_count,
        "other_reserved_rust_slots": other_rust_slots(),
        "global_rust_count": global_rust,
        "own_rust_count": len(own),
        "own_rust_slots": own_rust_slots,
        "reserved_cpus_before": cpus,
        "reserved_cpus_after": cpus + 1,
        "reclaimable_bytes": reclaimable,
        "headroom_after_reservation_bytes": headroom,
        "admitted": admitted,
        "reason": reason,
    }


def write_slot(kind: str) -> None:
    os.makedirs(SLOT_DIR, exist_ok=True)
    slot = own_slot()
    slot.update({"task_id": TASK, "session_uuid": SESSION, "prefix": PREFIX})
    if kind == "rust":
        slot["rust_slots"] = 1
        slot["phase"] = "rust"
    elif kind == "source":
        slot["source_reserved"] = True
        slot["phase"] = "source"
    elif kind == "pg":
        slot["pg_slots"] = 1
        slot["phase"] = "pg"
    else:
        raise SystemExit(f"unknown kind {kind}")
    slot.setdefault("rust_slots", 0)
    slot.setdefault("pg_slots", 0)
    with open(SLOT_PATH, "w", encoding="utf-8") as handle:
        json.dump(slot, handle, indent=2)
        handle.write("\n")


def release(kind: str) -> None:
    slot = own_slot()
    if not slot:
        return
    if kind in ("rust", "all"):
        slot["rust_slots"] = 0
    if kind in ("source", "all"):
        slot["source_reserved"] = False
    if kind in ("pg", "all"):
        slot["pg_slots"] = 0
    if int(slot.get("rust_slots") or 0) == 0 and int(slot.get("pg_slots") or 0) == 0 and not slot.get("source_reserved"):
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

    fd = os.open(LOCK_PATH, os.O_CREAT | os.O_RDWR, 0o644)
    try:
        fcntl.flock(fd, fcntl.LOCK_EX)
        return action()
    finally:
        fcntl.flock(fd, fcntl.LOCK_UN)
        os.close(fd)


def main() -> int:
    action = sys.argv[1]
    if action == "acquire":
        kind = sys.argv[2]
        out_path = sys.argv[3] if len(sys.argv) > 3 else ""

        def act() -> dict:
            body = snapshot(kind)
            if body["admitted"]:
                write_slot(kind)
            if out_path:
                os.makedirs(os.path.dirname(out_path), exist_ok=True)
                with open(out_path, "w", encoding="utf-8") as handle:
                    json.dump(body, handle, indent=2)
                    handle.write("\n")
            return body

        body = locked(act)
        print(json.dumps(body))
        return 0 if body["admitted"] else 3
    if action == "release":
        kind = sys.argv[2]
        print(json.dumps(locked(lambda: release(kind) or {"released": kind})))
        return 0
    raise SystemExit(f"unknown action {action}")


if __name__ == "__main__":
    raise SystemExit(main())
