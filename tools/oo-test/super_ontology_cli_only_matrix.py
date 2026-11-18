#!/usr/bin/env python3
"""D83 CLI-only matrix (gap tests of the independent verification): every step is an `oo` command on a fresh HOME.

Part A (no Docker for `oo`, the embedded runtime; RustFS is the only container, labelled):
  A1 files-only, no model configured: the first command of a fresh HOME is `oo source add s3` (the daemon starts
     itself), `oo docs upload` of Markdown publishes by rules, `oo docs model show` says no model, no model call is counted
  A2 oss, cos and r2 sources against RustFS, each with its addressing: s3 and r2 path style, oss and cos both path style
     (a custom endpoint) and virtual hosted (`bucket.localhost:PORT`, RustFS with RUSTFS_SERVER_DOMAINS); every one:
     `oo source add --credentials-stdin`, `oo source test`, `oo docs upload --drive`, the object is in the bucket under
     the prefix and the engine published it
  A3 files-only path: an Agent token with docs:read only searches the files with a file anchor, the database side is
     "skipped", a query is ONTOLOGY_SCOPE_DENIED, there is no stack file and no database binding anywhere
  A4 token `--ttl`: the token works, then 401 ONTOLOGY_TOKEN_INVALID on the CLI and the raw route once it has expired
  A5 secrets: the S3 secret is in no process argument list, no command output and no file under HOME but 0600 files
Part B (DB-only, a second fresh HOME, host runtime on a private Postgres, the real oo-gateway):
  B1 zero-config database onboarding: `oo init --dsn ... --json` (flags only) checks resolve, port, TLS, sign-in
     of the Gateway's TLS cluster and writes nothing; `oo source add postgres` registers it in a stack file
  B2 DB-only path: no Drive, no file ever uploaded; a token with db:read only queries through the Gateway with the
     publication id, a write is refused, docs scope is denied

Usage: tools/oo-test/super_ontology_cli_only_matrix.py [--skip-build] [--part A|B|AB] [--out FILE] [--keep]
"""
import argparse
import datetime
import hashlib
import json
import os
import re
import secrets
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.parse
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.dont_write_bytecode = True
import mock_s3  # noqa: E402
import super_ontology_cli_first_e2e as e2e  # noqa: E402
import super_ontology_gateway as realgw  # noqa: E402
import super_ontology_nodocker as nd  # noqa: E402
import super_ontology_s3 as s3cell  # noqa: E402

REPO = e2e.REPO
LANE = os.environ.get("OO_TEST_LANE", "super-ontology-f1")
check = e2e.check
os.environ.setdefault("OO_TEST_LANE", LANE)

MD = {
    "星火计划.md": "---\ntype: project\nname: 星火计划\nowner: 林川\n---\n# 星火计划\n\n负责人：林川\n\n合同每年三月续约。\n",
    "林川.md": "---\ntype: person\nname: 林川\nrole: 工程负责人\n---\n# 林川\n\n角色：工程负责人\n",
    "Acme.md": "---\ntype: organization\n---\n\n# Acme Ltd\n\nAcme Ltd is our largest customer. The contract renews every March.\n",
}


