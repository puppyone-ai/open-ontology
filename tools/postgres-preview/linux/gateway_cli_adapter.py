#!/usr/bin/env python3
"""Run the exact PG46 public OO CLI in its private Gateway network namespace."""

import pathlib
import subprocess
import sys

HOST_TOKEN = "/tmp/oo-dl01w1-46-gateway/token.private"
CONTAINER_TOKEN = "/run-pg46/token.private"
CONTAINER = "oo-dl01w1-46-gateway-preview"
CLI = "/run-pg46/oo-cli"


def main() -> int:
    if not pathlib.Path(HOST_TOKEN).is_file():
        return 2
    args = list(sys.argv[1:])
    for index, value in enumerate(args[:-1]):
        if value == "--token-file":
            if args[index + 1] != HOST_TOKEN:
                return 2
            args[index + 1] = CONTAINER_TOKEN
    result = subprocess.run(["docker", "exec", CONTAINER, CLI, *args], check=False)
    return result.returncode


if __name__ == "__main__":
    raise SystemExit(main())
