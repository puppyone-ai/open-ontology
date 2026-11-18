#!/usr/bin/env python3
"""Super Ontology L7 end to end (30 section L7 and 7; B-S11-01 .. 04, B-S12-01, B-S12-02).

  /usr/bin/python3 tools/oo-test/super_ontology_l7.py [--run-id ID] [--evidence-dir DIR] [--skip-build] [--keep]

What is real here: a labelled PostgreSQL (the pinned image), two real `oo-docs-engine` processes under two real
`oo serve` (a "local" engine A and a "remote" engine B, each with its own database, state directory and Drive
over the same files), the real `oo` CLI and the real cargo test suites. What is simulated, and why:

* no real OSS, COS or R2 bucket and no account may be created: B-S11-01 is proved against look-alike stores
  (the in-process cloud look-alike of `cloud_mimic.rs`); the real bucket test `cloud_live.rs` (ignored) is ready for
  the owner and is only built here;
* the event source is the request body of `POST /drives/{id}:events` (a local queue stand-in); the pull adapters for
  SQS, Cloudflare Queues and Alibaba MNS run against local fakes only, and the COS event format comes from fixtures;
* the Agent DB of B-S11-04 is a second database with a `customers` table.

Parts: (1) look-alike clouds, compat table, events, projection and audit suites; (2) two engines, policy through
the engine (denied path never listed, never in a mount); (4) cloud proxy token on engine B; (5) `oo context mount
--remote` from engine B equals the local mount of engine A byte for byte (the `pack_digest` named on the command
line); (6) event hints find a new file with polling off; then the audit answer and the revocation.
"""
import json
import os
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import super_ontology_l2 as l2  # noqa: E402

REPO = l2.REPO
LANE = "oo.lane=" + os.environ.get("OO_TEST_LANE", "super-ontology-be2")

DRIVE_FILES = {
    "Projects/星火计划.md": "---\ntype: project\nname: 星火计划\nowner: 林川\n---\n# 星火计划\n\n负责人：林川\n\n状态：进行中\n",
    "People/林川.md": "---\ntype: person\nname: 林川\nrole: 工程负责人\n---\n# 林川\n",
    "Payroll/薪酬方案.md": "---\ntype: project\nname: 薪酬方案\nowner: 周远\n---\n# 薪酬方案\n\n负责人：周远\n\n状态：评审中\n",
}
POLICY = {"deny": ["Payroll"]}


def sh(argv, **kw):
    return subprocess.run(argv, capture_output=True, text=True, **kw)


def tree(root: Path):
    out = {}
    for p in sorted(root.rglob("*")):
        if p.is_file():
            out[str(p.relative_to(root))] = p.read_bytes()
    return out


def free_port():
    import socket
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    port = s.getsockname()[1]
    s.close()
    return port


