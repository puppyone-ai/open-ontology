#!/usr/bin/env python3
"""Docker-free runtime prototype (card B-S14-01, ADR-S14-docker-free-runtime).

Same flow as the L2 script, minus Docker: a private PostgreSQL cluster created with initdb and run by
pg_ctl (the stand-in for an embedded PostgreSQL distribution), `oo serve --runtime host` with the
engine as a child process, a 20 file Markdown Drive. It records the wall time of every phase from
"start" to the first BRIEF. Markdown, text and CSV need no parser sidecar; the Office and PDF formats
need the uv-managed sidecar, which this prototype does not start (see the ADR).

Usage: tools/oo-test/super_ontology_nodocker.py [--skip-build] [--out FILE]
Needs PostgreSQL server binaries on PATH (initdb, pg_ctl) and debug builds of `oo` (feature docs)
and `oo-docs-engine`.
"""

import argparse
import json
import os
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import super_ontology_l2 as l2  # noqa: E402

# macOS postmaster refuses to start multithreaded without a valid locale
os.environ.setdefault("LC_ALL", "en_US.UTF-8")

REPO = l2.REPO


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


class Native:
    """A private PostgreSQL cluster; trust authentication on loopback only."""

    def __init__(self, root):
        self.data = root / "pgdata"
        self.port = free_port()

    def start(self):
        subprocess.run(["initdb", "-D", str(self.data), "-U", "postgres", "--auth=trust", "-E", "UTF8", "--no-locale"],
                       check=True, capture_output=True)
        subprocess.run(["pg_ctl", "-D", str(self.data), "-w", "-t", "60", "-l", str(self.data.parent / "pg.log"),
                        "-o", f"-p {self.port} -c listen_addresses=127.0.0.1 -c unix_socket_directories={self.data.parent}",
                        "start"], check=True, capture_output=True)

    def psql(self, db, sql):
        p = subprocess.run(["psql", "-h", "127.0.0.1", "-p", str(self.port), "-U", "postgres", "-d", db, "-v", "ON_ERROR_STOP=1", "-tAc", sql],
                           capture_output=True, text=True)
        if p.returncode:
            raise SystemExit("psql failed: " + p.stderr.strip())
        return p.stdout.strip()

    def url(self, user, db):
        return f"postgres://{user}@127.0.0.1:{self.port}/{db}"

    def stop(self):
        subprocess.run(["pg_ctl", "-D", str(self.data), "-m", "fast", "stop"], capture_output=True)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--skip-build", action="store_true")
    ap.add_argument("--out", default="")
    args = ap.parse_args()
    target = Path(os.environ.get("CARGO_TARGET_DIR", REPO / "target")) / "debug"
    oo, engine = target / "oo", target / "oo-docs-engine"
    if not args.skip_build:
        subprocess.run(["cargo", "build", "--locked", "-p", "oo-cli", "--features", "docs"], cwd=REPO, check=True)
        subprocess.run(["cargo", "build", "--locked", "-p", "oo-docs", "--bin", "oo-docs-engine"], cwd=REPO, check=True)
    work = Path(tempfile.mkdtemp(prefix="oo-nodocker-", dir="/tmp"))
    pg = Native(work)
    serve = None
    marks = {}
    t0 = time.time()

    def mark(name):
        marks[name] = round(time.time() - t0, 2)

    ok = False
    try:
        pg.start()
        mark("postgres_started")
        pg.psql("postgres", "CREATE DATABASE oo_docs_nd")
        state = work / "state"
        (state / "engine").mkdir(parents=True)
        state.chmod(0o700)
        (state / "engine").chmod(0o700)
        (state / "engine/docs.json").write_text(json.dumps({"version": "oo.docs/v1", "sync": {"fs_poll_seconds": 5}}))
        drive = work / "drive"
        n = l2.build_drive(drive)
        admin = pg.url("postgres", "oo_docs_nd")
        tok = work / "install.token"
        tok.write_text("oodi_install_only_0123456789abcdef")
        tok.chmod(0o600)
        base = {"PATH": os.environ.get("PATH", ""), "TMPDIR": os.environ.get("TMPDIR", "/tmp")}
        (work / "install-state").mkdir()
        inst = subprocess.Popen([str(engine)], text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=dict(
            base, OO_DOCS_LISTEN="127.0.0.1:0", OO_DOCS_STATE_DIR=str(work / "install-state"),
            OO_DOCS_DATABASE_URL=admin, OO_DOCS_ADMIN_DATABASE_URL=admin, OO_DOCS_ENGINE_TOKEN_FILE=str(tok)))
        first = inst.stdout.readline()
        inst.send_signal(signal.SIGTERM)
        inst.wait(timeout=30)
        assert '"listening"' in first, first
        pg.psql("oo_docs_nd", "CREATE ROLE oo_docs_nd_rt LOGIN IN ROLE oo_docs_runtime")
        mark("schema_installed")
        env = dict(base, OO_DOCS_DATABASE_URL=pg.url("oo_docs_nd_rt", "oo_docs_nd"))
        serve = l2.Serve(l2.Evidence(work / "ev", "nd"), oo, engine, state, env)
        api = serve.start()
        mark("serve_up")
        st, d = api.request("POST", "/v1/docs/drives", {"name": "nd", "backend": "fs", "config": {"root": str(drive)}})
        assert st == 201, st
        drive_id = d["id"]
        mark("drive_registered")
        deadline = time.time() + 120
        while time.time() < deadline:
            s = l2.file_states(api, drive_id)
            if len(s) == n and all(v == "published" for v in s.values()):
                break
            time.sleep(0.25)
        else:
            raise SystemExit("files did not publish")
        mark("all_published")
        st, pack = api.request("POST", "/v1/docs/context-packs:compile", {"drive_id": drive_id, "scope": ["Projects/a"]})
        assert st in (200, 201) and pack.get("brief_preview"), st
        mark("first_brief")
        ok = True
    finally:
        if serve:
            serve.stop()
        pg.stop()
        shutil.rmtree(work, ignore_errors=True)
    doc = {"protocol": "oo.bench.nodocker/v1", "ok": ok, "files": n if ok else None, "seconds_from_start": marks,
           "host": "macOS arm64, Homebrew PostgreSQL 17 binaries, debug builds of oo and oo-docs-engine",
           "excludes": "downloading or building the binaries, uv sidecar environment, model downloads"}
    print(json.dumps(doc, indent=1))
    if args.out:
        Path(args.out).write_text(json.dumps(doc, indent=1) + "\n")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
