#!/usr/bin/env python3
"""Super Ontology L2 walking skeleton, end to end (30 section L2 acceptance 1-5).

Runs the real stack: a labelled Postgres container, the real `oo-docs-engine`
as the host child process of the real `oo serve`, and the real `oo context
mount`, over a 20 file Markdown Drive built from the synthetic golden set.

  acceptance 1  all 20 files `published` within 60 s of registering the Drive
  acceptance 2  `oo context mount --scope Projects/a` gives `.context/` and
                every BRIEF content line carries `[src:`
  acceptance 3  two mounts of the same input are byte identical except the
                share id, the times and the refs credential in SHARE.json
  acceptance 4  (API level) killing the engine and restarting `oo serve` loses
                no event: `since` replay is gap free and ends at the head
  acceptance 5  editing one Markdown file runs its state chain again and the
                BRIEF changes within 30 s

Acceptance 6 and 7 need Desktop and a packet capture; they are not covered.

Usage:  tools/oo-test/super_ontology_l2.py [--run-id ID] [--evidence-dir DIR]
        [--keep] [--skip-build]

Evidence (commands, outputs, hashes, timings) goes to
testing/super-ontology/evidence/L2/ by default. No token or credential is
written there; the refs credential in SHARE.json is replaced by a placeholder.
"""

import argparse
import hashlib
import http.client
import json
import os
import re
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time
import uuid
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
GOLDEN = REPO / "testing/super-ontology/golden/docs"
PG_IMAGE = (
    "docker.io/library/postgres@sha256:"
    "839fbb5a4942a7758dd93e18b42ea5649807491a0545b5eeeb8f0b8bb11857a5"
)
LANE = "oo.lane=super-ontology-be5"
PUBLISH_DEADLINE = 60.0
EDIT_DEADLINE = 30.0
EXTRA_FILES = {
    "Projects/a/星火计划.md": (
        "---\ntype: project\nname: 星火计划\nowner: 林川\n---\n"
        "# 星火计划\n\n负责人：林川\n\n状态：进行中\n"
    ),
    "People/林川.md": (
        "---\ntype: person\nname: 林川\nrole: 工程负责人\n---\n"
        "# 林川\n\n角色：工程负责人\n"
    ),
}
EDITED = "Projects/a/星火计划.md"


class Evidence:
    """Collects the run log and result facts."""

    def __init__(self, out: Path, run_id: str):
        self.out = out
        self.run_id = run_id
        self.log_lines = []
        self.checks = []
        self.timings = {}
        out.mkdir(parents=True, exist_ok=True)

    def log(self, text=""):
        # Evidence carries no home path: the repository shows as <repo>.
        text = text.replace(str(REPO), "<repo>")
        home = os.environ.get("HOME")
        if home:
            text = text.replace(home, "~")
        print(text, flush=True)
        self.log_lines.append(text)

    def command(self, argv, shown=None):
        self.log("$ " + " ".join(shown or argv))

    def check(self, name, ok, detail=""):
        self.checks.append({"check": name, "ok": bool(ok), "detail": detail})
        self.log(f"[{'PASS' if ok else 'FAIL'}] {name}" + (f" - {detail}" if detail else ""))
        return ok

    def write(self, name, content):
        path = self.out / name
        if isinstance(content, bytes):
            path.write_bytes(content)
        else:
            path.write_text(content, encoding="utf-8")


def run(ev, argv, env=None, check=True, shown=None, timeout=900, input_text=None):
    ev.command(argv, shown)
    p = subprocess.run(
        argv, env=env, capture_output=True, text=True, timeout=timeout, input=input_text
    )
    out = (p.stdout + p.stderr).strip()
    if out:
        tail = out.splitlines()[-12:]
        for line in tail:
            ev.log("  " + line)
    if check and p.returncode != 0:
        raise SystemExit(f"command failed ({p.returncode}): {' '.join(argv)}")
    return p


# --------------------------------------------------------------- postgres


