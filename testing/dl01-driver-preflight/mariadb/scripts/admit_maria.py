#!/usr/bin/env python3
"""Admit the 2 GiB MariaDB container under the wave resource lock. Not a Rust slot."""

import fcntl
import json
import os
import subprocess
import sys
import time

LOCK_PATH = "/tmp/oo-dl01-wave1-resources.lock"
SESSION = "0d8d559a-7e46-4859-838e-21ffccd62772"
GIB = 2 * 1024 * 1024 * 1024
MIN_HEADROOM = 10 * 1024 * 1024 * 1024
CPU_CAP = 16


def run(args: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(args, text=True, capture_output=True)


def reclaimable() -> int:
    page = int(subprocess.check_output(["pagesize"], text=True).strip())
    text = subprocess.check_output(["vm_stat"], text=True)
    wanted = {"Pages free": 0, "Pages inactive": 0, "Pages speculative": 0, "Pages purgeable": 0}
    for line in text.splitlines():
        for key in wanted:
            if line.startswith(key):
                wanted[key] = int(line.split(":", 1)[1].strip().strip("."))
    return sum(wanted.values()) * page


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


def main() -> int:
    evidence = sys.argv[1]
    command = sys.argv[sys.argv.index("--") + 1 :]
    fd = os.open(LOCK_PATH, os.O_CREAT | os.O_RDWR, 0o644)
    fcntl.flock(fd, fcntl.LOCK_EX)
    try:
        free = reclaimable()
        cpus = reserved_cpus()
        headroom = free - GIB
        body = {
            "task_id": "DL01-W1-39",
            "session_uuid": SESSION,
            "phase": "mariadb",
            "observed_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "reserved_cpus_before": cpus,
            "reserved_cpus_after": cpus + 1,
            "reclaimable_bytes": free,
            "headroom_after_reservation_bytes": headroom,
            "memory": "2g",
            "cpus": 1,
            "rust_slots": 0,
        }
        admitted = headroom >= MIN_HEADROOM and cpus + 1 <= CPU_CAP
        body["admitted"] = admitted
        os.makedirs(evidence, exist_ok=True)
        with open(os.path.join(evidence, "maria-admission.json"), "w", encoding="utf-8") as handle:
            json.dump(body, handle, indent=2)
            handle.write("\n")
        if not admitted:
            print("admission=pending-capacity")
            return 75
        print("admission=granted")
        return subprocess.run(command).returncode
    finally:
        fcntl.flock(fd, fcntl.LOCK_UN)
        os.close(fd)


if __name__ == "__main__":
    sys.exit(main())
