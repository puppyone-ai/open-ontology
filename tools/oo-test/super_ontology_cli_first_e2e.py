#!/usr/bin/env python3
"""D83 CLI-first end to end (WP4): one `oo` binary, a fresh HOME, no Desktop.

  fresh HOME -> files into a Drive -> a local Postgres as the database source -> link rule and projection
  -> `oo agent token create` -> the Agent (CLI and MCP, token file only) asks both sources -> every answer cites
  its source (a file anchor or a table row).

Negatives: a docs-only token cannot query the database, a write is refused, an Agent token cannot change the
engine, a revoked token is 401 on the CLI, on MCP and on the raw route, and the token text is on disk only in
files of mode 0600.

Stand-ins, stated plainly: PostgreSQL is a private native cluster (initdb/pg_ctl, no container); the Gateway is a
small HTTPS server with a private CA that runs read-only SQL against that Postgres (the real Gateway needs the
full `oo up` stack); the Agent DB binding is an engine HTTP call with the
control token (the link rule and the projection run use `oo source link`). Everything the Agent does goes through `oo`.

Usage: tools/oo-test/super_ontology_cli_first_e2e.py [--skip-build] [--out FILE] [--keep]
"""

import argparse
import http.client
import json
import os
import shutil
import signal
import ssl
import subprocess
import sys
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.dont_write_bytecode = True
import super_ontology_l2 as l2  # noqa: E402
import super_ontology_nodocker as nd  # noqa: E402

REPO = l2.REPO
LANE = os.environ.get("OO_TEST_LANE", "super-ontology-wp3")
CHECKS = []


def check(name, cond, detail=""):
    CHECKS.append({"check": name, "ok": bool(cond)})
    print(("PASS " if cond else "FAIL ") + name + ("" if cond else f"  {detail}"), flush=True)
    if not cond:
        raise SystemExit(1)


class Oo:
    def __init__(self, oo, env, state):
        self.oo, self.env, self.state = str(oo), env, str(state)

    def run(self, *args, env=None, state=True, timeout=120):
        argv = [self.oo, *args] + (["--state-dir", self.state] if state else [])
        p = subprocess.run(argv, env=dict(self.env, **(env or {})), capture_output=True, text=True, timeout=timeout)
        return p.returncode, p.stdout, p.stderr

    def json(self, *args, **kw):
        code, out, err = self.run(*args, "--json", **kw)
        try:
            return code, json.loads(out.strip().splitlines()[0]) if out.strip() else None, err
        except ValueError:
            return code, None, err + out