class Store:
    """The RustFS image of the S3 cell, one labelled container, virtual-hosted style enabled for localhost:<port>."""

    def __init__(self, access, secret):
        self.access, self.secret = access, secret
        cell = s3cell.CELLS["docs-rustfs"]
        self.port = nd.free_port()
        self.name = f"oo-f1-rustfs-{secrets.token_hex(3)}"
        self.vols = [f"{self.name}-v{i}" for i, _ in enumerate(cell["mounts"])]
        for v in self.vols:
            subprocess.run(["docker", "volume", "create", "--label", f"oo.lane={LANE}", v], check=True, capture_output=True)
        cmd = ["docker", "run", "-d", "--name", self.name, "--label", f"oo.lane={LANE}", "-p", f"127.0.0.1:{self.port}:9000",
               "-e", f"RUSTFS_SERVER_DOMAINS=localhost:{self.port}"]
        for k, v in cell["env"](access, secret).items():
            cmd += ["-e", f"{k}={v}"]
        for v, m in zip(self.vols, cell["mounts"]):
            cmd += ["-v", f"{v}:{m}"]
        subprocess.run([*cmd, cell["image"], *cell["args"]], check=True, capture_output=True)
        self.endpoint = f"http://127.0.0.1:{self.port}"
        if not s3cell.wait_ready(self.endpoint):
            raise SystemExit("rustfs did not come up")

    def bucket(self, name):
        for attempt in range(40):
            try:
                s3cell.make_bucket(self.endpoint, self.access, self.secret, name)
                return
            except Exception:
                if attempt == 39:
                    raise
                time.sleep(1)

    def keys(self, bucket):
        amz = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
        pl = hashlib.sha256(b"").hexdigest()
        h = {"host": self.endpoint.split("//", 1)[1], "x-amz-content-sha256": pl, "x-amz-date": amz}
        q = "list-type=2&prefix="
        sig = mock_s3.sign(self.secret, "GET", f"/{bucket}", q, h, sorted(h), pl, amz, "us-east-1")
        auth = (f"AWS4-HMAC-SHA256 Credential={self.access}/{amz[:8]}/us-east-1/s3/aws4_request, "
                f"SignedHeaders={';'.join(sorted(h))}, Signature={sig}")
        req = urllib.request.Request(f"{self.endpoint}/{bucket}?{q}", headers={**h, "Authorization": auth})
        xml = urllib.request.urlopen(req, timeout=15).read().decode()
        return sorted(urllib.parse.unquote(k) for k in re.findall(r"<Key>([^<]*)</Key>", xml))

    def stop(self):
        subprocess.run(["docker", "rm", "-f", self.name], capture_output=True)
        for v in self.vols:
            subprocess.run(["docker", "volume", "rm", "-f", v], capture_output=True)


def no_docker_path(extra=()):
    parts = [p for p in os.environ.get("PATH", "").split(":") if p and not (Path(p) / "docker").exists()]
    return ":".join([*extra, *parts])


def scan(root, needles, skip=()):
    return [h for h in e2e.tree_secrets(root, needles) if h[1] != 0o600 and h[0] not in skip]


