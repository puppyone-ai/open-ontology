#!/usr/bin/env python3
"""Read-only host gate for E-S5b; never pulls, starts or stops containers."""
import json
import math
import re
import shutil
import subprocess

MIN_FREE_BYTES = 25 * 1024**3
MIN_MEMORY_BYTES = 6 * 1024**3


def memory_bytes(value):
    match = re.fullmatch(r"(\d+(?:\.\d+)?)\s*(B|KiB|MiB|GiB|TiB)", value)
    if not match:
        raise ValueError("unrecognized Docker memory unit")
    return math.ceil(float(match[1]) * 1024 ** ["B", "KiB", "MiB", "GiB", "TiB"].index(match[2]))


def assess(free_bytes, lane_running_ids, memory_headroom_bytes):
    blockers = []
    if free_bytes < MIN_FREE_BYTES:
        blockers.append("disk_below_25_gib")
    if memory_headroom_bytes < MIN_MEMORY_BYTES:
        blockers.append("docker_memory_headroom_below_6_gib")
    if lane_running_ids:
        blockers.append("lane_e_container_already_running")
    return {
        "schema": "oo.db2.host-preflight/v1",
        "free_bytes": free_bytes,
        "minimum_free_bytes": MIN_FREE_BYTES,
        "lane_running_container_count": len(lane_running_ids),
        "docker_memory_headroom_bytes": memory_headroom_bytes,
        "minimum_memory_headroom_bytes": MIN_MEMORY_BYTES,
        "memory_basis": "Docker MemTotal minus container stats usage; not VM MemAvailable",
        "blockers": blockers,
        "ready": not blockers,
        "product_suites": {"T0": "not_run", "T1": "not_run", "T8": "not_run"},
    }


def docker(*args):
    return subprocess.run(
        ["docker", *args], capture_output=True, text=True, check=True, timeout=30,
    ).stdout


def main():
    try:
        lane_ids = docker("ps", "--quiet", "--no-trunc", "--filter", "label=oo.lane=e")
        lane_ids += "\n" + docker("ps", "--quiet", "--no-trunc", "--filter", "label=oo.lane=E")
        total = int(docker("info", "--format", "{{.MemTotal}}").strip())
        used = sum(memory_bytes(line.split(" / ")[0]) for line in
                   docker("stats", "--no-stream", "--format", "{{.MemUsage}}").splitlines())
        if total <= 0 or used > total:
            raise ValueError("invalid Docker memory accounting")
        report = assess(shutil.disk_usage("/").free, lane_ids.split(), total - used)
    except (OSError, ValueError, subprocess.SubprocessError):
        # Do not expose daemon endpoints or arbitrary command diagnostics.
        print(json.dumps({"ready": False, "blockers": ["host_probe_failed"]}))
        return 2
    print(json.dumps(report, indent=2))
    return 0 if report["ready"] else 2


if __name__ == "__main__":
    raise SystemExit(main())
