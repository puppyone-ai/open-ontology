#!/usr/bin/env python3
"""Offline review acceptance: real Rust compiler, real PTY, no model/network."""
import argparse
import copy
import fcntl
import json
import os
from pathlib import Path
import pty
import re
import select
import struct
import subprocess
import sys
import termios
import time

ROOT = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(ROOT / "tools/pg-builder/scripts"))
import selfbuild_budget as lib
from test_selfbuild_budget import Acceptance, Model, fixture


def make_input(out):
    tables, edges = fixture(2, 2)
    source = ROOT / "testing/real-db/gateway-compiled-pack/fixtures"
    effective = json.loads((source / "effective.json").read_text())
    visibility = json.loads((source / "visibility-picker.json").read_text())
    effective["ontology"]["include_schemas"] = ["app"]
    effective["datasets"] = [
        dict(schema_version="oos/v1", dataset=t["dataset"], tier="file",
             label=t["dataset"], primary_key=["id"],
             columns=[dict(name=c["name"], pii="none", semantic_type="identifier")
                      for c in t["columns"]]) for t in tables]
    effective["metrics"] = [
        dict(metric="item_count", label="Item count", status="draft",
             sql="SELECT COUNT(*) FROM app.item0")]
    visibility["datasets"] = [
        dict(dataset=t["dataset"], row_security="none", global_stats="denied",
             columns=[c["name"] for c in t["columns"]]) for t in tables]
    draft = lib.Runner(Acceptance().config(), out / "model-replay", Model()).run(tables, edges)
    return dict(protocol="oo.builder.reviewed-community/v1", draft=draft,
                effective=effective, visibility=visibility, orphan_threshold=.2, ceilings={})


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--run-id", required=True)
    args = parser.parse_args()
    assert re.fullmatch("[a-z0-9-]+", args.run_id)
    os.umask(0o077)
    out = ROOT / "evidence" / args.run_id
    out.mkdir(mode=0o700)
    binary = ROOT / "target/debug/oo"
    command = make_input(out)
    lib.write_private(out / "input.json", command)
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 80, 180, 0, 0))
    transcript = bytearray()
    process = subprocess.Popen(
        [str(binary), "review", "--input", str(out / "input.json"),
         "--out", str(out / "tty.json")], cwd=out,
        env=dict(os.environ, TERM="xterm-256color"),
        stdin=slave, stdout=slave, stderr=slave, close_fds=True)
    os.close(slave)

    def drain(seconds=.12):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            if select.select([master], [], [], .02)[0]:
                try:
                    data = os.read(master, 65536)
                except OSError:
                    break
                if not data:
                    break
                transcript.extend(data)

    def enter(text):
        os.write(master, (text + "\r").encode())
        drain(.25)

    def visible():
        return re.sub(rb"\s+", b"", re.sub(rb"\x1b\[[0-?]*[ -/]*[@-~]", b"", bytes(transcript)))

    try:
        for _ in range(80):
            drain()
            if b"Reviewdate" in visible():
                break
        assert b"Reviewdate" in visible(), bytes(transcript)
        enter("2026-11-04")
        # An empty answer must not accept the first entity.
        enter("")
        pending = out / "tty.pending.json"
        assert not pending.exists()
        entity_ids = sorted(command["draft"]["registry"]["entities"])
        enter("edit")
        enter('{"canonical_name":"Reviewed item"}')
        enter("Owner supplied business label")
        # Interrupt with a genuine terminal cancellation after the first saved decision.
        assert len(json.loads(pending.read_text())["items"]) == 1
        os.write(master, b"\x03")
        process.wait(timeout=10)
        drain()
        assert process.returncode != 0 and not (out / "tty.json").exists()
        (out / "terminal-interrupted.raw").write_bytes(transcript)
    finally:
        (out / "terminal-first.raw").write_bytes(transcript)
        if process.poll() is None:
            process.kill()
            process.wait()
        os.close(master)

    # Resume the actual TUI, then replay its operator decisions noninteractively.
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 80, 180, 0, 0))
    transcript.clear()
    process = subprocess.Popen(
        [str(binary), "review", "--input", str(out / "input.json"), "--out", str(out / "tty.json")],
        cwd=out, env=dict(os.environ, TERM="xterm-256color"),
        stdin=slave, stdout=slave, stderr=slave, close_fds=True)
    os.close(slave)
    try:
        drain(1)
        # Remaining entity; table/column descriptions and relationship; metric; classifications.
        count = len(entity_ids) + sum(
            v["kind"] != "entity" for v in command["draft"]["accepted"].values()
        ) + 1 + sum(len(t["columns"]) for t in command["effective"]["datasets"])
        for _ in range(count - 1):
            enter("accept")
        drain(1)
        assert b"G14connectivityreview" in visible()
        enter("Owner checked connectivity and FK mapping")
        process.wait(timeout=20)
        drain()
        assert process.returncode == 0, bytes(transcript)[-2000:]
        (out / "terminal-resumed.raw").write_bytes(transcript)
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
        os.close(master)
    reviewed = json.loads((out / "tty.json").read_text())
    decisions = reviewed["review_decision"]["decisions"]
    assert reviewed["review_decision"]["mode"] == "self"
    assert reviewed["compiled"]["effective"]["entities"][0]["label"] == "Reviewed item"
    assert reviewed["compiled"]["published"] is False
    lib.write_private(out / "decisions.json", decisions)

    def run(name, value=decisions, inp=command, extra=()):
        lib.write_private(out / (name + "-input.json"), inp)
        lib.write_private(out / (name + "-decisions.json"), value)
        result = subprocess.run(
            [str(binary), "review", "--input", str(out / (name + "-input.json")),
             "--review-file", str(out / (name + "-decisions.json")),
             "--out", str(out / (name + ".json")), *extra],
            capture_output=True, timeout=30)
        (out / (name + ".log")).write_bytes(result.stdout + result.stderr)
        return result

    assert run("file").returncode == 0
    assert json.loads((out / "file.json").read_text()) == reviewed
    tests = ["pty_edit_and_resume", "pty_file_identical", "real_pack_compile"]
    for name, mutate in [
        ("missing", lambda d: d["items"].pop(next(iter(d["items"])))),
        ("stale", lambda d: d.update(input_sha256="0" * 64)),
        ("g14-stale", lambda d: d["g14"].update(summary_sha256="0" * 64)),
        ("g14-missing", lambda d: d.update(g14=None)),
        ("auto", lambda d: d.update(auto_accept=True)),
        ("unknown-item", lambda d: d["items"].update(unknown={"action":"accept"})),
        ("invalid-edit", lambda d: d["items"].update({
            next(iter(d["items"])): {"action":"edit", "reason":"not allowed",
                                    "replacement":{"id":"different"}}})),
    ]:
        value = copy.deepcopy(decisions)
        mutate(value)
        assert run(name, value).returncode != 0, name
        assert not (out / (name + ".json")).exists()
        tests.append(name)
    rejected = copy.deepcopy(decisions)
    rejected["items"][next(iter(rejected["items"]))] = {
        "action":"reject", "reason":"Needs correction"}
    assert run("reject", rejected).returncode != 0
    rejected_output = json.loads((out / "reject.json").read_text())
    assert rejected_output["status"] == "rejected" and rejected_output["compiled"] is None
    assert run("yes", extra=("--yes",)).returncode != 0
    # No non-TTY implicit approval and no credential/content echo in diagnostics.
    result = subprocess.run([str(binary), "review", "--input", str(out / "input.json"),
                             "--out", str(out / "no-file.json")], capture_output=True)
    assert result.returncode != 0 and b"REVIEW_FILE_REQUIRED" in result.stderr
    tests += ["reject_blocks_pack", "yes_not_approval", "non_tty_requires_file"]
    for path in (out / "tty.json", out / "file.json", pending):
        assert path.stat().st_mode & 0o777 == 0o600
    lib.write_private(out / "result.json", dict(status="passed", tests=tests,
        model="deterministic_replay", live_model=False, published=False))
    print(json.dumps(dict(status="passed", tests=len(tests), run_id=args.run_id)))


if __name__ == "__main__":
    main()