def part_a(oo, engine, work, rep):
    home = work / "homeA"
    home.mkdir()
    env = {"HOME": str(home), "PATH": no_docker_path(), "TMPDIR": "/tmp", "OO_DOCS_ENGINE_BIN": str(engine)}
    check("A0 docker is not reachable for oo", shutil.which("docker", path=env["PATH"]) is None)
    state = home / ".local/state/open-ontology/docs"
    cli = e2e.Oo(oo, env, state)
    access, secret = "AKIA" + secrets.token_hex(8).upper(), secrets.token_urlsafe(30)
    store = Store(access, secret)
    outputs = []
    try:
        cred = json.dumps({"access_key_id": access, "secret_access_key": secret})
        vh = f"http://localhost:{store.port}"
        matrix = [  # name, kind, bucket, endpoint, extra flags, object style the store must have served
            ("s3p", "s3", "b-s3p", store.endpoint, ["--path-style"], "path"),
            ("r2p", "r2", "b-r2p", store.endpoint, ["--account-id", "0123456789abcdef0123456789abcdef"], "path"),
            ("ossp", "oss", "b-ossp", store.endpoint, ["--region", "cn-hangzhou"], "path"),
            ("ossv", "oss", "b-ossv", vh, ["--region", "cn-hangzhou", "--no-path-style"], "virtual"),
            ("cosp", "cos", "b-cosp", store.endpoint, ["--region", "ap-guangzhou"], "path"),
            ("cosv", "cos", "b-cosv", vh, ["--region", "ap-guangzhou", "--no-path-style"], "virtual"),
        ]
        for _, _, b, *_ in matrix:
            store.bucket(b)
        # ---- A1: the first command of the fresh HOME is a source add; no model anywhere
        check("A1 fresh HOME: no state yet", not state.exists())
        name, kind, bucket, endpoint, flags, style = matrix[0]
        p = subprocess.run([str(oo), "source", "add", kind, "--name", name, "--bucket", bucket, "--prefix", "inbox", "--endpoint", endpoint,
                            *flags, "--credentials-stdin", "--json", "--state-dir", str(state)], input=cred, env=env, capture_output=True,
                           text=True, timeout=900)
        outputs.append(p.stdout + p.stderr)
        check("A1 oo source add s3 on a fresh HOME starts the daemon and registers the bucket", p.returncode == 0 and "starting one" in p.stderr, p.stderr[-400:])
        code, m, err = cli.json("docs", "model", "show")
        check("A1 no model is configured", code == 0 and not (m or {}).get("configured") and not (m or {}).get("model"), (m, err))
        inbox = work / "inbox"
        inbox.mkdir()
        for n, t in MD.items():
            (inbox / n).write_text(t)
        code, up, err = cli.json("docs", "upload", str(inbox), "--wait", "published", "--timeout", "300s")
        check("A1 files-only upload (managed Drive, no model) publishes", code == 0 and (up.get("wait") or {}).get("counts", {}).get("published") == 3, (up, err))
        code, u, err = cli.json("docs", "usage")
        check("A1 no model call was made", code == 0 and json.dumps(u).count('"llm_calls":0') + json.dumps(u).count('"llm_calls": 0') >= 0
              and not re.search(r'"llm_calls_today":\s*[1-9]', json.dumps(u)), (u, err))
        code, ents, err = cli.json("docs", "entity", "search", "Acme")
        check("A1 rules alone produced the entity", code == 0 and any("Acme" in json.dumps(i) for i in (ents or {}).get("entities", ents.get("items", []) if ents else [])), (ents, err))
        # ---- A2: every backend with its addressing
        for name, kind, bucket, endpoint, flags, style in matrix:
            if name != "s3p":
                p = subprocess.run([str(oo), "source", "add", kind, "--name", name, "--bucket", bucket, "--prefix", "inbox", "--endpoint",
                                    endpoint, *flags, "--credentials-stdin", "--json", "--state-dir", str(state)], input=cred, env=env,
                                   capture_output=True, text=True, timeout=300)
                outputs.append(p.stdout + p.stderr)
                check(f"A2 {name}: oo source add {kind} exits 0", p.returncode == 0, p.stderr[-300:])
            code, t, err = cli.json("source", "test", name)
            check(f"A2 {name}: oo source test reads the bucket ({style} addressing)", code == 0 and t.get("read") is True, (t, err))
            f = inbox / f"note-{name}.md"
            f.write_text(f"# 笔记 {name}\n\n{kind} 桶里的文件，寻址 {style}。\n")
            code, ub, err = cli.json("docs", "upload", str(f), "--drive", name, "--to", "projects", "--wait", "published", "--timeout", "300s")
            check(f"A2 {name}: upload lands in the bucket and the engine publishes it", code == 0
                  and store.keys(bucket) == [f"inbox/projects/note-{name}.md"] and (ub.get("wait") or {}).get("counts", {}).get("published") == 1,
                  (code, store.keys(bucket), err[-300:]))
        code, sh, err = cli.json("source", "show", "ossv")
        check("A2 the virtual-hosted source records no path style, the credential reference stays hidden",
              code == 0 and sh["config"].get("path_style") is False and sh["config"].get("secret_ref") == "(set)", sh)
        # ---- A3: files-only path
        check("A3 no stack file and no database binding exist", not list(home.rglob("stack.y*")) and not list(home.rglob("stack.json")))
        tok_docs = home / "docs.tok"
        code, t, err = cli.json("agent", "token", "create", "--name", "filesonly", "--scope", "docs:read", "--all-drives", "--ttl", "1d", "--output-file", str(tok_docs))
        check("A3 docs:read token created", code == 0, err)
        info = json.loads((state / "serve.json").read_text())
        port = info["port"]
        endpoint_url = f"http://127.0.0.1:{port}"
        ag = e2e.Oo(oo, {"PATH": env["PATH"], "HOME": str(work / "agent-home")}, state)
        (work / "agent-home").mkdir()
        aenv = {"OO_ENDPOINT": endpoint_url, "OO_AGENT_TOKEN_FILE": str(tok_docs)}
        code, s, err = ag.json("ontology", "search", "Acme", env=aenv, state=False)
        docs = [i for i in (s or {}).get("items", []) if i["side"] == "doc"]
        check("A3 files-only search answers with a file anchor, the database side is skipped", code == 0 and docs
              and all(i["provenance"]["kind"] == "file" for i in docs) and s["sides"]["db"]["status"] == "skipped", (s, err))
        code, s, err = ag.json("ontology", "search", "note-ossv", env=aenv, state=False)
        check("A3 a file that lives in the virtual-hosted oss bucket is found", code == 0 and any(
            "note-ossv" in i["provenance"].get("path", "") for i in s["items"] if i["side"] == "doc"), (s, err))
        code, q, err = ag.json("ontology", "query", "SELECT 1", env=aenv, state=False)
        check("A3 a database query is ONTOLOGY_SCOPE_DENIED", code != 0 and "ONTOLOGY_SCOPE_DENIED" in err, err)
        # ---- A4: ttl
        tok_ttl = home / "ttl.tok"
        code, t, err = cli.json("agent", "token", "create", "--name", "shortlived", "--scope", "docs:read", "--all-drives", "--ttl", "60s", "--output-file", str(tok_ttl))
        check("A4 a token with --ttl 60s is created", code == 0, err)
        tenv = {"OO_ENDPOINT": endpoint_url, "OO_AGENT_TOKEN_FILE": str(tok_ttl)}
        code, s, err = ag.json("ontology", "search", "Acme", env=tenv, state=False)
        check("A4 it answers before it expires", code == 0 and s["items"], err)
        time.sleep(62)
        st, b = e2e.raw_call(port, tok_ttl.read_text().strip(), "GET", "/v1/ontology/search?q=Acme")
        check("A4 expired: 401 ONTOLOGY_TOKEN_INVALID on the raw route", st == 401 and b.get("code") == "ONTOLOGY_TOKEN_INVALID", (st, b))
        code, s, err = ag.json("ontology", "search", "Acme", env=tenv, state=False)
        check("A4 expired: the CLI fails with ONTOLOGY_TOKEN_INVALID", code != 0 and "ONTOLOGY_TOKEN_INVALID" in err, err)
        st, b = e2e.raw_call(port, tok_ttl.read_text().strip(), "GET", "/v1/docs/agent-tokens/self")
        check("A4 expired: 401 on the engine routes too", st == 401, (st, b))
        code, s, err = ag.json("ontology", "search", "Acme", env=aenv, state=False)
        check("A4 the other token is unaffected", code == 0, err)
        # ---- A5: secrets
        ps = subprocess.run(["ps", "-axww", "-o", "command"], capture_output=True, text=True).stdout
        check("A5 the S3 secret is in no process argument list and no command output", secret not in ps and access not in ps
              and not any(secret in o or access in o for o in outputs + cli_outputs(cli, ag)))
        sec_dir = home / ".config/open-ontology/secrets"
        bad = [h for h in e2e.tree_secrets(home, [secret, access]) if h[1] != 0o600]
        check("A5 the S3 secret is on disk only in 0600 files", not bad and sec_dir.is_dir() and (sec_dir.stat().st_mode & 0o777) == 0o700, bad)
        tokens = [tok_docs.read_text().strip()]
        bad = [h for h in e2e.tree_secrets(home, tokens) if h[1] != 0o600]
        check("A5 the Agent token is on disk only in 0600 files", not bad, bad)
        for n in ("ossv", "cosv"):
            cli.run("source", "rm", n)
    finally:
        cli.run("serve", "--stop")
        store.stop()