class Gateway(threading.Thread):
    """Read-only SQL over HTTPS, a private CA, one bearer token; records every call."""

    def __init__(self, work, pg, db, token):
        super().__init__(daemon=True)
        def ssl_(*a):
            subprocess.run(["openssl", *a], check=True, capture_output=True, cwd=work)

        # a private CA and a leaf for localhost (a CA certificate cannot serve as the end entity)
        ssl_("req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", "ca.key", "-out", "ca.crt", "-days", "2", "-subj", "/CN=oo-e2e-ca")
        ssl_("req", "-newkey", "rsa:2048", "-nodes", "-keyout", "gw.key", "-out", "gw.csr", "-subj", "/CN=localhost")
        (work / "leaf.ext").write_text("subjectAltName=DNS:localhost\nbasicConstraints=CA:FALSE\nextendedKeyUsage=serverAuth\n")
        ssl_("x509", "-req", "-in", "gw.csr", "-CA", "ca.crt", "-CAkey", "ca.key", "-CAcreateserial", "-out", "gw.crt", "-days", "2", "-extfile", "leaf.ext")
        self.crt, self.calls = work / "ca.crt", []
        outer = self

        class H(BaseHTTPRequestHandler):
            def log_message(self, *a):
                pass

            def _send(self, status, body, extra=None):
                raw = json.dumps(body).encode()
                self.send_response(status)
                self.send_header("content-type", "application/json")
                self.send_header("content-length", str(len(raw)))
                for k, v in (extra or {}).items():
                    self.send_header(k, v)
                self.end_headers()
                self.wfile.write(raw)

            def _auth(self):
                return self.headers.get("authorization") == f"Bearer {token}"

            def do_GET(self):
                outer.calls.append(("GET", self.path))
                if not self._auth():
                    return self._send(401, {"error": {"code": "UNAUTHENTICATED"}})
                self._send(200, {"publication": "live", "tables": []}, {"x-oo-publication-id": "pub-e2e-1"})

            def do_POST(self):
                body = json.loads(self.rfile.read(int(self.headers.get("content-length", 0))) or b"{}")
                outer.calls.append(("POST", self.path, body.get("sql")))
                if not self._auth():
                    return self._send(401, {"error": {"code": "UNAUTHENTICATED"}})
                if self.path != "/v1/query":
                    return self._send(404, {"error": {"code": "NOT_FOUND"}})
                sql = body["sql"].strip().rstrip(";")
                wrapped = f"SELECT coalesce(json_agg(row_to_json(t)), '[]'::json) FROM ({sql}) t"
                p = subprocess.run(["psql", "-h", "127.0.0.1", "-p", str(pg.port), "-U", "postgres", "-d", db, "-tA", "-c", wrapped],
                                   capture_output=True, text=True, env=dict(os.environ, PGOPTIONS="-c default_transaction_read_only=on"))
                if p.returncode:
                    return self._send(422, {"error": {"code": "SQL_REFUSED"}})
                rows = json.loads(p.stdout)
                cols = list(rows[0].keys()) if rows else []
                cell = lambda v: None if v is None else (v if isinstance(v, bool) else str(v))  # noqa: E731
                self._send(200, {"query_id": f"q-{len(outer.calls)}", "columns": [{"name": c, "type": "text"} for c in cols],
                                 "rows": [[cell(r[c]) for c in cols] for r in rows], "returned_rows": len(rows), "truncated": False,
                                 "freshness": [], "sources": [], "warnings": []},
                           {"x-oo-publication-id": "pub-e2e-1", "x-oo-data-vector-sha256": "ab" * 32})

        self.srv = HTTPServer(("127.0.0.1", 0), H)
        ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        ctx.load_cert_chain(str(work / "gw.crt"), str(work / "gw.key"))
        self.srv.socket = ctx.wrap_socket(self.srv.socket, server_side=True)
        self.port = self.srv.server_address[1]

    def run(self):
        self.srv.serve_forever()


def raw_call(port, token, method, path, body=None):
    c = http.client.HTTPConnection("127.0.0.1", port, timeout=30)
    h = {"Authorization": f"Bearer {token}"}
    data = None
    if body is not None:
        data, h["Content-Type"] = json.dumps(body).encode(), "application/json"
    c.request(method, path, body=data, headers=h)
    r = c.getresponse()
    raw = r.read()
    c.close()
    try:
        return r.status, json.loads(raw)
    except ValueError:
        return r.status, raw


