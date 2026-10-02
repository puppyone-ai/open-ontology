#!/usr/bin/env python3
"""Speak one P5 admit frame to a built worker and print the response JSON.

This is a fixture helper. It does not apply the parent deadline or reap path.
The Rust tests cover that path. SQL is an argument, never read from the environment.
"""

import json
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

MAGIC = b"P05P"
PROTOCOL_VERSION = 1
KIND_ADMIT = 2


def frame(kind: int, body: bytes) -> bytes:
    return MAGIC + struct.pack("<HHI", PROTOCOL_VERSION, kind, len(body)) + body


def read_frame(stream) -> tuple[int, bytes]:
    magic = stream.read(4)
    if magic != MAGIC:
        raise SystemExit(f"bad magic {magic!r}")
    version, kind, length = struct.unpack("<HHI", stream.read(8))
    if version != PROTOCOL_VERSION:
        raise SystemExit(f"bad version {version}")
    if length > 256 * 1024:
        raise SystemExit("frame too large")
    body = stream.read(length)
    if len(body) != length:
        raise SystemExit("short frame")
    return kind, body


def main() -> None:
    if len(sys.argv) != 4:
        raise SystemExit("usage: admit-dump.py WORKER POLICY.json SQL")
    worker, policy_path, sql = sys.argv[1:]
    policy = json.loads(Path(policy_path).read_text())
    nonce = "p05-dump-nonce"
    request = json.dumps({"sql": sql, "policy": policy}).encode()
    with tempfile.TemporaryDirectory(prefix="oo-p05-dump-") as work:
        proc = subprocess.Popen(
            [worker, nonce],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            cwd=work,
            env={"PATH": "/usr/bin:/bin"},
        )
        assert proc.stdin is not None and proc.stdout is not None
        kind, ready = read_frame(proc.stdout)
        if kind != 1:
            raise SystemExit(f"expected ready, got {kind}")
        proc.stdin.write(frame(KIND_ADMIT, request))
        proc.stdin.close()
        kind, body = read_frame(proc.stdout)
        err = proc.stderr.read() if proc.stderr is not None else b""
        rc = proc.wait(timeout=5)
    if kind != 3:
        raise SystemExit(f"expected response, got {kind} rc={rc} err={err!r}")
    decision = json.loads(body)
    decision["worker_exit"] = rc
    decision["ready_nonce_match"] = json.loads(ready).get("nonce") == nonce
    json.dump(decision, sys.stdout)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
