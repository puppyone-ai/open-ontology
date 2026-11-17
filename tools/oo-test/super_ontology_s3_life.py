#!/usr/bin/env python3
"""Super Ontology L3 acceptance 5, the s3 round (30 section L3 acceptance 5, B-S4-02/04).

A real `oo serve` (host engine built with `objectstore-s3`) over an `s3` Drive
on a RustFS or MinIO container, and a Postgres container. The Drive's
credential is a `secret_ref` (`file:/abs/path`, 0600): the config the engine
stores has no key. The round, in the order of the spec:

  add          a markdown object appears, is published, has live evidence
  modify       same size and same mtime second, other content (a store that
               tells changes by tag must still see it): new version published,
               the old version's evidence withdrawn, the new live
  rename       copy + delete: same content under a new key is a rename
               (file.renamed), evidence moves, no fact is retracted
  delete       the object goes: file deleted, evidence withdrawn, a fact left
               without evidence is retracted
  cache        the original of a remote object is in the CAS (`orig/`)
  secrets      the engine log, the config row and the API carry no key

Usage: tools/oo-test/super_ontology_s3_life.py [--store rustfs|minio]
       [--run-id ID] [--evidence-dir DIR] [--skip-build] [--keep]
Evidence: testing/super-ontology/evidence/L3/s3-life-<store>.md
"""

import argparse
import datetime
import hashlib
import hmac
import json
import os
import re
import secrets
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import super_ontology_l2 as l2  # noqa: E402
import super_ontology_s3 as s3cells  # noqa: E402

REPO = l2.REPO
LANE = "super-ontology-be1"


def sign_and_send(endpoint, access, secret, method, path, body=b"", extra=None, region="us-east-1"):
    host = endpoint.split("//", 1)[1]
    now = datetime.datetime.now(datetime.timezone.utc)
    amz, day = now.strftime("%Y%m%dT%H%M%SZ"), now.strftime("%Y%m%d")
    payload = hashlib.sha256(body).hexdigest()
    headers = {"host": host, "x-amz-content-sha256": payload, "x-amz-date": amz, **(extra or {})}
    signed = ";".join(sorted(headers))
    canon_uri = urllib.request.quote(path, safe="/-_.~")
    canon = "\n".join([method, canon_uri, "", *[f"{k}:{headers[k]}" for k in sorted(headers)], "", signed, payload])
    scope = f"{day}/{region}/s3/aws4_request"
    to_sign = "\n".join(["AWS4-HMAC-SHA256", amz, scope, hashlib.sha256(canon.encode()).hexdigest()])
    k = s3cells.hmac_sha(("AWS4" + secret).encode(), day)
    for part in (region, "s3", "aws4_request"):
        k = s3cells.hmac_sha(k, part)
    sig = hmac.new(k, to_sign.encode(), hashlib.sha256).hexdigest()
    auth = f"AWS4-HMAC-SHA256 Credential={access}/{scope}, SignedHeaders={signed}, Signature={sig}"
    req = urllib.request.Request(endpoint + canon_uri, method=method, data=body if method == "PUT" else None,
                                 headers={**headers, "Authorization": auth})
    return urllib.request.urlopen(req, timeout=30).read()


class Store:
    def __init__(self, endpoint, access, secret, bucket):
        self.endpoint, self.access, self.secret, self.bucket = endpoint, access, secret, bucket

    def put(self, key, body):
        sign_and_send(self.endpoint, self.access, self.secret, "PUT", f"/{self.bucket}/{key}", body)

    def delete(self, key):
        sign_and_send(self.endpoint, self.access, self.secret, "DELETE", f"/{self.bucket}/{key}")

    def copy(self, src, dst):
        sign_and_send(self.endpoint, self.access, self.secret, "PUT", f"/{self.bucket}/{dst}", b"",
                      {"x-amz-copy-source": urllib.request.quote(f"/{self.bucket}/{src}", safe="/-_.~")})


