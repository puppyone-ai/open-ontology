#!/usr/bin/env python3
"""Run the fixed Linux OO CLI inside the ClickHouse gateway container."""
import os
import pathlib
import sys

os.environ.setdefault("DOCKER_CONFIG", "/tmp/oo-ch-docker-config")
args = sys.argv[1:]
try:
    index = args.index("--token-file")
    token = pathlib.Path(args[index + 1])
except (ValueError, IndexError):
    raise SystemExit("external token-file required")
text = str(token if token.is_absolute() else pathlib.Path.cwd() / token)
if text.startswith("/private/tmp/"):
    text = "/tmp/" + text[len("/private/tmp/"):]
token = pathlib.Path(text)
root = pathlib.Path("/tmp/oo-clickhouse-connection-whole/gateway-mount.txt").read_text().strip()
base = pathlib.Path(root)
if token != base and base not in token.parents:
    raise SystemExit("unexpected token path")
args[index + 1] = "/run-current/" + str(token.relative_to(base))
os.execvp("docker", ["docker", "exec", "oo-clickhouse-connection-whole-gateway", "/fixed/oo", *args])
