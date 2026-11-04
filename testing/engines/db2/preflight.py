#!/usr/bin/env python3
"""Read-only host gate for E-S5; never pulls, starts or stops containers."""
import json
import shutil
import subprocess


MIN_FREE_BYTES = 25 * 1024**3


def assess(free_bytes, running_ids):
    blockers = []
    if free_bytes < MIN_FREE_BYTES:
        blockers.append("disk_below_25_gib")
    # Fail closed: image names cannot reliably identify every database service.
    if running_ids:
        blockers.append("db2_run_alone_requires_idle_docker")
    return {
        "schema": "oo.db2.host-preflight/v1",
        "free_bytes": free_bytes,
        "minimum_free_bytes": MIN_FREE_BYTES,
        "running_container_count": len(running_ids),
        "blockers": blockers,
        "ready": not blockers,
        "product_suites": {"T0": "not_run", "T1": "not_run", "T8": "not_run"},
    }


def main():
    try:
        result = subprocess.run(
            ["docker", "ps", "--quiet", "--no-trunc"],
            capture_output=True, text=True, check=True, timeout=30,
        )
        report = assess(shutil.disk_usage("/").free, result.stdout.split())
    except (OSError, subprocess.SubprocessError):
        # Do not expose daemon endpoints or arbitrary command diagnostics.
        print(json.dumps({"ready": False, "blockers": ["host_probe_failed"]}))
        return 2
    print(json.dumps(report, indent=2))
    return 0 if report["ready"] else 2


if __name__ == "__main__":
    raise SystemExit(main())