def doc(state):
    return f"---\ntype: project\nname: 对象存储项目\nowner: 林川\n---\n# 对象存储项目\n\n负责人：林川\n\n状态：{state}\n"


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--store", choices=("rustfs", "minio"), default="rustfs")
    ap.add_argument("--run-id", default=time.strftime("s3l-%Y%m%d-%H%M%S"))
    ap.add_argument("--evidence-dir", default=str(REPO / "testing/super-ontology/evidence/L3"))
    ap.add_argument("--skip-build", action="store_true")
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    cell = s3cells.CELLS["docs-" + args.store]
    ev = l2.Evidence(Path(args.evidence_dir), args.run_id)
    target = Path(os.environ.get("CARGO_TARGET_DIR", REPO / "target")) / "debug"
    oo, engine = target / "oo", target / "oo-docs-engine"
    work = Path(tempfile.mkdtemp(prefix=f"oo-s3l-{args.run_id}-", dir="/tmp"))
    l2.LANE = f"oo.lane={LANE}"
    pg = l2.Postgres(ev, args.run_id)
    pg.name = f"oo-be1-pg-{args.run_id}"
    cname = f"oo-be1-s3l-{args.run_id}"[:60]
    vols = [f"{cname}-v{i}" for i, _ in enumerate(cell["mounts"])]
    access, secret = "ootest" + secrets.token_hex(4), secrets.token_urlsafe(24)
    serve = None
    ok = False
    ev.log(f"# s3 round of acceptance 5 on {cell['note']}, run {args.run_id}")
    try:
        if not args.skip_build:
            l2.run(ev, ["cargo", "build", "--locked", "-p", "oo-cli", "--features", "docs"])
            l2.run(ev, ["cargo", "build", "--locked", "-p", "oo-docs", "--features", "objectstore-s3", "--bin", "oo-docs-engine"])
        for b in (oo, engine):
            if not b.is_file():
                raise SystemExit(f"missing binary {b}")
        # ---- the store
        for v in vols:
            subprocess.run(["docker", "volume", "create", "--label", f"oo.lane={LANE}", v], check=True, capture_output=True)
        cmd = ["docker", "run", "-d", "--name", cname, "--label", f"oo.lane={LANE}", "-p", "127.0.0.1::9000"]
        for k, v in cell["env"](access, secret).items():
            cmd += ["-e", f"{k}={v}"]
        for v, m in zip(vols, cell["mounts"]):
            cmd += ["-v", f"{v}:{m}"]
        subprocess.run(cmd + [cell["image"], *cell["args"]], check=True, capture_output=True)
        port = subprocess.check_output(["docker", "port", cname, "9000/tcp"], text=True).strip().splitlines()[0].rsplit(":", 1)[1]
        endpoint = f"http://127.0.0.1:{port}"
        ev.check("the store is up", s3cells.wait_ready(endpoint), cell["image"].split("@")[0])
        s3cells.make_bucket(endpoint, access, secret, "oo-s3l")
        store = Store(endpoint, access, secret, "oo-s3l")

        # ---- postgres, schema, engine, serve (as in L2)
        pg.start()
        db, login = "oo_docs_s3l", "oo_docs_s3l_runtime"
        pg.psql("postgres", f"CREATE DATABASE {db}")
        admin_url = pg.url("postgres", db)
        state = work / "state"
        (state / "engine").mkdir(parents=True)
        state.chmod(0o700)
        (state / "engine").chmod(0o700)
        (state / "engine/docs.json").write_text(json.dumps({"version": "oo.docs/v1", "sync": {"fs_poll_seconds": 5}}))
        tokfile = work / "install.token"
        tokfile.write_text("oodi_install_only_0123456789abcdef")
        tokfile.chmod(0o600)
        cred = work / "s3-credential.json"
        cred.write_text(json.dumps({"access_key_id": access, "secret_access_key": secret}))
        cred.chmod(0o600)
        env_base = {"PATH": os.environ.get("PATH", ""), "TMPDIR": os.environ.get("TMPDIR", "/tmp")}
        inst_env = dict(env_base, OO_DOCS_LISTEN="127.0.0.1:0", OO_DOCS_STATE_DIR=str(work / "install-state"),
                        OO_DOCS_DATABASE_URL=admin_url, OO_DOCS_ADMIN_DATABASE_URL=admin_url,
                        OO_DOCS_ENGINE_TOKEN_FILE=str(tokfile))
        (work / "install-state").mkdir()
        inst = subprocess.Popen([str(engine)], env=inst_env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        first = inst.stdout.readline()
        inst.send_signal(signal.SIGTERM)
        inst.wait(timeout=30)
        ev.check("schema installed", '"listening"' in first, first.strip()[:60])
        pg.psql(db, f"CREATE ROLE {login} LOGIN IN ROLE oo_docs_runtime")
        serve = l2.Serve(ev, oo, engine, state, dict(env_base, OO_DOCS_DATABASE_URL=pg.url(login, db)))
        api = serve.start()

        def psql(sql):
            return pg.psql(db, sql)

        # ---- the Drive: config with a secret_ref and nothing else secret
        config = {"bucket": "oo-s3l", "prefix": "drive", "endpoint": endpoint, "path_style": True,
                  "region": "us-east-1", "secret_ref": f"file:{cred}"}
        st, bad = api.request("POST", "/v1/docs/drives", {"name": "s3-inline", "backend": "s3",
                                                         "config": {**config, "access_key_id": access}})
        ev.check("a Drive with an inline key is refused", st == 400, f"HTTP {st}")
        st, d = api.request("POST", "/v1/docs/drives", {"name": "s3-drive", "backend": "s3", "config": config})
        ev.check("the s3 Drive is registered", st == 201, f"HTTP {st}")
        drive_id = d["id"]

        def sync_now():
            api.request("POST", f"/v1/docs/drives/{drive_id}:sync", {})

        def states():
            return l2.file_states(api, drive_id)

        def wait_state(path, want, deadline=90):
            def f():
                sync_now()
                return states().get(path) == want
            return l2.wait_until(deadline, f, step=1.0)

        def history(path):
            return psql(f"SELECT string_agg(h.to_state, '>' ORDER BY h.seq) FROM _oo_docs.file_state_history h JOIN _oo_docs.file f ON f.id = h.file_id WHERE f.path = '{path}'")

        def live_evidence(path):
            return int(psql(f"SELECT count(*) FROM _oo_docs.evidence e JOIN _oo_docs.file f ON f.current_version_id = e.version_id WHERE f.path = '{path}' AND f.deleted_at IS NULL AND e.withdrawn_at IS NULL"))

        # a bystander, so that deleting the object under test is not an emptied root
        store.put("drive/keep.md", doc("不变").encode())
        ev.check("the bystander is published", wait_state("keep.md", "published"))
        # add
        t0 = time.time()
        store.put("drive/life/a.md", doc("进行中").encode())
        ev.check("add: published", wait_state("life/a.md", "published"), f"{time.time() - t0:.1f}s; {history('life/a.md')}")
        ev.check("add: evidence is live", live_evidence("life/a.md") > 0, f"{live_evidence('life/a.md')} rows")
        b3_1 = psql("SELECT fv.b3 FROM _oo_docs.file f JOIN _oo_docs.file_version fv ON fv.id = f.current_version_id WHERE f.path = 'life/a.md'")
        # modify: same length, other content
        a = doc("进行中").encode()
        b = doc("已完成").encode()
        ev.check("modify: the two bodies have the same length", len(a) == len(b), f"{len(a)} bytes")
        store.put("drive/life/a.md", b)
        def modified():
            sync_now()
            v = psql("SELECT fv.b3 FROM _oo_docs.file f JOIN _oo_docs.file_version fv ON fv.id = f.current_version_id WHERE f.path = 'life/a.md'")
            return v != b3_1 and states().get("life/a.md") == "published"
        ev.check("modify: new content published", bool(l2.wait_until(90, modified, step=1.0)), history("life/a.md"))
        old_live = psql(f"SELECT count(*) FROM _oo_docs.evidence e JOIN _oo_docs.file_version v ON v.id = e.version_id JOIN _oo_docs.file f ON f.id = v.file_id WHERE f.path = 'life/a.md' AND v.b3 = '{b3_1}' AND e.withdrawn_at IS NULL")
        ev.check("modify: the old version's evidence is withdrawn, the new is live", old_live == "0" and live_evidence("life/a.md") > 0, f"old {old_live}, new {live_evidence('life/a.md')}")
        # rename = copy + delete
        retracted0 = int(psql("SELECT count(*) FROM _oo_docs.fact WHERE status = 'retracted'"))
        store.copy("drive/life/a.md", "drive/life/b.md")
        store.delete("drive/life/a.md")
        def renamed():
            sync_now()
            s = states()
            return s.get("life/b.md") in ("published", "duplicate") and "life/a.md" not in s
        ev.check("rename: the new key has its result, the old is gone", bool(l2.wait_until(90, renamed, step=1.0)), history("life/b.md"))
        ev.check("rename: evidence is live on the new key", live_evidence("life/b.md") > 0, f"{live_evidence('life/b.md')} rows")
        n_ren = psql("SELECT count(*) FROM _oo_docs.event WHERE type = 'file.renamed'")
        ev.check("rename: a file.renamed event exists", int(n_ren) >= 1, f"{n_ren}")
        retracted1 = int(psql("SELECT count(*) FROM _oo_docs.fact WHERE status = 'retracted'"))
        ev.check("rename: no fact was retracted", retracted1 == retracted0, f"{retracted0} -> {retracted1}")
        # cache
        b3 = psql("SELECT fv.b3 FROM _oo_docs.file f JOIN _oo_docs.file_version fv ON fv.id = f.current_version_id WHERE f.path = 'life/b.md'")
        cached = list((state / "engine").rglob(f"orig/b3/**/{b3}")) + list(state.rglob(f"orig/b3/**/{b3}"))
        ev.check("cache: the original is in the CAS orig/", len(cached) >= 1, f"{len(cached)} object(s)")
        # delete
        store.delete("drive/life/b.md")
        def gone():
            sync_now()
            return "life/b.md" not in states()
        ev.check("delete: the file disappears", bool(l2.wait_until(90, gone, step=1.0)))
        row = psql("SELECT (deleted_at IS NOT NULL)::text FROM _oo_docs.file WHERE path = 'life/b.md' ORDER BY state_since DESC LIMIT 1")
        ev.check("delete: the row is marked deleted", row == "true", row)
        live = psql("SELECT count(*) FROM _oo_docs.evidence e JOIN _oo_docs.file f ON f.current_version_id = e.version_id WHERE f.path = 'life/b.md' AND e.withdrawn_at IS NULL")
        ev.check("delete: its evidence is withdrawn", live == "0", f"{live} live rows")
        retracted2 = int(psql("SELECT count(*) FROM _oo_docs.fact WHERE status = 'retracted'"))
        ev.check("delete: facts left without evidence are retracted", retracted2 > retracted1, f"{retracted1} -> {retracted2}")
        # secrets
        cfg_row = psql(f"SELECT config::text FROM _oo_docs.drive WHERE id = '{drive_id}'")
        log_text = (state / "engine.log").read_text(errors="replace") if (state / "engine.log").exists() else ""
        st, listing = api.request("GET", f"/v1/docs/drives/{drive_id}")
        shown = json.dumps(listing)
        ev.check("secrets: no key in the stored config, the API or the engine log",
                 all(secret not in t and access not in t for t in (cfg_row, shown, log_text)),
                 "secret_ref is a file path")
        ok = all(c["ok"] for c in ev.checks)
    finally:
        if serve:
            serve.stop()
        if not args.keep:
            pg.stop()
            subprocess.run(["docker", "rm", "-f", cname], capture_output=True)
            for v in vols:
                subprocess.run(["docker", "volume", "rm", "-f", v], capture_output=True)
            l2.remove_tree(work)
            shutil.rmtree(work, ignore_errors=True)
    n_ok = sum(1 for c in ev.checks if c["ok"])
    ev.log(f"\n# {n_ok}/{len(ev.checks)} checks passed")
    ev.write(f"s3-life-{args.store}.md", "\n".join(ev.log_lines) + "\n")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
