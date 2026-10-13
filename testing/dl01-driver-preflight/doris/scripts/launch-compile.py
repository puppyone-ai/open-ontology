#!/usr/bin/env python3
"""Admit one Linux 1.94 rust builder under the wave lock, then start it."""

import fcntl
import json
import os
import select
import subprocess
import sys

LOCK = "/tmp/oo-dl01-wave1-resources.lock"
ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
REPO = os.path.abspath(os.path.join(ROOT, "..", "..", ".."))
EVIDENCE = os.path.join(
    REPO,
    "evidence/5106fed0-5bcd-4aaf-8df7-838c95a7d7e2/DL01/doris-driver-preflight/compile-6",
)
BASELINE = os.path.join(
    REPO,
    "evidence/5106fed0-5bcd-4aaf-8df7-838c95a7d7e2/DL01/doris-driver-preflight/pre-edit/attempt-3",
)
IMAGE = "docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
NAME = "oo-dl01w1-43-rust"
OWNER = "DL01-W1-43"
SESSION = "5106fed0-5bcd-4aaf-8df7-838c95a7d7e2"
NEW_CPU = 1.0
NEW_MEM = 6 * 1024 ** 3


def run(cmd):
    return subprocess.run(cmd, text=True, capture_output=True)


def write(name, text):
    os.makedirs(EVIDENCE, exist_ok=True)
    path = os.path.join(EVIDENCE, name)
    with open(path, "w", encoding="utf-8") as handle:
        handle.write(text)
    return path


def measure():
    listed = run(["docker", "ps", "-q"])
    ids = [line for line in listed.stdout.split() if line]
    cpu = 0.0
    mem = 0
    unlimited = 0
    for container in ids:
        inspected = run(
            ["docker", "inspect", "-f", "{{.HostConfig.NanoCpus}} {{.HostConfig.Memory}}", container]
        )
        nano, memory = inspected.stdout.split()
        nano = int(nano)
        memory = int(memory)
        cpu += 1.0 if nano == 0 else nano / 1_000_000_000
        if memory == 0:
            unlimited += 1
        mem += memory
    builders = run(["docker", "ps", "-q", "--filter", "label=oo.kind=rust-builder"]).stdout.split()
    own = run(
        [
            "docker",
            "ps",
            "-q",
            "--filter",
            f"label=oo.owner={OWNER}",
            "--filter",
            "label=oo.kind=rust-builder",
        ]
    ).stdout.split()
    host = run(["ps", "-ax", "-o", "pid=,command="])
    host_rust = []
    for line in host.stdout.splitlines():
        bits = line.strip().split(None, 1)
        if len(bits) < 2:
            continue
        base = os.path.basename(bits[1].split()[0])
        if base in {"rustc", "cargo", "rustdoc"}:
            host_rust.append(line.strip())
    info = run(["docker", "info", "-f", "{{.MemTotal}}"])
    total = int(info.stdout.strip() or "0")
    headroom = total - mem - NEW_MEM
    return {
        "running": len(ids),
        "cpu": cpu,
        "cpu_after": cpu + NEW_CPU,
        "mem_bytes": mem,
        "mem_unlimited": unlimited,
        "mem_total": total,
        "headroom_bytes": headroom,
        "global_rust_builders": len(builders),
        "own_rust_builders": len(own),
        "host_rust_toolchains": len(host_rust),
        "global_rust": len(builders) + len(host_rust),
        "cap_cpu": 16,
        "cap_rust": 10,
        "cap_own_rust": 1,
        "headroom_required": 10 * 1024 ** 3,
    }


def admit():
    os.makedirs(EVIDENCE, exist_ok=True)
    lock = open(LOCK, "a")
    fcntl.flock(lock.fileno(), fcntl.LOCK_EX)
    try:
        snap = measure()
        write("capacity.json", json.dumps(snap, indent=2) + "\n")
        blocked = (
            snap["own_rust_builders"] >= 1
            or snap["global_rust"] >= 10
            or snap["cpu_after"] > 16
            or snap["headroom_bytes"] <= 10 * 1024 ** 3
        )
        if blocked:
            write("pending-capacity.json", json.dumps({"status": "pending-capacity", **snap}, indent=2) + "\n")
            write("admit.exit", "3\n")
            return 3
        for old in ("oo-dl01w1-43-pre-edit", NAME):
            previous = run(["docker", "inspect", "-f", "{{.State.Status}} {{.Config.Labels}}", old])
            write(f"prior-{old}.txt", previous.stdout + previous.stderr)
            if previous.returncode == 0 and previous.stdout.startswith("exited") and OWNER in previous.stdout:
                removed = run(["docker", "rm", old])
                write(f"prior-{old}-rm.exit", str(removed.returncode) + "\n")
                write(f"prior-{old}-rm.stderr", removed.stderr)
        exists = run(["docker", "container", "inspect", NAME])
        if exists.returncode == 0:
            write("already-exists.txt", NAME + "\n")
            write("admit.exit", "2\n")
            return 2
        command = [
            "docker", "run", "-d", "--name", NAME,
            "--platform", "linux/arm64", "--pull", "never", "--init", "--user", "0:0",
            "--security-opt", "no-new-privileges", "--cap-drop", "ALL",
            "--memory", "6g", "--memory-swap", "6g", "--cpus", "1", "--pids-limit", "4096",
            "--network", "oo-dl01w1-43-build",
            "-e", "RUSTUP_TOOLCHAIN=1.94.0",
            "-e", "CARGO_BUILD_JOBS=1",
            "-e", "GIT_TERMINAL_PROMPT=0",
            "--label", "oo.project=open-ontology",
            "--label", f"oo.owner={OWNER}",
            "--label", f"oo.session={SESSION}",
            "--label", "oo.prefix=oo-dl01w1-43",
            "--label", f"oo.task={OWNER}",
            "--label", "oo.toolchain=1.94.0",
            "--label", "oo.kind=rust-builder",
            "--mount", f"type=bind,source={ROOT},target=/src",
            "--mount", f"type=bind,source={EVIDENCE},target=/evidence",
            "--mount", f"type=bind,source={BASELINE},target=/baseline,readonly",
            "--mount", "type=volume,source=oo-dl01w1-43-cargo-registry,target=/usr/local/cargo/registry",
            "--entrypoint", "bash",
            IMAGE, "/src/scripts/build-inside.sh",
        ]
        write("invocation.txt", " ".join(command) + "\n")
        started = run(command)
        write("docker-run.stdout", started.stdout)
        write("docker-run.stderr", started.stderr)
        write("docker-run.exit", str(started.returncode) + "\n")
        write("admit.exit", "0\n" if started.returncode == 0 else "1\n")
        return 0 if started.returncode == 0 else 1
    finally:
        fcntl.flock(lock.fileno(), fcntl.LOCK_UN)


def wait_event():
    proc = subprocess.Popen(
        ["docker", "events", "--filter", "type=container", "--format", "{{.Action}}"],
        stdout=subprocess.PIPE,
        text=True,
    )
    ready, _, _ = select.select([proc.stdout], [], [], 900)
    if ready:
        line = proc.stdout.readline().strip()
        write("capacity-event.txt", line + "\n")
    else:
        write("capacity-event.txt", "900s without a container event\n")
    proc.kill()
    proc.wait()


def main():
    while True:
        code = admit()
        if code != 3:
            print(f"admit {code}")
            return code
        print("pending-capacity; waiting one docker event or 900s")
        wait_event()


if __name__ == "__main__":
    sys.exit(main())