class Mcp:
    def __init__(self, oo, env, endpoint, token_file):
        self.p = subprocess.Popen([str(oo), "mcp", "--stdio", "--endpoint", endpoint, "--token-file", str(token_file)], env=env,
                                  stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
        self.n = 0
        self.rpc("initialize", {"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "e2e", "version": "0"}})
        self.p.stdin.write(json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + "\n")
        self.p.stdin.flush()

    def rpc(self, method, params):
        self.n += 1
        self.p.stdin.write(json.dumps({"jsonrpc": "2.0", "id": self.n, "method": method, "params": params}) + "\n")
        self.p.stdin.flush()
        while True:
            line = self.p.stdout.readline()
            if not line:
                raise SystemExit("mcp server closed")
            m = json.loads(line)
            if m.get("id") == self.n:
                return m

    def tool(self, name, args):
        r = self.rpc("tools/call", {"name": name, "arguments": args})
        res = r.get("result", {})
        return res.get("isError", False) or "error" in r, res.get("structuredContent") or res

    def close(self):
        self.p.terminate()


def tree_secrets(root, needles, allowed_mode=0o600):
    """Files under root that contain a needle; every one must be mode 0600."""
    hits = []
    for dp, _, fs in os.walk(root):
        for f in fs:
            p = Path(dp) / f
            try:
                if p.is_symlink() or p.stat().st_size > 8_000_000:
                    continue
                data = p.read_bytes()
            except OSError:
                continue
            if any(n.encode() in data for n in needles):
                hits.append((str(p), p.stat().st_mode & 0o777))
    return hits


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--skip-build", action="store_true")
    ap.add_argument("--out", default="")
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    target = Path(os.environ.get("CARGO_TARGET_DIR", REPO / "target")) / "debug"
    oo, engine = target / "oo", target / "oo-docs-engine"
    mcp_target = REPO / "tools/mcp-gateway/target/debug/mcp-gateway"
    if not args.skip_build:
        subprocess.run(["cargo", "build", "--locked", "-p", "oo-cli", "--features", "docs"], cwd=REPO, check=True)
        subprocess.run(["cargo", "build", "--locked", "-p", "oo-docs", "--bin", "oo-docs-engine"], cwd=REPO, check=True)
        subprocess.run(["cargo", "build", "--locked"], cwd=REPO / "tools/mcp-gateway", check=True)
    work = Path(tempfile.mkdtemp(prefix="oo-e2e-", dir="/tmp"))
    work.chmod(0o700)
    home = work / "home"
    home.mkdir(mode=0o700)
    bindir = work / "bin"
    bindir.mkdir()
    shutil.copy(oo, bindir / "oo")
    shutil.copy(mcp_target, bindir / "mcp-gateway")
    pg = nd.Native(work)
    serve = gw = mcp = None
    ok = False
    try:
        pg.start()
        pg.psql("postgres", "CREATE DATABASE oo_docs_e2e")
        pg.psql("postgres", "CREATE DATABASE agentdb")
        pg.psql("agentdb", "CREATE SCHEMA sales; CREATE TABLE sales.customers(id int primary key, name text, credit_limit int);"
                           "INSERT INTO sales.customers VALUES (1,'Acme Ltd',50000),(2,'Globex',12000)")
        admin = pg.url("postgres", "oo_docs_e2e")
        tok = work / "install.token"
        tok.write_text("oodi_install_only_0123456789abcdef")
        tok.chmod(0o600)
        base = {"PATH": os.environ.get("PATH", ""), "TMPDIR": "/tmp", "HOME": str(home), "LC_ALL": "en_US.UTF-8"}
        (work / "install-state").mkdir()
        inst = subprocess.Popen([str(engine)], text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=dict(
            base, OO_DOCS_LISTEN="127.0.0.1:0", OO_DOCS_STATE_DIR=str(work / "install-state"), OO_DOCS_DATABASE_URL=admin,
            OO_DOCS_ADMIN_DATABASE_URL=admin, OO_DOCS_ENGINE_TOKEN_FILE=str(tok)))
        assert '"listening"' in inst.stdout.readline()
        inst.send_signal(signal.SIGTERM)
        inst.wait(timeout=30)
        pg.psql("oo_docs_e2e", "CREATE ROLE oo_docs_e2e_rt LOGIN IN ROLE oo_docs_runtime")

        # secrets: only as 0600 files under HOME
        cfg = home / ".config/open-ontology"
        (cfg / "secrets").mkdir(parents=True, mode=0o700)
        for d in (home / ".config", cfg):
            d.chmod(0o700)
        agentdb = cfg / "secrets/agentdb"
        agentdb.write_text(pg.url("postgres", "agentdb"))
        agentdb.chmod(0o600)

        # the Gateway stand-in and the profile the Agent token will name
        gwtoken = "ooct_" + os.urandom(22).hex()[:43]
        gw = Gateway(work, pg, "agentdb", gwtoken)
        gw.start()
        (cfg / "profiles").mkdir(mode=0o700)
        (cfg / "profiles").chmod(0o700)
        gtf = cfg / "gw.token"
        gtf.write_text(gwtoken + "\n")
        gtf.chmod(0o600)
        ca = cfg / "gw-ca.pem"
        shutil.copy(gw.crt, ca)
        ca.chmod(0o600)
        prof = cfg / "profiles/salesgw.json"
        prof.write_text(json.dumps({"schema_version": 1, "gateway": f"https://localhost:{gw.port}", "token_file": str(gtf), "ca_file": str(ca)}))
        prof.chmod(0o600)

        # the engine and oo serve, fresh state
        state = work / "state"
        (state / "engine").mkdir(parents=True, mode=0o700)
        state.chmod(0o700)
        (state / "engine").chmod(0o700)
        (state / "engine/docs.json").write_text(json.dumps({"version": "oo.docs/v1", "sync": {"fs_poll_seconds": 5}}))
        env = dict(base, OO_DOCS_DATABASE_URL=pg.url("oo_docs_e2e_rt", "oo_docs_e2e"), OO_DOCS_ENGINE_BIN=str(engine))
        serve = subprocess.Popen([str(bindir / "oo"), "serve", "--json", "--port", "0", "--state-dir", str(state)], env=env,
                                 stdout=subprocess.PIPE, stderr=open(work / "serve.err", "w"), text=True)
        info = json.loads(serve.stdout.readline())
        port = info["port"]
        control = Path(info["token_file"]).read_text().strip()
        cli = Oo(bindir / "oo", base, state)
        endpoint = f"http://127.0.0.1:{port}"
        check("oo serve is up", port > 0)

        # files -> Drive (the upload command is WP1's: files go into the Drive folder, then `oo docs drive add`)
        drive = work / "files"
        (drive / "Customers").mkdir(parents=True)
        (drive / "Customers/acme.md").write_text(
            "---\ntype: organization\n---\n\n# Acme Ltd\n\nAcme Ltd is our largest customer. Contact: Jane Roe.\n\n## Contract\n\nThe contract renews every March.\n")
        (drive / "Customers/globex.md").write_text("---\ntype: organization\n---\n\n# Globex\n\nGlobex is a small reseller.\n")
        code, d, err = cli.json("docs", "drive", "add", "--name", "default", "--root", str(drive))
        check("drive registered", code == 0 and d, err)
        deadline = time.time() + 120
        while time.time() < deadline:
            code, files, _ = cli.json("docs", "files", "ls", "--drive", "default")
            items = (files or {}).get("files", [])
            if len(items) == 2 and all(i["state"] == "published" for i in items):
                break
            time.sleep(0.5)
        else:
            raise SystemExit(f"files did not publish: {files}")
        check("every file published", True)

        # database source: link rule, Agent DB binding, projection (engine calls with the control token)
        api = l2.Api(port, control)
        drive_id = d["id"] if "id" in d else d["drive"]["id"]
        st, b = api.request("PUT", "/v1/docs/agent-db-binding", {"secret_ref": f"file:{agentdb}"})
        check("agent db bound", st == 200, b)
        code, b, err = cli.json("source", "link", "add", "--table", "sales.customers", "--key-col", "id", "--name-col", "name",
                                "--entity-type", "Organization")
        check("link rule stored", code == 0, (b, err))
        code, b, err = cli.json("source", "link", "project")
        check("projection ran", code == 0, (b, err))

        # agent tokens through the CLI
        bot, ro, gone = home / "bot.tok", home / "docsonly.tok", home / "gone.tok"
        code, t, err = cli.json("agent", "token", "create", "--name", "bot", "--scope", "docs:read", "--scope", "db:read",
                                "--gw-profile", "salesgw", "--drive", "default", "--ttl", "1d", "--output-file", str(bot))
        check("agent token created", code == 0 and t and "token" not in t, err)
        check("token file is 0600", (bot.stat().st_mode & 0o777) == 0o600)
        check("a second create refuses to overwrite", cli.json("agent", "token", "create", "--name", "bot2", "--scope", "db:read",
                                                                 "--gw-profile", "salesgw", "--output-file", str(bot))[0] != 0)
        check("docs-only token", cli.json("agent", "token", "create", "--name", "docsonly", "--scope", "docs:read", "--drive", "default",
                                          "--output-file", str(ro))[0] == 0)
        check("third token", cli.json("agent", "token", "create", "--name", "gone", "--scope", "docs:read", "--scope", "db:read",
                                      "--gw-profile", "salesgw", "--drive", "default", "--output-file", str(gone))[0] == 0)
        code, lst, _ = cli.json("agent", "token", "ls")
        check("token list has no secret", code == 0 and "ooa_" not in json.dumps(lst))

        # the Agent: CLI with the token file only
        ag = Oo(bindir / "oo", {"PATH": base["PATH"], "HOME": str(work / "agent-home")}, state)
        (work / "agent-home").mkdir()
        aenv = {"OO_ENDPOINT": endpoint, "OO_AGENT_TOKEN_FILE": str(bot)}
        code, s, err = ag.json("ontology", "search", "Acme", env=aenv, state=False)
        sides = {i["side"] for i in (s or {}).get("items", [])}
        check("search answers from both sources", code == 0 and sides == {"doc", "db"}, (s, err))
        doc = next(i for i in s["items"] if i["side"] == "doc")
        db = next(i for i in s["items"] if i["side"] == "db")
        check("doc item cites a file", doc["provenance"]["kind"] == "file" and doc["provenance"]["path"].endswith("acme.md"))
        check("db item cites a table row", db["provenance"]["kind"] == "table_row" and db["provenance"]["table"] == "sales.customers"
              and db["provenance"]["pk"] == "1" and db["provenance"]["publication_id"] == "pub-e2e-1")
        check("urns are stable", doc["urn"].startswith("oo:doc/") and db["urn"] == "oo:db/salesgw/sales.customers/1")
        code, e, err = ag.json("ontology", "entity", doc["urn"], env=aenv, state=False)
        link = next((link for link in (e or {}).get("links", []) if link.get("db_table") == "sales.customers"), None)
        check("doc entity: facts cited, linked to the row", code == 0 and e["facts"] and all(f["provenance"] for f in e["facts"]) and link, (e, err))
        check("linked row comes with its provenance", link["row"]["columns"]["name"] == "Acme Ltd" and link["row"]["provenance"]["kind"] == "table_row")
        code, e, err = ag.json("ontology", "entity", db["urn"], env=aenv, state=False)
        check("row entity links back to the document", code == 0 and any(l["target"]["urn"].startswith("oo:doc/") for l in e["links"]), (e, err))
        code, q, err = ag.json("ontology", "query", "SELECT credit_limit FROM sales.customers WHERE name='Acme Ltd'", env=aenv, state=False)
        check("query cites the publication", code == 0 and q["rows"] == [["50000"]] and q["provenance"]["publication_id"] == "pub-e2e-1", (q, err))
        code, out, err = ag.run("ontology", "search", "Acme", env=aenv, state=False)
        check("human output has [source] lines", code == 0 and out.count("[source]") >= 2, out)

        # MCP, the same tools, the same token
        mcp = Mcp(bindir / "oo", {"PATH": base["PATH"], "HOME": str(work / "agent-home")}, endpoint, bot)
        r = mcp.rpc("tools/list", {})
        check("mcp lists the unified tools", [t["name"] for t in r["result"]["tools"]] == ["ontology_search", "ontology_entity", "ontology_sources", "ontology_query"])
        err_flag, res = mcp.tool("ontology_search", {"q": "Acme"})
        check("mcp search answers from both sources", not err_flag and {i["side"] for i in res["items"]} == {"doc", "db"}, res)
        err_flag, res = mcp.tool("ontology_query", {"sql": "SELECT credit_limit FROM sales.customers WHERE name='Acme Ltd'"})
        check("mcp query cites its source", not err_flag and res["provenance"]["publication_id"] == "pub-e2e-1", res)

        # negatives
        code, o, err = ag.json("ontology", "query", "DELETE FROM sales.customers", env=aenv, state=False)
        check("a write is refused", code != 0 and "ONTOLOGY_READ_ONLY" in err, err)
        code, o, err = ag.json("ontology", "query", "SELECT 1; DROP TABLE sales.customers", env=aenv, state=False)
        check("a second statement is refused", code != 0 and "ONTOLOGY_READ_ONLY" in err, err)
        check("the database is untouched", pg.psql("agentdb", "SELECT count(*) FROM sales.customers") == "2")
        denv = {"OO_ENDPOINT": endpoint, "OO_AGENT_TOKEN_FILE": str(ro)}
        code, o, err = ag.json("ontology", "query", "SELECT 1", env=denv, state=False)
        check("docs-only token cannot query the database", code != 0 and "ONTOLOGY_SCOPE_DENIED" in err, err)
        code, s2, err = ag.json("ontology", "search", "Acme", env=denv, state=False)
        check("docs-only token searches documents only", code == 0 and {i["side"] for i in s2["items"]} == {"doc"}
              and s2["sides"]["db"]["status"] == "skipped", (s2, err))
        err_flag, res = mcp.tool("ontology_query", {"sql": "DELETE FROM sales.customers"})
        check("mcp refuses the write", err_flag and "ONTOLOGY_READ_ONLY" in json.dumps(res), res)
        st, b = raw_call(port, bot.read_text().strip(), "POST", "/v1/docs/drives", {"name": "x", "backend": "fs", "config": {"root": str(drive)}})
        check("agent token cannot write to the engine", st == 403, (st, b))
        st, b = raw_call(port, bot.read_text().strip(), "GET", "/v1/docs/agent-tokens")
        check("agent token cannot list tokens", st == 403, (st, b))

        # revoke -> 401 everywhere
        gtok = gone.read_text().strip()
        check("gone token works before revoke", raw_call(port, gtok, "GET", "/v1/ontology/sources")[0] == 200)
        code, r, err = cli.json("agent", "token", "revoke", "gone")
        check("revoke", code == 0 and r["revoked"], err)
        st, b = raw_call(port, gtok, "GET", "/v1/ontology/search?q=Acme")
        check("revoked token is 401 on the ontology surface", st == 401 and b["code"] == "ONTOLOGY_TOKEN_INVALID", (st, b))
        st, b = raw_call(port, gtok, "GET", "/v1/docs/agent-tokens/self")
        check("revoked token is 401 on the engine routes", st == 401, (st, b))
        code, o, err = ag.json("ontology", "search", "Acme", env={"OO_ENDPOINT": endpoint, "OO_AGENT_TOKEN_FILE": str(gone)}, state=False)
        check("revoked token fails on the CLI", code != 0 and "ONTOLOGY_TOKEN_INVALID" in err, err)
        cli.json("agent", "token", "revoke", "bot")
        err_flag, res = mcp.tool("ontology_search", {"q": "Acme"})
        check("revoked token fails on MCP", err_flag and "ONTOLOGY_TOKEN_INVALID" in json.dumps(res), res)
        check("the Gateway saw only the proxy's calls", all(c[1] in ("/v1/query", "/v1/freshness") for c in gw.calls) and len(gw.calls) > 0)

        # secrets on disk
        needles = [tok.read_text().strip(), gwtoken, bot.read_text().strip(), ro.read_text().strip(), gtok, control]
        hits = tree_secrets(work, needles)
        bad = [h for h in hits if h[1] != 0o600]
        check("secrets appear only in 0600 files", not bad, bad)
        check("no secret in the serve log", not any(n in (work / "serve.err").read_text() for n in needles if n))
        ok = True
    finally:
        if mcp:
            mcp.close()
        if serve:
            serve.send_signal(signal.SIGTERM)
            try:
                serve.wait(timeout=30)
            except subprocess.TimeoutExpired:
                serve.kill()
        pg.stop()
        if not args.keep:
            shutil.rmtree(work, ignore_errors=True)
    doc = {"protocol": "oo.e2e.cli-first/v1", "ok": ok, "lane": LANE, "checks": CHECKS}
    print(json.dumps({"ok": ok, "checks": len(CHECKS)}))
    if args.out:
        Path(args.out).write_text(json.dumps(doc, indent=1) + "\n")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
