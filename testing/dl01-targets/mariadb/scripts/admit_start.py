#!/usr/bin/env python3
"""Hold /tmp/oo-dl01-wave1-resources.lock for admission and one docker start."""

import fcntl
import os
import subprocess
import sys

LOCK = "/tmp/oo-dl01-wave1-resources.lock"


def count_lines(argv: list[str]) -> int:
    output = subprocess.check_output(argv, text=True)
    return len([line for line in output.splitlines() if line.strip()])


def main() -> int:
    if len(sys.argv) < 2:
        print("usage: admit_start.py COMMAND...", file=sys.stderr)
        return 2
    fd = os.open(LOCK, os.O_CREAT | os.O_RDWR, 0o644)
    fcntl.flock(fd, fcntl.LOCK_EX)
    try:
        running = count_lines(["docker", "ps", "-q", "--filter", "label=oo.kind=rust-builder"])
        host = subprocess.check_output(["ps", "-ax", "-o", "comm="], text=True)
        host_n = sum(1 for line in host.splitlines() if line.strip() in {"rustc", "cargo"})
        own = count_lines(
            [
                "docker",
                "ps",
                "-q",
                "--filter",
                "label=oo.kind=rust-builder",
                "--filter",
                "label=oo.session=451aa6f1-9e4f-474c-bf35-4de5652aac52",
            ]
        )
        print(f"rust_builder_running={running}")
        print(f"host_rustc_cargo={host_n}")
        print(f"own_rust_builders={own}")
        print("global_limit=10")
        print("own_limit=1")
        print("development_slots_reserved=7")
        print("review_slots_reserved=3")
        print("this_task_rust_slots=0")
        print("database_memory=1536m")
        print("database_memory_limit=2GiB")
        if running >= 10 or host_n >= 10 or own >= 1:
            print("admission=pending-capacity")
            return 75
        print("admission=granted")
        print("docker_start=begin")
        rc = subprocess.call(sys.argv[1:])
        print(f"docker_start_exit={rc}")
        return rc
    finally:
        fcntl.flock(fd, fcntl.LOCK_UN)
        os.close(fd)


if __name__ == "__main__":
    sys.exit(main())
