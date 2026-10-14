#!/usr/bin/env python3
"""Run the current PG46 public OO CLI inside the continuous Gateway namespace."""

import pathlib
import subprocess
import sys

HOST_TOKEN = "/Users/redux/.cache/oo-postgresql-connection-whole/gateway-token"
CONTAINER_TOKEN = "/run-pg46/token-current.private"
CONTAINER = "oo-pg46-continuous-gateway"
CLI = "/run-pg46/oo-cli"


def main() -> int:
    token = pathlib.Path(HOST_TOKEN)
    if not token.is_file() or token.stat().st_mode & 0o077:
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