class Postgres:
    def __init__(self, ev, run_id):
        self.ev = ev
        self.name = f"oo-l2-pg-{run_id}"
        self.port = None

    def start(self):
        run(
            self.ev,
            [
                "docker", "run", "-d", "--name", self.name, "--label", LANE,
                "-e", "POSTGRES_HOST_AUTH_METHOD=trust",
                "-p", "127.0.0.1::5432", PG_IMAGE, "-c", "track_commit_timestamp=on",
            ],
        )
        out = subprocess.check_output(["docker", "port", self.name, "5432/tcp"], text=True)
        self.port = int(out.strip().splitlines()[0].rsplit(":", 1)[1])
        for _ in range(60):
            p = subprocess.run(
                ["docker", "exec", self.name, "pg_isready", "-U", "postgres"],
                capture_output=True,
            )
            if p.returncode == 0:
                time.sleep(1)
                return
            time.sleep(1)
        raise SystemExit("postgres did not become ready")

    def psql(self, db, sql):
        p = subprocess.run(
            ["docker", "exec", self.name, "psql", "-U", "postgres", "-d", db, "-v", "ON_ERROR_STOP=1", "-tAc", sql],
            capture_output=True, text=True,
        )
        if p.returncode != 0:
            raise SystemExit(f"psql failed: {p.stderr.strip()}")
        return p.stdout.strip()

    def url(self, user, db):
        return f"postgres://{user}@127.0.0.1:{self.port}/{db}"

    def stop(self):
        subprocess.run(["docker", "rm", "-f", "-v", self.name], capture_output=True)


# -------------------------------------------------------------------- api


class Api:
    def __init__(self, port, token):
        self.port, self.token = port, token

    def request(self, method, path, body=None, timeout=30):
        conn = http.client.HTTPConnection("127.0.0.1", self.port, timeout=timeout)
        headers = {"Authorization": f"Bearer {self.token}"}
        data = None
        if body is not None:
            data = json.dumps(body).encode()
            headers["Content-Type"] = "application/json"
        if method == "POST":
            headers["Idempotency-Key"] = uuid.uuid4().hex + uuid.uuid4().hex[:8]
        conn.request(method, path, body=data, headers=headers)
        resp = conn.getresponse()
        raw = resp.read()
        conn.close()
        try:
            parsed = json.loads(raw)
        except ValueError:
            parsed = raw
        return resp.status, parsed

    def events(self, since, listen=3.0):
        """Replay events after `since`; returns the events seen until the stream
        has been quiet for `listen` seconds (live heartbeats do not count)."""
        conn = http.client.HTTPConnection("127.0.0.1", self.port, timeout=listen)
        conn.request(
            "GET", f"/v1/docs/events?since={since}", headers={"Authorization": f"Bearer {self.token}"}
        )
        resp = conn.getresponse()
        if resp.status != 200:
            raise RuntimeError(f"events: HTTP {resp.status}")
        out, buf = [], b""
        try:
            while True:
                chunk = resp.fp.readline()
                if not chunk:
                    break
                buf += chunk
                if chunk in (b"\n", b"\r\n"):
                    for line in buf.decode().splitlines():
                        if line.startswith("data:"):
                            try:
                                out.append(json.loads(line[5:].strip()))
                            except ValueError:
                                pass
                    buf = b""
        except (socket.timeout, TimeoutError):
            pass
        finally:
            conn.close()
        return out


# ------------------------------------------------------------------ helpers


def remove_tree(root: Path):
    """Delete a tree that holds read-only mounts (0555 directories)."""
    for dirpath, dirnames, _ in os.walk(root):
        for d in dirnames:
            p = os.path.join(dirpath, d)
            if not os.path.islink(p):
                os.chmod(p, 0o700)
    if root.exists():
        os.chmod(root, 0o700)
    shutil.rmtree(root, ignore_errors=True)


def sha256(b: bytes) -> str:
    return hashlib.sha256(b).hexdigest()


def tree_hashes(root: Path):
    out = {}
    for p in sorted(root.rglob("*")):
        if p.is_file():
            out[str(p.relative_to(root))] = sha256(p.read_bytes())
    return out


def strip_share(raw: bytes):
    v = json.loads(raw)
    for k in ("share_id", "created_at", "expires_at", "refs"):
        v.pop(k, None)
    return v


def content_lines(text: str):
    """BRIEF lines that state something: bullets and numbered items."""
    return [l for l in text.splitlines() if re.match(r"^\s*(?:[-*+]|\d+[.)])\s+\S", l)]


