#!/usr/bin/env python3
"""Admit this task under the wave resource lock, then start one OpenSearch container."""

from __future__ import annotations

import fcntl
import json
import os
import subprocess
import time
from pathlib import Path

LOCK = Path("/tmp/oo-dl01-wave1-resources.lock")
EVIDENCE = Path(
    "/Users/redux/puppy-one/open-ontology/evidence/b53c75ff-62f4-4869-8428-a605d263cb43/DL01/opensearch"
)
PRIVATE = Path("/tmp/oo-dl01w1-15-private")
IMAGE = os.environ.get(
    "OO_OS_IMAGE",
    "opensearchproject/opensearch@sha256:cf05462a958be93fe2172aea2c6757debc7d3a1e4a6838d39e56af591a3dcdbb",
)
NAME = "oo-dl01w1-15-opensearch"
VOLUME = "oo-dl01w1-15-data"
RUST_PREFIXES = (
    "sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5",
    "sha256:218765d808d35e2e3fa1b12b2e1314e2bfdd427a3530de071398553a95743cba",
    "sha256:f31fa9eaac4e417505e10009786ec0c636558e02dff3fa662cc92c10894ba065",
    "sha256:f747a9f2b58b8c5f9d6d0680fe8af2edba26ac258a7d6142bb4f65f9693711f4",
)


def sh(cmd: list[str], check: bool = True) -> subprocess.CompletedProcess[str]:
    proc = subprocess.run(cmd, text=True, capture_output=True)
    if check and proc.returncode != 0:
        raise RuntimeError(f"{cmd[0]} exited {proc.returncode}: {proc.stderr[-500:]}")
    return proc


def snapshot() -> dict:
    host = sh(["ps", "-ax", "-o", "pid=,comm="], check=False)
    host_rust = []
    for line in host.stdout.splitlines():
        parts = line.split(None, 1)
        if len(parts) == 2 and parts[1] in {"rustc", "cargo", "rustdoc"}:
            host_rust.append({"pid": parts[0], "comm": parts[1]})
    containers = sh(
        ["docker", "ps", "--format", "{{.ID}}\t{{.Names}}\t{{.Image}}\t{{.Status}}"],
        check=False,
    )
    rust_running = []
    own = []
    for line in containers.stdout.splitlines():
        cid, name, image, status = (line.split("\t") + ["", "", "", ""])[:4]
        inspect = sh(
            ["docker", "inspect", "-f", "{{.Image}}", cid],
            check=False,
        )
        image_id = inspect.stdout.strip()
        is_rust = image_id.startswith(RUST_PREFIXES) or image.startswith("rust")
        row = {"id": cid, "name": name, "image": image, "image_id": image_id, "status": status}
        if is_rust:
            rust_running.append(row)
        if name.startswith("oo-dl01w1-15"):
            own.append(row)
    return {
        "host_rust": host_rust,
        "rust_running": rust_running,
        "own_containers": own,
        "global_rust": len(host_rust) + len(rust_running),
        "own_rust": len(host_rust) + len([row for row in rust_running if row["name"].startswith("oo-dl01w1-15")]),
    }


def main() -> None:
    raw = EVIDENCE / "raw" / "admit"
    raw.mkdir(parents=True, exist_ok=True)
    LOCK.touch()
    with LOCK.open("a+") as lock:
        fcntl.flock(lock.fileno(), fcntl.LOCK_EX)
        fact = snapshot()
        fact["dev_slots_named"] = 7
        fact["review_slots_reserved_by_coordinator"] = 3
        fact["rust_slots_requested"] = 0
        fact["database_memory_limit"] = "1792m"
        admitted = fact["global_rust"] < 10 and fact["own_rust"] < 1
        fact["admitted"] = admitted
        (raw / "admission.json").write_text(json.dumps(fact, indent=2) + "\n", encoding="utf-8")
        if not admitted:
            pending = {
                "status": "pending-capacity",
                "reason": "global rust >= 10 or own rust >= 1",
                "snapshot": fact,
            }
            (EVIDENCE / "pending-capacity.json").write_text(
                json.dumps(pending, indent=2) + "\n", encoding="utf-8"
            )
            (raw / "admit.exit").write_text("3\n", encoding="utf-8")
            raise SystemExit(3)
        existing = sh(["docker", "ps", "-a", "--filter", f"name=^{NAME}$", "--format", "{{.ID}} {{.Names}}"], check=False)
        if existing.stdout.strip():
            labels = sh(
                ["docker", "inspect", "-f", "{{index .Config.Labels \"session\"}}", NAME],
                check=False,
            )
            if labels.stdout.strip() != "b53c75ff-62f4-4869-8428-a605d263cb43":
                (raw / "admit.exit").write_text("4\n", encoding="utf-8")
                raise SystemExit(4)
            sh(["docker", "rm", "-f", NAME])
        sh(["docker", "volume", "rm", "-f", VOLUME], check=False)
        cmd = [
            "docker", "run", "-d",
            "--name", NAME,
            "--memory", "1792m",
            "--memory-swap", "1792m",
            "--cpus", "1",
            "--ulimit", "nofile=65536:65536",
            "-p", "127.0.0.1:19215:9200",
            "-v", f"{PRIVATE / 'opensearch.yml'}:/usr/share/opensearch/config/opensearch.yml:ro",
            "-v", f"{PRIVATE / 'certs'}:/usr/share/opensearch/config/certs:ro",
            "-v", f"{PRIVATE / 'security'}:/usr/share/opensearch/config/opensearch-security:ro",
            "-v", f"{VOLUME}:/usr/share/opensearch/data",
            "-e", "discovery.type=single-node",
            "-e", "DISABLE_INSTALL_DEMO_CONFIG=true",
            "-e", "DISABLE_PERFORMANCE_ANALYZER_AGENT_CLI=true",
            "-e", "OPENSEARCH_JAVA_OPTS=-Xms512m -Xmx512m",
            "--label", "project=open-ontology",
            "--label", "owner=dl01-w1-15",
            "--label", "session=b53c75ff-62f4-4869-8428-a605d263cb43",
            "--label", "kind=target-preparation",
            "--label", "task=DL01-W1-15",
            IMAGE,
        ]
        started = time.time()
        proc = sh(cmd)
        (raw / "docker-run.stdout").write_text(proc.stdout, encoding="utf-8")
        (raw / "docker-run.stderr").write_text(proc.stderr, encoding="utf-8")
        fact["container_started_at"] = started
        fact["container_id"] = proc.stdout.strip()
        (raw / "admission.json").write_text(json.dumps(fact, indent=2) + "\n", encoding="utf-8")
        (raw / "admit.exit").write_text("0\n", encoding="utf-8")
        fcntl.flock(lock.fileno(), fcntl.LOCK_UN)
    print(proc.stdout.strip())


if __name__ == "__main__":
    main()
