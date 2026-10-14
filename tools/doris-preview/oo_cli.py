#!/usr/bin/env python3
"""Run the pinned Linux OO CLI inside the Doris gateway network namespace."""
import os
import pathlib
import sys

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
root = pathlib.Path("/tmp/oo-doris-connection-whole")
if token != root and root not in token.parents:
    raise SystemExit("unexpected token path")
args[index + 1] = "/run-current/" + str(token.relative_to(root))
os.execvp("docker", ["docker", "exec", "-u", "501:20", "oo-doris-connection-whole-gateway", "/fixed/oo", *args])
