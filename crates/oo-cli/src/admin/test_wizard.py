#!/usr/bin/env python3
"""Real PTY input, captured terminal output, equivalence and secret assertions."""
import argparse
import fcntl
import json
import os
from pathlib import Path
import pty
import re
import secrets
import select
import struct
import subprocess
import sys
import termios
import time

ROOT = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(ROOT / "tools/model-egress/tests"))
from test_egress import Replay


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--run-id", required=True)
    args = parser.parse_args()
    if not args.run_id or any(c not in "abcdefghijklmnopqrstuvwxyz0123456789-" for c in args.run_id):
        parser.error("invalid run id")
    out = ROOT / "evidence" / args.run_id
    out.mkdir(exist_ok=False)
    home = out / "home"
    home.mkdir(mode=0o700)
    replay = Replay()
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 60, 180, 0, 0))
    transcript = bytearray()
    env = dict(os.environ, HOME=str(home), TERM="xterm-256color")
    binary = ROOT / "target/debug/oo"
    password, key = secrets.token_hex(24), secrets.token_hex(24)
    process = subprocess.Popen([str(binary), "init"], cwd=out, env=env,
                               stdin=slave, stdout=slave, stderr=slave, close_fds=True)
    os.close(slave)

    def drain(seconds=0.15):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            if select.select([master], [], [], 0.03)[0]:
                try:
                    data = os.read(master, 65536)
                except OSError:
                    return
                if not data:
                    return
                transcript.extend(data)

    try:
        deadline = time.monotonic() + 15
        while b"unavailable" not in transcript and time.monotonic() < deadline:
            drain()
        assert b"unavailable" in transcript and process.poll() is None
        drain()
        # Deliberately unavailable MySQL: verifies blocked preflight can save a
        # configuration but never reports deployment. The model is replay-only.
        values = [
            "mysql", "source", "localhost", "3306", "fixture", "reader",
            "source-password", password, "", "", "fixture.events", "",
            "openai-compatible", f"http://127.0.0.1:{replay.httpd.server_port}/v1",
            "fixture-model", "model-key", key, "", "",
        ]
        for value in values:
            os.write(master, (value + "\r").encode())
            drain(0.4)
        drain(1)
        for value in ["", "SAVE", ""]:
            os.write(master, (value + "\r").encode())
            drain(0.3)
        process.wait(timeout=10)
        drain()
        assert process.returncode == 0
        assert password.encode() not in transcript and key.encode() not in transcript
        visible = re.sub(rb"\x1b\[[0-?]*[ -/]*[@-~]", b"", bytes(transcript))
        assert password.encode() not in visible and key.encode() not in visible
        (out / "terminal.raw").write_bytes(transcript)
        wizard = (out / "stack.yaml").read_bytes()
        expected = {
            "version": "oo.stack/v1",
            "sources": [{"id": "source", "engine": "mysql", "host": "localhost",
                         "port": 3306, "database": "fixture", "user": "reader",
                         "password": {"secret_ref": "source-password"},
                         "scope": {"include": ["fixture.events"], "exclude": []}}],
            "model": {"provider": "openai-compatible",
                      "base_url": f"http://127.0.0.1:{replay.httpd.server_port}/v1",
                      "model": "fixture-model", "key": {"secret_ref": "model-key"}},
        }
        config = out / "noninteractive.yaml"
        config.write_text(json.dumps(expected))
        normalized = subprocess.run([str(binary), "init", "--config", str(config), "--json"],
                                    env=env, capture_output=True, timeout=10)
        assert normalized.returncode == 0
        assert wizard == config.read_bytes()
        store = home / ".config/open-ontology/secrets"
        assert store.stat().st_mode & 0o777 == 0o700
        for name, value in [("source-password", password), ("model-key", key)]:
            path = store / name
            assert path.stat().st_mode & 0o777 == 0o600 and path.read_text() == value
            assert value.encode() not in wizard
        assert (out / "stack.yaml").stat().st_mode & 0o777 == 0o600
        (out / "result.json").write_text(json.dumps({
            "pty": "passed", "byte_identical": True, "secret_scan": "passed",
            "source": "unavailable MySQL (blocked preflight, not source acceptance)",
            "model": "synthetic replay", "deployment": "not implemented",
        }, indent=2))
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
        os.close(master)
        replay.close()
        # Preserve only non-secret evidence, including failed terminal output
        # with synthetic credentials removed.
        raw = bytes(transcript).replace(password.encode(), b"[REDACTED]").replace(key.encode(), b"[REDACTED]")
        (out / "terminal.redacted.raw").write_bytes(raw)
        store = home / ".config/open-ontology/secrets"
        for name in ("source-password", "model-key"):
            (store / name).unlink(missing_ok=True)


if __name__ == "__main__":
    main()