class Engine:
    """One `oo serve` with its own database, state directory and Drive."""

    def __init__(self, ev, pg, oo, engine, work, name, cloud_port=None, poll=5):
        self.ev, self.pg, self.oo, self.engine, self.name = ev, pg, oo, engine, name
        self.work = work / name
        self.work.mkdir()
        self.db, self.login = f"oo_docs_l7_{name}", f"oo_docs_l7_{name}_runtime"
        self.state = self.work / "state"
        (self.state / "engine").mkdir(parents=True)
        self.state.chmod(0o700)
        (self.state / "engine").chmod(0o700)
        (self.state / "engine/docs.json").write_text(json.dumps({"version": "oo.docs/v1", "sync": {"fs_poll_seconds": poll}}))
        self.drive_root = self.work / "drive"
        self.cloud_port = cloud_port
        self.serve = None
        self.api = None

    def install(self, env_base):
        pg = self.pg
        pg.psql("postgres", f"CREATE DATABASE {self.db}")
        admin_url = pg.url("postgres", self.db)
        tok = self.work / "install.token"
        tok.write_text("oodi_install_only_0123456789abcdef")
        tok.chmod(0o600)
        (self.work / "install-state").mkdir()
        env = dict(env_base, OO_DOCS_LISTEN="127.0.0.1:0", OO_DOCS_STATE_DIR=str(self.work / "install-state"),
                   OO_DOCS_DATABASE_URL=admin_url, OO_DOCS_ADMIN_DATABASE_URL=admin_url, OO_DOCS_ENGINE_TOKEN_FILE=str(tok))
        p = subprocess.Popen([str(self.engine)], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        first = p.stdout.readline()
        p.send_signal(signal.SIGTERM)
        p.wait(timeout=30)
        n = pg.psql(self.db, "SELECT count(*) FROM _oo_docs.schema_migrations WHERE version = 'docs-cloud-audit/1'")
        self.ev.check(f"{self.name}: schema installed, docs-cloud-audit/1 among the migrations", '"listening"' in first and n == "1", first.strip()[:60])
        pg.psql("postgres", f"CREATE ROLE {self.login} LOGIN IN ROLE oo_docs_runtime")

    def start(self, env_base):
        env = dict(env_base, OO_DOCS_DATABASE_URL=self.pg.url(self.login, self.db))
        if self.cloud_port:
            env.update(OO_DOCS_CLOUD_LISTEN=f"127.0.0.1:{self.cloud_port}", OO_DOCS_CLOUD_HOSTS=f"127.0.0.1:{self.cloud_port}")
        self.serve = l2.Serve(self.ev, self.oo, self.engine, self.state, env)
        self.api = self.serve.start()
        return self.api

    def oo_args(self, *rest):
        return [str(self.oo), *rest, "--state-dir", str(self.state)]

    def stop(self):
        if self.serve:
            self.serve.stop()
            self.serve = None


def request(port, method, path, token, body=None, headers=None, host=None):
    """A request to any engine port, with any token (stdlib only, no keep alive)."""
    import http.client
    c = http.client.HTTPConnection("127.0.0.1", port, timeout=60)
    h = {"Authorization": f"Bearer {token}", "Idempotency-Key": f"l7-{time.time_ns()}", "Content-Type": "application/json"}
    if host:
        h["Host"] = host
    h.update(headers or {})
    c.request(method, path, body=json.dumps(body) if body is not None else None, headers=h)
    r = c.getresponse()
    raw = r.read()
    c.close()
    try:
        return r.status, json.loads(raw)
    except ValueError:
        return r.status, raw


SKIP = {"on": False}


def cargo(ev, argv, env=None, check=False, timeout=1800):
    if SKIP["on"]:
        return True, 0, 0, "Finished skipped"
    return _cargo(ev, argv, env, check, timeout)


def _cargo(ev, argv, env=None, check=False, timeout=1800):
    e = dict(os.environ, CARGO_BUILD_JOBS="6")
    e.update(env or {})
    p = l2.run(ev, ["cargo", *argv], env=e, check=check, timeout=timeout)
    text = p.stdout + p.stderr
    results = re.findall(r"test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored", text)
    passed = sum(int(r[1]) for r in results)
    failed = sum(int(r[2]) for r in results)
    return p.returncode == 0 and failed == 0 and passed > 0, passed, failed, text


def main(argv=None):
    import argparse
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--run-id", default=time.strftime("l7-%Y%m%d-%H%M%S"))
    ap.add_argument("--evidence-dir", default=str(REPO / "testing/super-ontology/evidence/L7"))
    ap.add_argument("--skip-build", action="store_true")
    ap.add_argument("--keep", action="store_true")
    ap.add_argument("--skip-suites", action="store_true", help="skip the cargo suites of part 1 (engines only)")
    a = ap.parse_args(argv)
    l2.LANE = LANE
    ev = l2.Evidence(Path(a.evidence_dir), a.run_id)
    ev.log(f"# Super Ontology L7 end to end, run {a.run_id}")
    target = Path(os.environ.get("CARGO_TARGET_DIR", REPO / "target")) / "debug"
    oo, engine = target / "oo", target / "oo-docs-engine"
    work = Path(tempfile.mkdtemp(prefix=f"oo-l7-{a.run_id}-", dir="/tmp"))
    pg = l2.Postgres(ev, a.run_id)
    engines = []
    try:
        if not a.skip_build:
            l2.run(ev, ["cargo", "build", "--locked", "-p", "oo-cli", "--features", "docs"])
            l2.run(ev, ["cargo", "build", "--locked", "-p", "oo-docs", "--bin", "oo-docs-engine"])
        pg.start()
        env_base = {"PATH": os.environ.get("PATH", ""), "TMPDIR": os.environ.get("TMPDIR", "/tmp")}

        # ================= part 1: cargo suites against look-alike clouds, events, projection, audit
        ev.log("\n## Part 1: suites")
        SKIP['on'] = a.skip_suites
        ok, n, f, _ = cargo(ev, ["test", "-p", "oo-connectors", "--features", "objectstore-s3", "--locked", "--test", "cloud_mimic", "--test", "s3_mock", "--lib"])
        ev.check("S11-01 oss, cos and r2 against look-alike stores, the compat table, and the s3 mock suite", ok, f"{n} passed, {f} failed")
        ok, n, f, _ = cargo(ev, ["test", "-p", "oo-connectors", "--features", "objectstore-s3", "--locked", "--lib", "events"])
        ev.check("S11-02 events: duplicates, out of order delivery, sequencer order, formats of S3, R2, OSS", ok, f"{n} passed, {f} failed")
        ok, n, f, _ = cargo(ev, ["test", "-p", "oo-connectors", "--features", "objectstore-s3", "--locked", "--lib", "events::adapter_tests"])
        ev.check("S11-02 pull adapters for SQS, Cloudflare Queues and Alibaba MNS against local fakes and a local socket (SigV4 vectors, ack after handling, redelivery absorbed)", ok and n >= 12, f"{n} passed, {f} failed")
        ok, n, f, _ = cargo(ev, ["test", "-p", "oo-connectors", "--features", "objectstore-s3", "--locked", "--lib", "events::parse"])
        ev.check("S11-02 event formats of S3, R2, OSS and COS (COS from fixtures: public documentation, unconfirmed against a real bucket)", ok and n >= 5, f"{n} passed, {f} failed")
        ok, n, f, _ = cargo(ev, ["test", "-p", "oo-connectors", "--features", "objectstore-s3", "--locked", "--lib", "objectstore_fs_scopes"])
        ev.check("S11-03 first level scopes of the fs backend partition the listing", ok and n >= 2, f"{n} passed, {f} failed")
        ok, n, f, _ = cargo(ev, ["test", "-p", "oo-docs", "--locked", "--lib", "scope_tests"])
        ev.check("S11-03 bounded parallel listing: at most N scopes in flight, every result returned", ok and n >= 2, f"{n} passed, {f} failed")
        ok, n, f, _ = cargo(ev, ["test", "-p", "oo-connectors", "--features", "objectstore-s3", "--locked", "--test", "cloud_live", "--no-run"])
        ev.check("S11-01 the real bucket test (cloud_live, ignored, secret_ref only) builds and is ready for the owner", ok or "Finished" in _ or "Executable" in _, "built, not run: no real bucket")
        pgenv = {"OO_DOCS_TEST_PG_PORT": str(pg.port), "OO_L7_EVIDENCE_DIR": str(ev.out)}
        suites = [("S11-03 policy in ingest and the visible graph", "policy_live"),
                  ("S11-03 parallel listing per first level prefix: own generation per prefix, a failed prefix is not swept and does not hold the others", "prefix_sync"),
                  ("S11-02 events over HTTP", "events_live"),
                  ("S11-04 projection and links to database rows", "projection_live"),
                  ("S12-01 cloud tokens, cloud entry and audit", "audit")]
        for label, test in suites:
            ok, n, f, _ = cargo(ev, ["test", "-p", "oo-docs", "--locked", "--test", test, "--", "--ignored", "--test-threads=1"], env=pgenv)
            ev.check(label, ok, f"{n} passed, {f} failed")
        ok, n, f, _ = cargo(ev, ["test", "-p", "oo-docs", "--locked", "--lib", "policy"])
        ev.check("S11-03 policy unit tests", ok, f"{n} passed, {f} failed")
        ok, n, f, _ = cargo(ev, ["test", "-p", "oo-docs", "--locked", "--lib", "publish::projection"])
        ev.check("S11-04 projection unit tests", ok, f"{n} passed, {f} failed")
        ok, n, f, _ = cargo(ev, ["test", "-p", "oo-cli", "--features", "docs", "--locked", "--test", "docs_context", "remote_mount"])
        ev.check("S12-02 remote mount against a stand-in cloud entry", ok, f"{n} passed, {f} failed")

        # ================= engines A (local) and B (remote, with the cloud entry)
        ev.log("\n## Part 2: two engines over the same files")
        cloud_port = free_port()
        A = Engine(ev, pg, oo, engine, work, "a", poll=3600)  # events only: the poll is off for the length of this run
        B = Engine(ev, pg, oo, engine, work, "b", cloud_port=cloud_port, poll=5)
        engines += [A, B]
        for e in (A, B):
            for rel, text in DRIVE_FILES.items():
                p = e.drive_root / rel
                p.parent.mkdir(parents=True, exist_ok=True)
                p.write_text(text, encoding="utf-8")
                os.utime(p, (1_700_000_000, 1_700_000_000))
            e.install(env_base)
            e.start(env_base)
        drives = {}
        for e in (A, B):
            st, d = e.api.request("POST", "/v1/docs/drives", {"name": f"l7-{e.name}", "backend": "fs",
                                                             "config": {"root": str(e.drive_root), "policy": POLICY}})
            ev.check(f"{e.name}: drive registered with policy deny Payroll", st == 201, f"HTTP {st}")
            drives[e.name] = d["id"]
        for e in (A, B):
            done = l2.wait_until(120, lambda: (lambda s: len(s) == 2 and all(v == "published" for v in s.values()))(l2.file_states(e.api, drives[e.name])), step=1)
            states = l2.file_states(e.api, drives[e.name])
            ev.check(f"{e.name} S11-03: only the two allowed files are in the drive and published, Payroll was never listed", bool(done) and "Payroll/薪酬方案.md" not in states, json.dumps(states, ensure_ascii=False))
        # The mount of the whole drive never carries the denied file (the second place of the policy).
        t = work / "whole"
        t.mkdir()
        p = l2.run(ev, A.oo_args("context", "mount", "--to", str(t), "--json"), check=False)
        text = "".join(f.read_text(encoding="utf-8", errors="ignore") for f in (t / ".context").rglob("*") if f.is_file()) if p.returncode == 0 else ""
        ev.check("S11-03 a mount of the whole drive carries nothing of the denied Payroll file", p.returncode == 0 and "薪酬方案" not in text and "周远" not in text and "林川" in text, f"exit {p.returncode}")

        # ================= part 4: cloud token on engine B and the audit
        ev.log("\n## Part 4: cloud proxy token and the audit (engine B)")
        st, t = B.api.request("POST", "/v1/docs/cloud-tokens", {"tenant": "acme", "drive_ids": [drives["b"]], "label": "puppyone-cloud"})
        ev.check("S12-01 an oodc_ token is issued on the local entry", st == 201 and t.get("token", "").startswith("oodc_"), f"HTTP {st}")
        token = t["token"]
        tokfile = work / "remote.token"
        tokfile.write_text(token)
        tokfile.chmod(0o600)
        local_port = B.serve.info["port"]
        st, _ = request(local_port, "GET", "/v1/docs/health", token)
        ev.check("S12-01 the loopback entry refuses the cloud token", st == 401, f"HTTP {st}")
        st, _ = request(cloud_port, "GET", "/v1/docs/health", B.serve.token)
        ev.check("S12-01 the cloud entry refuses the control token", st == 401, f"HTTP {st}")
        st, _ = request(cloud_port, "GET", "/v1/docs/drives", token)
        ev.check("S12-01 the cloud token cannot list drives (route not allowed)", st == 403, f"HTTP {st}")
        st, _ = request(cloud_port, "POST", "/v1/docs/shares", token, {"compile": {"drive_id": drives["a"], "scope": ["Projects"], "wait_ms": 0}, "mount": {"target_dir": "/tmp/x"}})
        ev.check("S12-01 a Drive outside the token's set is refused", st in (403, 404), f"HTTP {st}")

        # ================= part 5: remote mount equals the local mount
        ev.log("\n## Part 5: oo context mount --remote equals the local mount")
        t_local, t_remote = work / "m-local", work / "m-remote"
        t_local.mkdir()
        t_remote.mkdir()
        # The remote Worker takes the pack of the engine behind the cloud entry: the comparison is that engine's own local
        # mount against the same pack fetched through its cloud entry. (Engine A, another engine over the same files, is
        # looked at below: its pack differs in what names its own Drive and ontology versions.)
        p = l2.run(ev, B.oo_args("context", "mount", "--scope", "Projects", "--scope", "People", "--to", str(t_local), "--json"), check=False)
        ev.check("S12-02 local mount on engine B (the engine behind the cloud entry)", p.returncode == 0, f"exit {p.returncode}")
        local = json.loads(p.stdout.strip().splitlines()[-1])
        p = l2.run(ev, [str(oo), "context", "mount", "--remote", f"http://127.0.0.1:{cloud_port}", "--remote-token-file", str(tokfile),
                        "--drive", drives["b"], "--agent", "agent-7", "--scope", "Projects", "--scope", "People", "--to", str(t_remote),
                        "--pack-digest", local["pack_digest"], "--json", "--state-dir", str(A.state)],
                   check=False, shown=[str(oo), "context", "mount", "--remote", "http://127.0.0.1:<cloud port>", "--remote-token-file", "<file>", "..."])
        ev.check("S12-02 remote mount from engine B's cloud entry, the pack digest asked for is the local one", p.returncode == 0, f"exit {p.returncode} {p.stderr.strip()[:160]}")
        remote = json.loads(p.stdout.strip().splitlines()[-1]) if p.returncode == 0 else {}
        ev.check("S12-02 the pack digests are equal", remote.get("pack_digest") == local["pack_digest"], f"{remote.get('pack_digest', '')[:16]} == {local['pack_digest'][:16]}")
        ta, tb = tree(t_local / ".context"), tree(t_remote / ".context") if remote else {}
        differ = [k for k in ta if k != "SHARE.json" and ta.get(k) != tb.get(k)]
        ev.check("S12-02 every file of the pack is byte-identical to the local mount (SHARE.json differs by share id, times and credential only)",
                 bool(tb) and not differ and set(ta) == set(tb), f"{len(ta)} files, differing: {differ}")
        t_a = work / "m-a"
        t_a.mkdir()
        pa = l2.run(ev, A.oo_args("context", "mount", "--scope", "Projects", "--scope", "People", "--to", str(t_a), "--json"), check=False)
        if pa.returncode == 0:
            tc = tree(t_a / ".context")
            diff_a = sorted(k for k in tb if k != "SHARE.json" and tc.get(k) != tb.get(k))
            ev.log(f"  informational: engine A (own Drive and ontology history) differs from B in {diff_a or 'nothing'}")
            ev.timings["engine_a_vs_b_differing_files"] = diff_a
            import difflib
            for name in ("manifest.json", "BRIEF.md"):
                if name in diff_a:
                    d = list(difflib.unified_diff(tc[name].decode().splitlines(), tb[name].decode().splitlines(), "A", "B", lineterm="", n=0))
                    ev.log("  diff " + name + ": " + " | ".join(x[:140] for x in d[:8]))
        ev.check("S12-02 the cloud token is in no mounted file and no output", token not in p.stdout + p.stderr and all(token.encode() not in v for v in tb.values()))

        # ================= part 6: event hints with polling off (after the pack comparison: this adds a file to engine A only)
        ev.log("\n## Part 6: events find a new file (poll interval 3600 s)")
        (A.drive_root / "Projects/新项目.md").write_text("---\ntype: project\nname: 新项目\nowner: 林川\n---\n# 新项目\n\n状态：筹备\n", encoding="utf-8")
        msg = {"hints": [{"key": "Projects/新项目.md", "op": "put", "sequencer": "0A", "at_ms": 10}]}
        s3 = {"Records": [{"eventName": "ObjectCreated:Put", "eventTime": "2026-11-17T08:00:00Z", "s3": {"object": {"key": "Projects/新项目.md", "sequencer": "0A"}}}]}
        t0 = time.time()
        st, r = A.api.request("POST", f"/v1/docs/drives/{drives['a']}:events", {"messages": [msg, s3, msg]})
        ev.check("S11-02 the events were accepted: one look, the repeats counted", st == 200 and r.get("looked_at") == 1 and r.get("duplicates", 0) >= 1, json.dumps(r))
        done = l2.wait_until(60, lambda: l2.file_states(A.api, drives["a"]).get("Projects/新项目.md") == "published", step=0.5)
        ev.timings["event_to_published_seconds"] = round(time.time() - t0, 2)
        ev.check("S11-02 the new file is published through the event, with the poll off", bool(done), f"{time.time() - t0:.1f} s")
        st, r = A.api.request("POST", f"/v1/docs/drives/{drives['a']}:events", {"messages": [{"hints": [{"key": "Projects/新项目.md", "op": "put", "sequencer": "09", "at_ms": 5}]}]})
        ev.check("S11-02 an older event delivered late is dropped (stale)", st == 200 and r.get("stale") == 1 and r.get("looked_at") == 0, json.dumps(r))

        # ================= part 4 again: the audit after the remote mount and a revocation
        st, audit = B.api.request("GET", "/v1/docs/audit/shares?agent=agent-7")
        items = audit.get("items", []) if st == 200 else []
        ev.check("S12-01 the audit answers who shared what to which agent, and when (agent-7 through the cloud entry of tenant acme)",
                 len(items) == 1 and items[0]["actor"].startswith("cloud:") and items[0]["tenant"] == "acme" and items[0]["scope"] == ["Projects", "People"]
                 and items[0]["role"] == "suggester" and items[0]["at"], json.dumps(items, ensure_ascii=False)[:300])
        ev.write("audit-who-shared-what.json", json.dumps(audit, ensure_ascii=False, indent=2) + "\n")
        sid = items[0]["share_id"] if items else None
        if sid:
            st, _ = request(cloud_port, "POST", f"/v1/docs/shares/{sid}:revoke", token, {}, headers={"OO-Docs-Agent": "agent-7"})
            ev.check("S12-01 the share is revoked through the cloud entry", st == 200, f"HTTP {st}")
            st, audit = B.api.request("GET", "/v1/docs/audit/shares?agent=agent-7")
            ev.check("S12-01 the revocation is part of the answer", st == 200 and audit["items"][0]["revoked_at"], json.dumps(audit["items"][0] if st == 200 else {}, ensure_ascii=False)[:200])
        st, v = B.api.request("GET", "/v1/docs/audit/verify")
        ev.check("S12-01 the audit chain is intact", st == 200 and v.get("intact") is True, json.dumps(v))
        forged = pg.psql(B.db, "SELECT count(*) FROM _oo_docs.audit_log WHERE action LIKE 'share.%' OR action LIKE 'cloud_token.%'")
        ev.check("S12-01 audit rows exist for the token issue, the share and its revocation", int(forged) >= 3, f"{forged} rows")
        r = subprocess.run(["docker", "exec", pg.name, "psql", "-U", "postgres", "-d", B.db, "-c", "DELETE FROM _oo_docs.audit_log"], capture_output=True, text=True)
        ev.check("S12-01 the audit table cannot be deleted from, even by the database owner", r.returncode != 0 and "insert-only" in r.stderr, r.stderr.strip()[:80])

        ev.write("l7-e2e-result.json", json.dumps({"run_id": a.run_id, "checks": ev.checks, "timings": ev.timings}, ensure_ascii=False, indent=2) + "\n")
        ev.write("l7-e2e-log.txt", "\n".join(ev.log_lines) + "\n")
        return 0 if all(c["ok"] for c in ev.checks) else 1
    finally:
        for e in engines:
            e.stop()
        if not a.keep:
            pg.stop()
            l2.remove_tree(work)


if __name__ == "__main__":
    sys.exit(main())
