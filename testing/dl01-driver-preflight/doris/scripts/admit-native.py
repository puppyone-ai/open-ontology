#!/usr/bin/env python3
"""Admit the capped Doris FE and BE under the wave lock. No Rust builder."""

import fcntl
import json
import os
import select
import subprocess
import sys

LOCK = "/tmp/oo-dl01-wave1-resources.lock"
ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
REPO = os.path.abspath(os.path.join(ROOT, "..", "..", ".."))
EVIDENCE = os.environ.get(
    "OO_DORIS_ADMIT_EVIDENCE",
    os.path.join(
        REPO,
        "evidence/5106fed0-5bcd-4aaf-8df7-838c95a7d7e2/DL01/doris-driver-preflight/native-1/admit",
    ),
)
BOOT = os.environ.get("OO_DORIS_BOOT_DIR", os.path.join(ROOT, "scripts"))
FE_REF = "docker.io/apache/doris:fe-3.1.4"
BE_REF = "docker.io/apache/doris:be-3.1.4"
FE_MEM = 1536 * 1024 * 1024
BE_MEM = 2304 * 1024 * 1024
NEW_CPU = 0.90
NEW_MEM = FE_MEM + BE_MEM


def run(cmd):
    return subprocess.run(cmd, text=True, capture_output=True)


def write(name, text):
    os.makedirs(EVIDENCE, exist_ok=True)
    with open(os.path.join(EVIDENCE, name), "w", encoding="utf-8") as handle:
        handle.write(text)


def measure():
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
    for line in run(["ps", "-ax", "-o", "command="]).stdout.splitlines():
        base = os.path.basename(line.strip().split()[0]) if line.strip() else ""
        if base in {"rustc", "cargo", "rustdoc"}:
            host_rust += 1
    total = int(run(["docker", "info", "-f", "{{.MemTotal}}"]).stdout.strip() or "0")
    return {
        "cpu_after": cpu + NEW_CPU,
        "headroom_bytes": total - mem - NEW_MEM,
        "global_rust": len(builders) + host_rust,
        "own_rust_builders": len(
            run(
                [
                    "docker", "ps", "-q",
                    "--filter", "label=oo.owner=DL01-W1-43",
                    "--filter", "label=oo.kind=rust-builder",
                ]
            ).stdout.split()
        ),
    }


def labels(kind):
    return [
        "--label", "oo.project=open-ontology",
        "--label", "oo.owner=DL01-W1-43",
        "--label", "oo.session=5106fed0-5bcd-4aaf-8df7-838c95a7d7e2",
        "--label", "oo.prefix=oo-dl01w1-43",
        "--label", "oo.task=DL01-W1-43",
        "--label", f"oo.kind={kind}",
    ]


def admit():
    lock = open(LOCK, "a")
    fcntl.flock(lock.fileno(), fcntl.LOCK_EX)
    try:
        snap = measure()
        write("capacity.json", json.dumps(snap, indent=2) + "\n")
        if snap["own_rust_builders"] or snap["global_rust"] >= 10 or snap["cpu_after"] > 16 or snap["headroom_bytes"] <= 10 * 1024 ** 3:
            write("pending-capacity.json", json.dumps({"status": "pending-capacity", **snap}, indent=2) + "\n")
            write("admit.exit", "3\n")
            return 3
        for image, label in ((FE_REF, "fe"), (BE_REF, "be")):
            inspected = run(["docker", "image", "inspect", "-f", "{{.Os}}\t{{.Architecture}}", image])
            write(f"{label}-image.txt", inspected.stdout + inspected.stderr)
            if inspected.returncode != 0 or inspected.stdout.strip() != "linux\tarm64":
                write("admit.exit", "4\n")
                return 4
        for name in ("oo-dl01w1-43-fe", "oo-dl01w1-43-be"):
            if run(["docker", "container", "inspect", name]).returncode == 0:
                write("already-exists.txt", name + "\n")
                write("admit.exit", "2\n")
                return 2
        if run(["docker", "network", "inspect", "oo-dl01w1-43-net"]).returncode != 0:
            created = run([
                "docker", "network", "create",
                "--subnet", "172.16.43.0/24", "--gateway", "172.16.43.1",
                *labels("network"),
                "oo-dl01w1-43-net",
            ])
            write("network-create.exit", str(created.returncode) + "\n")
            write("network-create.stderr", created.stderr)
            if created.returncode != 0:
                return created.returncode or 1
        fe_boot = os.path.join(BOOT, "fe-boot.sh")
        be_boot = os.path.join(BOOT, "be-boot.sh")
        fe_cmd = [
            "docker", "run", "-d", "--name", "oo-dl01w1-43-fe",
            "--platform", "linux/arm64", "--pull", "never",
            "--network", "oo-dl01w1-43-net", "--ip", "172.16.43.10",
            "--memory", "1536m", "--memory-swap", "1536m", "--cpus", "0.45",
            "--ulimit", "nofile=655350:655350",
            *labels("doris-fe"),
            "-e", "FE_SERVERS=fe1:172.16.43.10:9010",
            "-e", "FE_ID=1",
            "-e", "FE_HEAP=768m",
            "--mount", f"type=bind,source={fe_boot},target=/oo-fe-boot.sh,readonly",
            "--entrypoint", "bash",
            FE_REF, "/oo-fe-boot.sh",
        ]
        be_cmd = [
            "docker", "run", "-d", "--name", "oo-dl01w1-43-be",
            "--platform", "linux/arm64", "--pull", "never",
            "--network", "oo-dl01w1-43-net", "--ip", "172.16.43.20",
            "--memory", "2304m", "--memory-swap", "2304m", "--cpus", "0.45",
            "--ulimit", "nofile=655350:655350",
            *labels("doris-be"),
            "-e", "FE_SERVERS=fe1:172.16.43.10:9010",
            "-e", "BE_ADDR=172.16.43.20:9050",
            "-e", "FE_IP=172.16.43.10",
            "-e", "BE_IP=172.16.43.20",
            "-e", "BE_HEARTBEAT_PORT=9050",
            "-e", "QUERY_PORT=9030",
            "-e", "BE_JVM_HEAP=256m",
            "-e", "BE_MEM_LIMIT=1536M",
            "--mount", f"type=bind,source={be_boot},target=/oo-be-boot.sh,readonly",
            "--entrypoint", "bash",
            BE_REF, "/oo-be-boot.sh",
        ]
        write("fe-invocation.txt", " ".join(fe_cmd) + "\n")
        write("be-invocation.txt", " ".join(be_cmd) + "\n")
        fe = run(fe_cmd)
        write("fe-run.exit", str(fe.returncode) + "\n")
        write("fe-run.stderr", fe.stderr)
        if fe.returncode != 0:
            return fe.returncode or 1
        be = run(be_cmd)
        write("be-run.exit", str(be.returncode) + "\n")
        write("be-run.stderr", be.stderr)
        if be.returncode != 0:
            return be.returncode or 1
        write("admit.exit", "0\n")
        return 0
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
        write("capacity-event.txt", proc.stdout.readline())
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
