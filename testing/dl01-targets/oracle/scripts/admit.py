#!/usr/bin/env python3
"""Admission snapshot and owned container start under the wave fcntl lock.

Rust builders are counted. This task reserves zero Rust slots. A start is
refused when global Rust builders are already at the cap or this task already
has one.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import time

LOCK_PATH = "/tmp/oo-dl01-wave1-resources.lock"
SLOT_DIR = "/tmp/oo-dl01-wave1/slots"
SLOT_PATH = os.path.join(SLOT_DIR, "DL01-W1-10.json")
PREFIX = "oo-dl01w1-10"
TASK = "DL01-W1-10"
SESSION = "18da73a5-ad34-477a-8edf-0dc51122c1dd"
IMAGE = (
    "docker.io/gvenzl/oracle-free"
    "@sha256:cdc5b3a3685da4f1785daeee24f74de8a9768b7629192ea9abdaeeea343d12a8"
)
CONTAINER = f"{PREFIX}-oracle"
NETWORK = f"{PREFIX}-net"
VOLUME = f"{PREFIX}-oradata"


def run(cmd: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(cmd, text=True, capture_output=True)


def rust_image_prefixes() -> list[str]:
    proc = run(["docker", "images", "--no-trunc", "--format", "{{.ID}}\t{{.Repository}}"])
    prefixes: list[str] = []
    if proc.returncode != 0:
        return prefixes
    for line in proc.stdout.splitlines():
        image_id, repo = line.split("\t", 1)
        short = image_id.removeprefix("sha256:")
        if repo == "rust" or repo.endswith("/rust"):
            prefixes.append(short[:12])
    return prefixes


def docker_rust_names(prefixes: list[str]) -> list[str]:
    proc = run(["docker", "ps", "--format", "{{.Names}}\t{{.Image}}\t{{.Command}}"])
    if proc.returncode != 0:
        raise SystemExit(proc.stderr)
    names: list[str] = []
    for line in proc.stdout.splitlines():
        name, image, command = line.split("\t", 2)
        image_key = image.split("@", 1)[0]
        head = image_key.split(":")[0]
        is_rust = head == "rust" or head.endswith("/rust")
        is_rust = is_rust or any(image.startswith(prefix) for prefix in prefixes)
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
        if stripped.startswith("rustc ") or "/rustc " in f" {stripped}":
            count += 1
        elif stripped.startswith("cargo ") or "/cargo " in f" {stripped}":
            count += 1
    return count


def snapshot() -> dict:
    prefixes = rust_image_prefixes()
    docker_names = docker_rust_names(prefixes)
    host_count = host_compiler_count()
    own = [name for name in docker_names if name.startswith(PREFIX)]
    slot_rust = 0
    if os.path.isdir(SLOT_DIR):
        for entry in os.listdir(SLOT_DIR):
            path = os.path.join(SLOT_DIR, entry)
            try:
                with open(path, encoding="utf-8") as handle:
                    body = json.load(handle)
            except (OSError, json.JSONDecodeError):
                continue
            if body.get("task_id") == TASK:
                continue
            slot_rust += int(body.get("rust_slots") or 0)
    return {
        "task_id": TASK,
        "session_uuid": SESSION,
        "observed_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "docker_rust_builders": docker_names,
        "docker_rust_count": len(docker_names),
        "host_compiler_count": host_count,
        "global_rust_count": len(docker_names) + host_count,
        "own_rust_count": len(own),
        "other_reserved_rust_slots": slot_rust,
        "development_slots_named_reserved": 7,
        "review_slots_named_reserved": 3,
        "this_task_rust_slots": 0,
        "cap": 10,
        "admitted": len(docker_names) + host_count < 10 and len(own) < 1,
    }


def flock_snapshot(out_path: str | None) -> dict:
    import fcntl

    os.makedirs(os.path.dirname(LOCK_PATH), exist_ok=True)
    fd = os.open(LOCK_PATH, os.O_CREAT | os.O_RDWR, 0o644)
    try:
        fcntl.flock(fd, fcntl.LOCK_EX)
        body = snapshot()
        if out_path:
            os.makedirs(os.path.dirname(out_path), exist_ok=True)
            with open(out_path, "w", encoding="utf-8") as handle:
                json.dump(body, handle, indent=2)
                handle.write("\n")
        return body
    finally:
        fcntl.flock(fd, fcntl.LOCK_UN)
        os.close(fd)


def start(src: str, evidence: str, secrets: str) -> int:
    import fcntl

    os.makedirs(SLOT_DIR, exist_ok=True)
    fd = os.open(LOCK_PATH, os.O_CREAT | os.O_RDWR, 0o644)
    fcntl.flock(fd, fcntl.LOCK_EX)
    try:
        body = snapshot()
        snap_path = os.path.join(evidence, "raw", "admission-start.json")
        os.makedirs(os.path.dirname(snap_path), exist_ok=True)
        with open(snap_path, "w", encoding="utf-8") as handle:
            json.dump(body, handle, indent=2)
            handle.write("\n")
        if not body["admitted"]:
            pending = {
                "status": "pending-capacity",
                "reason": "global Rust builders are at the cap or this task already has a builder",
                "snapshot": body,
            }
            with open(os.path.join(evidence, "pending-capacity-receipt.json"), "w", encoding="utf-8") as handle:
                json.dump(pending, handle, indent=2)
                handle.write("\n")
            print(json.dumps(pending))
            return 3
        labels = [
            "project=open-ontology",
            "owner=dl01-w1-10",
            "session=18da73a5-ad34-477a-8edf-0dc51122c1dd",
            "kind=oracle-database-free",
            "task=DL01-W1-10",
        ]
        for name, kind in ((NETWORK, "network"), (VOLUME, "volume")):
            exists = run(["docker", kind, "inspect", name])
            if exists.returncode != 0:
                create = ["docker", kind, "create"]
                for label in labels:
                    create.extend(["--label", label])
                create.append(name)
                made = run(create)
                if made.returncode != 0:
                    sys.stderr.write(made.stderr)
                    return made.returncode
        command = [
            "docker",
            "run",
            "-d",
            "--name",
            CONTAINER,
            "--network",
            NETWORK,
            "--memory",
            "6g",
            "--memory-swap",
            "6g",
            "--cpus",
            "2",
            "--shm-size",
            "1g",
            "--pull",
            "never",
            "-e",
            "ORACLE_PASSWORD_FILE=/run/secrets/oracle-passwd",
            "-e",
            "TZ=UTC",
            "-v",
            f"{VOLUME}:/opt/oracle/oradata",
            "-v",
            f"{secrets}:/run/secrets:ro",
            "-v",
            f"{src}:/opt/oo:ro",
            "-v",
            f"{evidence}:/evidence",
        ]
        for label in labels:
            command.extend(["--label", label])
        command.append(IMAGE)
        started = run(command)
        sys.stdout.write(started.stdout)
        sys.stderr.write(started.stderr)
        if started.returncode != 0:
            return started.returncode
        reservation = {
            "task_id": TASK,
            "rust_slots": 0,
            "database_memory_gib": 6,
            "container": CONTAINER,
            "volume": VOLUME,
            "network": NETWORK,
            "image": IMAGE,
            "session_uuid": SESSION,
        }
        with open(SLOT_PATH, "w", encoding="utf-8") as handle:
            json.dump(reservation, handle, indent=2)
            handle.write("\n")
        return 0
    finally:
        fcntl.flock(fd, fcntl.LOCK_UN)
        os.close(fd)


def main() -> int:
    action = sys.argv[1]
    if action == "snapshot":
        body = flock_snapshot(sys.argv[2] if len(sys.argv) > 2 else None)
        print(json.dumps(body))
        return 0 if body["admitted"] else 3
    if action == "start":
        return start(sys.argv[2], sys.argv[3], sys.argv[4])
    sys.stderr.write("usage: admit.py snapshot [out] | start SRC EVIDENCE SECRETS\n")
    return 2


if __name__ == "__main__":
    sys.exit(main())