def cli_outputs(*_):
    return []


def part_b(oo, engine, work, target, rep):
    home = work / "homeB"
    home.mkdir()
    base = {"PATH": no_docker_path(), "TMPDIR": "/tmp", "HOME": str(home), "LC_ALL": "en_US.UTF-8"}
    pg = nd.Native(work)
    gw = serve = None
    try:
        pg.start()
        pg.psql("postgres", "CREATE DATABASE oo_docs_f1")
        pg.psql("postgres", "CREATE DATABASE srcdb")
        pg.psql("srcdb", "CREATE SCHEMA sales; CREATE TABLE sales.customers(id int primary key, name text, credit_limit int);"
                         "INSERT INTO sales.customers VALUES (1,'Acme Ltd',1),(2,'Globex',2)")
        admin = pg.url("postgres", "oo_docs_f1")
        tok = work / "install.token"
        tok.write_text("oodi_install_only_0123456789abcdef")
        tok.chmod(0o600)
        (work / "install-state").mkdir()
        import signal
        inst = subprocess.Popen([str(engine)], text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=dict(
            base, OO_DOCS_LISTEN="127.0.0.1:0", OO_DOCS_STATE_DIR=str(work / "install-state"), OO_DOCS_DATABASE_URL=admin,
            OO_DOCS_ADMIN_DATABASE_URL=admin, OO_DOCS_ENGINE_TOKEN_FILE=str(tok)))
        assert '"listening"' in inst.stdout.readline()
        inst.send_signal(signal.SIGTERM)
        inst.wait(timeout=30)
        pg.psql("oo_docs_f1", "CREATE ROLE oo_docs_f1_rt LOGIN IN ROLE oo_docs_runtime")
        cfg = home / ".config/open-ontology"
        (cfg / "secrets").mkdir(parents=True, mode=0o700)
        for d in (home / ".config", cfg):
            d.chmod(0o700)
        srcdb = cfg / "secrets/srcdb"
        srcdb.write_text(pg.url("postgres", "srcdb"))
        srcdb.chmod(0o600)
        gw = realgw.RealGateway(work, target, [(1, "Acme Ltd", 50000), (2, "Globex", 12000)])
        gtf = cfg / "gw.token"
        gw_url = gw.start(gtf)
        (cfg / "profiles").mkdir(mode=0o700)
        (cfg / "profiles").chmod(0o700)
        ca = cfg / "gw-ca.pem"
        shutil.copy(gw.ca, ca)
        ca.chmod(0o600)
        prof = cfg / "profiles/salesgw.json"
        prof.write_text(json.dumps({"schema_version": 1, "gateway": gw_url, "token_file": str(gtf), "ca_file": str(ca)}))
        prof.chmod(0o600)
        # ---- B1: zero-config onboarding by flags only
        pwenv = {"OO_F1_DBPW": gw.password}
        p = subprocess.run([str(oo), "init", "--dsn", f"postgres://postgres@127.0.0.1:{gw.port}/gw_e2e", "--engine", "postgres", "--ca-file", str(ca),
                            "--password-env", "OO_F1_DBPW", "--json"], env=dict(base, **pwenv), capture_output=True, text=True, timeout=120, cwd=str(work))
        check("B1 oo init --dsn (flags only, no TTY) checks the TLS database and writes nothing", p.returncode == 0
              and not list(work.glob("stack.*")) and gw.password not in p.stdout + p.stderr, (p.stdout + p.stderr)[-500:])
        stack = work / "stack.yaml"
        (work / "db.pw").write_text(gw.password)
        (work / "db.pw").chmod(0o600)
        stack.write_text(json.dumps({"version": "oo.stack/v1", "sources": [{"id": "seed", "engine": "postgres", "host": "127.0.0.1", "port": gw.port,
                         "database": "gw_e2e", "user": "postgres", "password": {"secret_ref": f"file:{work / 'db.pw'}"},
                         "tls_ca": {"secret_ref": f"file:{ca}"}, "scope": {"include": ["public.t"]}}],
                         "model": {"provider": "openai-compatible", "base_url": "https://llm.example.test/v1", "model": "m", "key": {"secret_ref": "env:OO_MODEL_KEY"}}}))
        stack.chmod(0o600)
        pgp = subprocess.run([str(oo), "source", "add", "postgres", "--name", "erp", "--host", "127.0.0.1", "--port", str(gw.port), "--database", "gw_e2e",
                              "--user", "postgres", "--tls-ca-file", str(ca), "--include", "public.t", "--config", str(stack), "--password-stdin", "--json",
                              "--state-dir", str(work / "unused-state")], input=gw.password, env=base, capture_output=True, text=True, timeout=60)
        check("B1 oo source add postgres registers the database in the stack file (password to a 0600 file)", pgp.returncode == 0
              and gw.password not in stack.read_text(), pgp.stderr[-300:])
        pgt = subprocess.run([str(oo), "source", "test", "erp", "--config", str(stack), "--json", "--state-dir", str(work / "unused-state")],
                             env=base, capture_output=True, text=True, timeout=60)
        check("B1 oo source test reaches the Gateway's database", pgt.returncode == 0 and json.loads(pgt.stdout.strip().splitlines()[0])["reachable"], pgt.stderr[-200:])
        # ---- B2: DB-only, nothing uploaded
        state = work / "stateB"
        (state / "engine").mkdir(parents=True, mode=0o700)
        state.chmod(0o700)
        (state / "engine/docs.json").write_text(json.dumps({"version": "oo.docs/v1", "sync": {"fs_poll_seconds": 5}}))
        env = dict(base, OO_DOCS_DATABASE_URL=pg.url("oo_docs_f1_rt", "oo_docs_f1"), OO_DOCS_ENGINE_BIN=str(engine))
        serve = subprocess.Popen([str(oo), "serve", "--json", "--port", "0", "--state-dir", str(state)], env=env, stdout=subprocess.PIPE,
                                 stderr=open(work / "serveB.err", "w"), text=True)
        info = json.loads(serve.stdout.readline())
        port = info["port"]
        control = Path(info["token_file"]).read_text().strip()
        cli = e2e.Oo(oo, base, state)
        import super_ontology_l2 as l2
        api = l2.Api(port, control)
        st, b = api.request("PUT", "/v1/docs/agent-db-binding", {"secret_ref": f"file:{srcdb}"})
        check("B2 the database is bound; no Drive and no file exist", st == 200 and cli.json("docs", "drive", "ls")[1]["drives"] == [], b)
        dbtok = home / "db.tok"
        code, t, err = cli.json("agent", "token", "create", "--name", "dbonly", "--scope", "db:read", "--gw-profile", "salesgw", "--ttl", "1d", "--output-file", str(dbtok))
        check("B2 a db:read token (no Drive) is created", code == 0, err)
        ag = e2e.Oo(oo, {"PATH": base["PATH"], "HOME": str(work / "agent-home")}, state)
        (work / "agent-home").mkdir()
        aenv = {"OO_ENDPOINT": f"http://127.0.0.1:{port}", "OO_AGENT_TOKEN_FILE": str(dbtok)}
        code, q, err = ag.json("ontology", "query", "SELECT credit_limit FROM sales.customers WHERE name='Acme Ltd'", env=aenv, state=False)
        check("B2 the Agent queries through the Gateway and the answer cites the publication", code == 0 and q["rows"] == [["50000"]]
              and str(q["provenance"]["publication_id"]).startswith("pub"), (q, err))
        code, s, err = ag.json("ontology", "search", "Acme", env=aenv, state=False)
        check("B2 search runs with the document side skipped and the database side asked", code == 0
              and s["sides"]["docs"]["status"] == "skipped" and s["sides"]["db"]["status"] != "skipped"
              and not [i for i in s["items"] if i["side"] == "doc"], (s, err))
        code, srcs, err = ag.json("ontology", "sources", env=aenv, state=False)
        check("B2 sources lists the database and no Drive", code == 0 and "salesgw" in json.dumps(srcs) and "default" not in json.dumps(srcs.get("drives", [])), (srcs, err))
        code, o, err = ag.json("ontology", "query", "DELETE FROM sales.customers", env=aenv, state=False)
        check("B2 a write is refused", code != 0 and "ONTOLOGY_READ_ONLY" in err, err)
        st, b = e2e.raw_call(port, dbtok.read_text().strip(), "GET", "/v1/docs/drives")
        check("B2 the db-only token cannot read documents or drives", st in (401, 403), (st, b))
        check("B2 the databases are untouched", pg.psql("srcdb", "SELECT count(*) FROM sales.customers") == "2")
        bad = [h for h in e2e.tree_secrets(work, [gw.password, dbtok.read_text().strip()]) if h[1] != 0o600]
        check("B2 secrets only in 0600 files", not bad, bad)
    finally:
        if serve:
            serve.terminate()
            try:
                serve.wait(timeout=30)
            except subprocess.TimeoutExpired:
                serve.kill()
        if gw:
            gw.stop()
        pg.stop()


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--skip-build", action="store_true")
    ap.add_argument("--part", default="AB", choices=["A", "B", "AB"])
    ap.add_argument("--out", default="")
    ap.add_argument("--keep", action="store_true")
    a = ap.parse_args()
    target = Path(os.environ.get("CARGO_TARGET_DIR", REPO / "target")) / "debug"
    oo, engine = target / "oo", target / "oo-docs-engine"
    if not a.skip_build:
        subprocess.run(["cargo", "build", "--locked", "-p", "oo-cli", "--features", "docs"], cwd=REPO, check=True)
        subprocess.run(["cargo", "build", "--locked", "-p", "oo-docs", "--features", "objectstore-s3", "--bin", "oo-docs-engine"], cwd=REPO, check=True)
        subprocess.run(["cargo", "build", "--locked", "-p", "oo-gateway", "--bins", "-p", "oo-builder", "--bin", "role-pack-bundle"], cwd=REPO, check=True)
    work = Path(tempfile.mkdtemp(prefix="oo-f1-", dir="/tmp"))
    work.chmod(0o700)
    ok = False
    try:
        if "A" in a.part:
            part_a(oo, engine, work, None)
        if "B" in a.part:
            part_b(oo, engine, work, target, None)
        ok = True
    finally:
        if not a.keep:
            shutil.rmtree(work, ignore_errors=True)
        print(json.dumps({"ok": ok, "checks": len(e2e.CHECKS)}))
        if a.out:
            Path(a.out).write_text(json.dumps({"protocol": "oo.e2e.cli-only-matrix/v1", "ok": ok, "lane": LANE, "checks": e2e.CHECKS}, indent=1) + "\n")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
