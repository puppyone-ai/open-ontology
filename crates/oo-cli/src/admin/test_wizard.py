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
        while b"Engine:" not in transcript and time.monotonic() < deadline:
            drain()
        assert b"Engine:" in transcript and process.poll() is None
        drain()
        # Missing MySQL TLS CA blocks before connection; saving never deploys.
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
        compact = re.sub(rb"\s+", b"", visible)
        assert b"Packpath:" in compact and b"pending" in compact
        assert b"Firsttoken:" in compact and b"enroll" in compact
        assert b"share-prompt" in visible
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
        # A real second PTY exercises execution and failure completion, without
        # fake successes or model spend. Resume retries the failed preflight.
        master2, slave2 = pty.openpty()
        fcntl.ioctl(slave2, termios.TIOCSWINSZ, struct.pack("HHHH", 60, 180, 0, 0))
        proc2 = subprocess.Popen([str(binary), "build", "--config", str(out/"stack.yaml")],
                                 cwd=out, env=env, stdin=slave2, stdout=slave2, stderr=slave2)
        os.close(slave2)
        captured = bytearray()
        try:
            deadline = time.monotonic()+30
            accepted = False
            while time.monotonic()<deadline:
                if select.select([master2], [], [], 0.1)[0]:
                    try:
                        captured.extend(os.read(master2,65536))
                    except OSError:
                        break
                if b"enroll" in captured and not accepted:
                    os.write(master2,b"\r")
                    accepted = True
                if proc2.poll() is not None:
                    break
            proc2.wait(timeout=10)
            assert proc2.returncode == 1
            assert b"execution" in captured and b"preflight" in captured
            assert b"STAGE_COMMAND_FAILED" in captured
            assert all(v.encode() not in captured for v in (password,key))
            (out/"execution.raw").write_bytes(captured)
        finally:
            (out/"execution.raw").write_bytes(captured)
            if proc2.poll() is None:
                proc2.kill()
                proc2.wait()
            os.close(master2)
        resumed = subprocess.run([str(binary),"build","--resume","--config",str(out/"stack.yaml"),"--json"],
                                 env=env,capture_output=True,timeout=30)
        assert resumed.returncode == 1
        state = json.loads(resumed.stdout)
        assert state["stages"][0]["attempts"] == 2
        assert state["status"] == "failed" and state["last_failure"]
        status = subprocess.run([str(binary),"status","--config",str(out/"stack.yaml"),"--json"],
                                env=env,capture_output=True,timeout=10)
        assert status.returncode == 1
        assert json.loads(status.stdout)["gateway_health"] == "unmeasured"
        assert all(v.encode() not in resumed.stdout+status.stdout for v in (password,key))
        handler = replay.httpd.RequestHandlerClass
        original_get = handler.do_GET
        def delayed_get(self):
            time.sleep(2)
            original_get(self)
        handler.do_GET = delayed_get
        replay.httpd.handle_error = lambda *_: None
        master3, slave3 = pty.openpty()
        fcntl.ioctl(slave3, termios.TIOCSWINSZ, struct.pack("HHHH", 60, 180, 0, 0))
        proc3 = subprocess.Popen([str(binary),"build","--resume","--config",str(out/"stack.yaml")],
                                 cwd=out,env=env,stdin=slave3,stdout=slave3,stderr=slave3)
        os.close(slave3)
        cancelled = bytearray()
        sent = False
        try:
            until = time.monotonic()+15
            while time.monotonic()<until:
                if select.select([master3],[],[],0.05)[0]:
                    try:
                        cancelled.extend(os.read(master3,65536))
                    except OSError:
                        break
                checkpoint=json.loads((out/".oo/run.json").read_text())
                if checkpoint["stages"][0]["attempts"]==3 and checkpoint["status"]=="running" and not sent:
                    os.write(master3,b"q")
                    sent=True
                if proc3.poll() is not None:
                    break
            proc3.wait(timeout=5)
            assert sent and proc3.returncode!=0
            assert b"BUILD_INTERRUPTED" in cancelled
            assert json.loads((out/".oo/run.json").read_text())["status"]=="failed"
            assert all(v.encode() not in cancelled for v in (password,key))
            (out/"cancel.raw").write_bytes(cancelled)
        finally:
            if proc3.poll() is None:
                proc3.kill();proc3.wait()
            os.close(master3)
            handler.do_GET=original_get
        (out / "result.json").write_text(json.dumps({
            "pty": "passed", "byte_identical": True, "secret_scan": "passed",
            "source": "missing MySQL TLS CA (blocked preflight, not source acceptance)",
            "model": "synthetic replay", "execution_failure_screen": "passed", "resume_retry": "passed",
            "status": "passed", "quit_checkpoint": "passed", "deployment": "not implemented",
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