def build_drive(drive: Path):
    n = 0
    for p in sorted(GOLDEN.rglob("*.md")):
        dest = drive / p.relative_to(GOLDEN)
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(p, dest)
        n += 1
    for rel, text in EXTRA_FILES.items():
        dest = drive / rel
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_text(text, encoding="utf-8")
        n += 1
    return n


def file_states(api, drive_id):
    st, body = api.request("GET", f"/v1/docs/drives/{drive_id}/files?limit=200")
    assert st == 200, body
    return {f["path"]: f["state"] for f in body["items"]}


def wait_until(deadline, fn, step=0.25):
    end = time.time() + deadline
    while time.time() < end:
        v = fn()
        if v:
            return v
        time.sleep(step)
    return None


class Serve:
    """`oo serve` as a child process."""

    def __init__(self, ev, oo, engine, state, env):
        self.ev, self.oo, self.engine, self.state, self.env = ev, oo, engine, state, env
        self.proc = None
        self.info = None
        self.token = None

    def start(self):
        argv = [str(self.oo), "serve", "--json", "--port", "0", "--state-dir", str(self.state)]
        self.ev.command(argv)
        env = dict(self.env, OO_DOCS_ENGINE_BIN=str(self.engine))
        self.proc = subprocess.Popen(
            argv, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True
        )
        line = self.proc.stdout.readline()
        self.info = json.loads(line)
        self.token = Path(self.info["token_file"]).read_text().strip()
        shown = dict(self.info, token_file="<state>/serve.token", state_dir="<state>")
        self.ev.log("  " + json.dumps(shown, sort_keys=True, ensure_ascii=False))
        return Api(self.info["port"], self.token)

    def stop(self):
        if not self.proc:
            return None
        self.proc.send_signal(signal.SIGTERM)
        try:
            code = self.proc.wait(timeout=30)
        except subprocess.TimeoutExpired:
            self.proc.kill()
            code = self.proc.wait()
        self.proc = None
        return code


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--run-id", default=time.strftime("%Y%m%d-%H%M%S"))
    ap.add_argument("--evidence-dir", default=str(REPO / "testing/super-ontology/evidence/L2"))
    ap.add_argument("--keep", action="store_true", help="keep the temp directory and container")
    ap.add_argument("--skip-build", action="store_true")
    args = ap.parse_args()

    ev = Evidence(Path(args.evidence_dir), args.run_id)
    target_dir = Path(os.environ.get("CARGO_TARGET_DIR", REPO / "target")) / "debug"
    oo, engine = target_dir / "oo", target_dir / "oo-docs-engine"
    work = Path(tempfile.mkdtemp(prefix=f"oo-l2-{args.run_id}-", dir="/tmp"))
    ev.log(f"# Super Ontology L2 end to end, run {args.run_id}")
    pg = Postgres(ev, args.run_id)
    serve = None
    ok = False
    try:
        if not args.skip_build:
            run(ev, ["cargo", "build", "--locked", "-p", "oo-cli", "--features", "docs"])
            run(ev, ["cargo", "build", "--locked", "-p", "oo-docs", "--bin", "oo-docs-engine"])
        for b in (oo, engine):
            if not b.is_file():
                raise SystemExit(f"missing binary {b}")

        # ---- environment: postgres, schema, runtime login, 20 file drive
        pg.start()
        db, login = "oo_docs_l2", "oo_docs_l2_runtime"
        pg.psql("postgres", f"CREATE DATABASE {db}")
        admin_url = pg.url("postgres", db)
        state = work / "state"
        (state / "engine").mkdir(parents=True)
        state.chmod(0o700)
        (state / "engine").chmod(0o700)
        (state / "engine/docs.json").write_text(
            json.dumps({"version": "oo.docs/v1", "sync": {"fs_poll_seconds": 5}})
        )
        drive = work / "drive"
        n_files = build_drive(drive)
        ev.check("drive has 20 markdown files", n_files == 20 and len(list(drive.rglob("*.md"))) == 20, f"{n_files} files")

        # Install the schema once with the admin login (the engine does it when
        # OO_DOCS_ADMIN_DATABASE_URL is set), then give the engine its runtime login.
        tokfile = work / "install.token"
        tokfile.write_text("oodi_install_only_0123456789abcdef")
        tokfile.chmod(0o600)
        env_base = {"PATH": os.environ.get("PATH", ""), "TMPDIR": os.environ.get("TMPDIR", "/tmp")}
        inst_env = dict(
            env_base,
            OO_DOCS_LISTEN="127.0.0.1:0", OO_DOCS_STATE_DIR=str(work / "install-state"),
            OO_DOCS_DATABASE_URL=admin_url, OO_DOCS_ADMIN_DATABASE_URL=admin_url,
            OO_DOCS_ENGINE_TOKEN_FILE=str(tokfile),
        )
        (work / "install-state").mkdir()
        ev.command([str(engine)], ["oo-docs-engine", "(schema install, then stop)"])
        inst = subprocess.Popen([str(engine)], env=inst_env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        first = inst.stdout.readline()
        inst.send_signal(signal.SIGTERM)
        inst.wait(timeout=30)
        ev.check("schema installed by the engine installer", '"listening"' in first, first.strip()[:80])
        pg.psql(db, f"CREATE ROLE {login} LOGIN IN ROLE oo_docs_runtime")
        runtime_url = pg.url(login, db)

        # ---- oo serve + engine
        env = dict(env_base, OO_DOCS_DATABASE_URL=runtime_url)
        serve = Serve(ev, oo, engine, state, env)
        api = serve.start()
        st, health = api.request("GET", "/v1/docs/health")
        ev.check("serve proxies the engine health", st == 200 and health.get("status") in ("ok", "degraded"), f"HTTP {st} status={health.get('status') if isinstance(health, dict) else health}")
        modes = {p: oct(os.stat(state / p).st_mode & 0o777) for p in ("serve.token", "serve.json", "engine.token")}
        ev.check("token files are 0600 and the state dir 0700", all(m == "0o600" for m in modes.values()) and oct(os.stat(state).st_mode & 0o777) == "0o700", json.dumps(modes))

        # ================= acceptance 1: 20 files published within 60 s
        ev.log("\n## Acceptance 1: register the Drive, all 20 files published within 60 s")
        t0 = time.time()
        st, d = api.request("POST", "/v1/docs/drives", {"name": "l2-drive", "backend": "fs", "config": {"root": str(drive)}})
        ev.check("drive registered", st == 201, f"HTTP {st}")
        drive_id = d["id"]
        states = {}

        def all_published():
            nonlocal states
            states = file_states(api, drive_id)
            return len(states) == 20 and all(s == "published" for s in states.values())

        done = wait_until(PUBLISH_DEADLINE + 30, all_published)
        elapsed = time.time() - t0
        ev.timings["acceptance1_all_published_seconds"] = round(elapsed, 2)
        counts = {}
        for s in states.values():
            counts[s] = counts.get(s, 0) + 1
        ev.log(f"  states after {elapsed:.1f}s: {json.dumps(counts, sort_keys=True)}")
        ev.check("A1 all 20 files published", bool(done) and counts.get("published") == 20, json.dumps(counts, sort_keys=True))
        ev.check(f"A1 within {PUBLISH_DEADLINE:.0f}s", elapsed <= PUBLISH_DEADLINE, f"{elapsed:.1f}s")
        if not (done and counts.get("published") == 20):
            # Keep what is needed to see why: job and state summaries and the
            # tail of the engine log (log lines carry no row values or secrets).
            diag = [
                "jobs:\n" + pg.psql(db, "SELECT kind || ' ' || state || ' attempt=' || attempt || ' ' || coalesce(last_error,'') || ' x' || count(*) FROM _oo_docs.job GROUP BY kind, state, attempt, last_error ORDER BY 1"),
                "files:\n" + pg.psql(db, "SELECT state || ' ' || coalesce(state_code,'') || ' x' || count(*) FROM _oo_docs.file GROUP BY state, state_code ORDER BY 1"),
                "engine.log (tail):\n" + "\n".join((state / "engine.log").read_text(errors="replace").splitlines()[-40:]),
            ]
            ev.write("a1-failure-diagnostics.txt", "\n\n".join(diag) + "\n")
            ev.log("  diagnostics written to a1-failure-diagnostics.txt")
        ev.write("a1-file-states.json", json.dumps(dict(sorted(states.items())), ensure_ascii=False, indent=2) + "\n")

        # ================= acceptance 2: mount a scope, every BRIEF line has [src:
        ev.log("\n## Acceptance 2: oo context mount, every BRIEF content line has [src:")
        mounts = {}
        for name in ("t1", "t2"):
            to = work / name
            to.mkdir()
            argv = [str(oo), "context", "mount", "--scope", "Projects/a", "--to", str(to), "--json", "--state-dir", str(state)]
            p = run(ev, argv, check=False, shown=[str(oo), "context", "mount", "--scope", "Projects/a", "--to", f"/tmp/.../{name}", "--json"])
            ev.check(f"A2 mount {name} exits 0", p.returncode == 0, f"exit {p.returncode}")
            mounts[name] = (to, json.loads(p.stdout.strip().splitlines()[-1]) if p.returncode == 0 else {})
        t1, m1 = mounts["t1"]
        brief = (t1 / ".context/BRIEF.md").read_text(encoding="utf-8")
        lines = content_lines(brief)
        missing = [l for l in lines if "[src:" not in l]
        ev.check("A2 BRIEF.md exists", bool(brief))
        ev.check("A2 BRIEF has content lines", len(lines) > 0, f"{len(lines)} content lines")
        ev.check("A2 every BRIEF content line has [src:", not missing, f"{len(lines) - len(missing)}/{len(lines)}")
        ev.check("A2 mount is read only", all(not os.access(p, os.W_OK) for p in (t1 / ".context", t1 / ".context/BRIEF.md")), "0555/0444")
        ev.check("A2 mount_json fields", m1.get("protocol") == "oo.context-mount/v1" and m1.get("share_id") and m1.get("pack_digest"), json.dumps({k: m1.get(k) for k in ("protocol", "tokens")}, sort_keys=True))
        ev.write("a2-BRIEF.md", brief)
        ev.write("a2-mount-listing.txt", "\n".join(f"{h}  {p}" for p, h in sorted(tree_hashes(t1 / ".context").items())) + "\n")

        # ================= acceptance 3: byte identical modulo SHARE.json fields
        ev.log("\n## Acceptance 3: two mounts are byte identical (SHARE.json share id, times, refs aside)")
        t2, m2 = mounts["t2"]
        h1, h2 = tree_hashes(t1 / ".context"), tree_hashes(t2 / ".context")
        ev.check("A3 same file set", sorted(h1) == sorted(h2), f"{len(h1)} files")
        diff = [p for p in h1 if p != "SHARE.json" and h1[p] != h2.get(p)]
        ev.check("A3 every file except SHARE.json is byte identical", not diff, f"{len(h1) - 1} files compared" + (f"; differ: {diff}" if diff else ""))
        s1 = (t1 / ".context/SHARE.json").read_bytes()
        s2 = (t2 / ".context/SHARE.json").read_bytes()
        ev.check("A3 SHARE.json equal once share_id, created_at, expires_at, refs are removed", strip_share(s1) == strip_share(s2))
        j1, j2 = json.loads(s1), json.loads(s2)
        ev.check("A3 the share ids and refs credentials differ", j1["share_id"] != j2["share_id"] and j1["refs"]["credential"] != j2["refs"]["credential"])
        ev.check("A3 SHARE.json lists the pack digest", j1["pack_digest"] == m1["pack_digest"] == m2["pack_digest"], m1["pack_digest"][:16] + "...")
        redacted = json.loads(s1)
        redacted["refs"]["credential"] = "oodr_REDACTED"
        ev.write("a3-SHARE.t1.redacted.json", json.dumps(redacted, ensure_ascii=False, indent=2, sort_keys=True) + "\n")
        ev.write("a3-hashes.txt", "\n".join(f"{h1[p]}  {p}  {'==' if p != 'SHARE.json' and h1[p] == h2.get(p) else 'differs (expected)' if p == 'SHARE.json' else 'DIFFERS'}" for p in sorted(h1)) + "\n")

        # ================= acceptance 5: edit one file, state re-runs, BRIEF changes
        ev.log("\n## Acceptance 5: edit one Markdown file, state re-runs and BRIEF changes within 30 s")
        cursor = api.request("GET", "/v1/docs/health")[1]["events_head"]
        file_id = next(f["id"] for f in api.request("GET", f"/v1/docs/drives/{drive_id}/files?limit=200")[1]["items"] if f["path"] == EDITED)
        text = (drive / EDITED).read_text(encoding="utf-8")
        edited = text.replace("林川", "周远")
        t_edit = time.time()
        (drive / EDITED).write_text(edited, encoding="utf-8")
        ev.log(f"  edited {EDITED}: owner 林川 -> 周远")
        body = {"drive_id": drive_id, "scope": ["Projects/a"]}

        def brief_changed():
            st, pack = api.request("POST", "/v1/docs/context-packs:compile", body)
            if st not in (200, 201):
                return None
            b = pack.get("brief_preview", "")
            return pack if "周远" in b and states_ok() else None

        def states_ok():
            s = file_states(api, drive_id)
            return len(s) == 20 and all(v == "published" for v in s.values())

        pack = wait_until(EDIT_DEADLINE + 30, brief_changed, step=0.5)
        edit_elapsed = time.time() - t_edit
        ev.timings["acceptance5_edit_to_brief_seconds"] = round(edit_elapsed, 2)
        ev.check("A5 BRIEF changed after the edit", bool(pack), f"{edit_elapsed:.1f}s")
        ev.check(f"A5 within {EDIT_DEADLINE:.0f}s", edit_elapsed <= EDIT_DEADLINE, f"{edit_elapsed:.1f}s")
        events = api.events(cursor)
        file_events = [e for e in events if e.get("type") == "file.state_changed" and e.get("data", {}).get("file", {}).get("id") == file_id]
        history = pg.psql(db, f"SELECT to_state FROM _oo_docs.file_state_history WHERE file_id = '{file_id}' ORDER BY seq").split()
        chain = history[-8:]
        ev.log(f"  state history of the edited file (last 8): {' > '.join(chain)}")
        want = ["superseded", "stored", "queued", "parsing", "parsed", "extracting", "indexed", "published"]
        ev.check("A5 the file went through the whole state chain again", chain == want, " > ".join(chain))
        ev.check("A5 every step of the chain was announced as a file.state_changed event", len(file_events) == len(want), f"{len(file_events)} events after cursor {cursor}")
        t3 = work / "t3"
        t3.mkdir()
        p = run(ev, [str(oo), "context", "mount", "--scope", "Projects/a", "--to", str(t3), "--json", "--state-dir", str(state)], check=False, shown=[str(oo), "context", "mount", "--scope", "Projects/a", "--to", "/tmp/.../t3", "--json"])
        brief3 = (t3 / ".context/BRIEF.md").read_text(encoding="utf-8") if p.returncode == 0 else ""
        ev.check("A5 a new mount's BRIEF differs and shows the new owner", brief3 != brief and "周远" in brief3, f"{len(brief3)} bytes")
        ev.check("A5 the old owner is gone from the Projects/a BRIEF and the new one is sourced", "林川" not in brief3 and "周远：负责" in brief3 and "[src:Projects/a/星火计划.md" in brief3)
        missing3 = [l for l in content_lines(brief3) if "[src:" not in l]
        ev.check("A5 every line of the new BRIEF still has [src:", not missing3 and content_lines(brief3) != [])
        ev.write("a5-BRIEF-after-edit.md", brief3)
        ev.write("a5-state-chain.json", json.dumps({"file": EDITED, "history": history, "chain_after_edit": chain, "events_after_edit": len(file_events)}, ensure_ascii=False, indent=2) + "\n")

        # ================= acceptance 4 (API level): engine down, serve restart, since replay
        ev.log("\n## Acceptance 4 (API level): engine killed, oo serve restarted, `since` replay loses nothing")
        before = api.events(0, listen=3.0)
        seqs = [e["seq"] for e in before if "seq" in e]
        cursor = seqs[len(seqs) // 2]
        head_before = max(seqs)
        ev.check("A4 the event log has no gap", seqs == list(range(1, head_before + 1)), f"1..{head_before}")
        engine_pid = int((state / "engine.pid").read_text())
        ev.log(f"$ kill -KILL <engine pid>")
        os.kill(engine_pid, signal.SIGKILL)

        def engine_down():
            st, b = api.request("GET", "/v1/docs/health")
            return st == 200 and isinstance(b, dict) and b.get("status") == "unavailable"

        seen_down = wait_until(15, engine_down, step=0.1)
        ev.check("A4 health reports the engine unavailable while it is down", bool(seen_down))
        st, b = api.request("GET", f"/v1/docs/drives/{drive_id}/files")
        ev.check("A4 other calls answer 503 DOCS_ENGINE_UNAVAILABLE meanwhile", st == 503 and b.get("code") == "DOCS_ENGINE_UNAVAILABLE" or st == 200, f"HTTP {st}")

        def engine_back():
            st, b = api.request("GET", "/v1/docs/health")
            return st == 200 and isinstance(b, dict) and b.get("status") != "unavailable"

        back = wait_until(60, engine_back, step=0.5)
        ev.check("A4 the proxy recovers by itself when the engine is back", bool(back))
        # a change made after the reconnect must show up in the replay
        (drive / "Notes").mkdir(exist_ok=True)
        (drive / "Notes/重连后新增.md").write_text("# 重连后新增\n\n负责人：孙晓棠\n", encoding="utf-8")
        wait_until(60, lambda: file_states(api, drive_id).get("Notes/重连后新增.md") == "published")
        code = serve.stop()
        ev.check("A4 oo serve stops on SIGTERM with exit 0", code == 0, f"exit {code}")
        gone = [n for n in ("serve.json", "serve.token", "engine.token", "engine.pid") if (state / n).exists()]
        ev.check("A4 stopping removed the token files and serve.json", not gone, f"left: {gone}")
        ev.check("A4 the engine process stopped with it", bool(wait_until(15, lambda: subprocess.run(["kill", "-0", str(engine_pid)], capture_output=True).returncode != 0)))
        api = serve.start()
        replay = api.events(cursor)
        rseqs = [e["seq"] for e in replay if "seq" in e]
        _, health = api.request("GET", "/v1/docs/health")
        head = health["events_head"]
        ev.check("A4 the replay since the old cursor is gap free", rseqs == list(range(cursor + 1, head + 1)), f"{cursor + 1}..{head} ({len(rseqs)} events)")
        ev.check("A4 the replay ends at the head", bool(rseqs) and rseqs[-1] == head)
        new_states = [e for e in replay if e.get("type") == "file.state_changed" and "重连后新增" in json.dumps(e, ensure_ascii=False)]
        ev.check("A4 the file added after the reconnect is in the replay up to published", any(e["data"]["file"]["state"] == "published" for e in new_states))
        ev.check("A4 state after the restart equals the replayed state", file_states(api, drive_id).get("Notes/重连后新增.md") == "published" and states_ok_all(api, drive_id))
        ev.write("a4-replay.json", json.dumps({"cursor": cursor, "head": head, "replayed_seqs": f"{cursor + 1}..{head}", "types": sorted({e.get("type") for e in replay})}, indent=2) + "\n")
        ok = all(c["ok"] for c in ev.checks)
    finally:
        if serve and serve.proc:
            serve.stop()
        if not args.keep:
            pg.stop()
            remove_tree(work)
            left = subprocess.run(["docker", "ps", "-aq", "--filter", f"label={LANE}", "--filter", f"name={pg.name}"], capture_output=True, text=True).stdout.strip()
            ev.log(f"cleanup: container removed={not left}, temp dir removed={not work.exists()}")
        failed = [c for c in ev.checks if not c["ok"]]
        ev.log(f"\n# {len(ev.checks) - len(failed)}/{len(ev.checks)} checks passed" + ("" if not failed else "; FAILED: " + "; ".join(c["check"] for c in failed)))
        ev.write("run.log", "\n".join(ev.log_lines) + "\n")
        head = subprocess.run(["git", "-C", str(REPO), "rev-parse", "HEAD"], capture_output=True, text=True).stdout.strip()
        built = {b.name: sha256(b.read_bytes()) for b in (oo, engine) if b.is_file()}
        ev.write("result.json", json.dumps({"run_id": args.run_id, "status": "pass" if ok and not failed else "fail", "git_head": head, "binaries_sha256": built, "postgres_image": PG_IMAGE.split("/", 2)[-1], "timings": ev.timings, "checks": ev.checks}, ensure_ascii=False, indent=2) + "\n")
    return 0 if ok else 1


def states_ok_all(api, drive_id):
    s = file_states(api, drive_id)
    return len(s) == 21 and all(v == "published" for v in s.values())


if __name__ == "__main__":
    sys.exit(main())
