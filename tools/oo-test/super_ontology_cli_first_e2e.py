#!/usr/bin/env python3
"""D83 CLI-first end to end (WP4): one `oo` binary, a fresh HOME, no Desktop.

  fresh HOME -> files into a Drive -> a local Postgres as the database source -> link rule and projection
  -> `oo agent token create` -> the Agent (CLI and MCP, token file only) asks both sources -> every answer cites
  its source (a file anchor or a table row).

Negatives: a docs-only token cannot query the database, a write is refused, an Agent token cannot change the
engine, a revoked token is 401 on the CLI, on MCP and on the raw route, and the token text is on disk only in
files of mode 0600.

The database side is the real OO Gateway of this repo (tools/oo-test/super_ontology_gateway.py): `oo-gateway` over HTTPS with a
private CA, a role pack from `role-pack-bundle`, a client token from `issue-token`, SQL guard, per-role session and audit WAL,
on a private TLS PostgreSQL cluster. The table the engine links by is a second, plain table whose `credit_limit` differs on
purpose, so a number that reaches the Agent can only have come through the Gateway.

Stand-ins, stated plainly: both PostgreSQL clusters are private native clusters (initdb/pg_ctl, no container: the host already
runs two container stacks); the published cut of the Gateway is built by hand (no Builder run); the engine's read-only
login (Agent DB binding) is an engine HTTP call with the control token; the link rule and the projection run use `oo source link`.
Everything the Agent does goes through `oo`.

Usage: tools/oo-test/super_ontology_cli_first_e2e.py [--skip-build] [--out FILE] [--keep]
"""

import argparse
import http.client
import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.dont_write_bytecode = True
import super_ontology_l2 as l2  # noqa: E402
import super_ontology_gateway as realgw  # noqa: E402
import super_ontology_nodocker as nd  # noqa: E402

REPO = l2.REPO
LANE = os.environ.get("OO_TEST_LANE", "super-ontology-l1")
CHECKS = []


def _col(n, t, s):
    return {"name": n, "data_type": t, "semantic_type": s, "pii": "none",
            "classification": {"state": "confirmed", "by": "fixture/test"}, "provenance": "declared"}


# a second published table and a foreign key between them: what `oo ontology schema` has to show (docs-api 1.12)
realgw.RealGateway.EXTRA_DATASETS = [{"schema_version": "oos/v1", "dataset": "sales.orders", "label": "Orders", "tier": "raw", "primary_key": ["id"],
                                      "columns": [_col("id", "bigint", "identifier"), _col("customer_id", "bigint", "foreign_key"), _col("total", "bigint", "quantity")]}]
realgw.RealGateway.RELATIONSHIPS = [{"name": "orders_to_customers", "from": {"dataset": "sales.orders", "columns": ["customer_id"]},
                                     "to": {"dataset": "sales.customers", "columns": ["id"]}, "cardinality": "many_to_one", "join_hint": "left",
                                     "provenance": "declared_fk"}]


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
        subprocess.run(["cargo", "build", "--locked", "-p", "oo-gateway", "--bins", "-p", "oo-builder", "--bin", "role-pack-bundle"],
                       cwd=REPO, check=True)
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
        # the table the engine links by: same keys and names as the Gateway's cut, other credit limits on purpose
        pg.psql("postgres", "CREATE DATABASE srcdb")
        pg.psql("srcdb", "CREATE SCHEMA sales; CREATE TABLE sales.customers(id int primary key, name text, credit_limit int);"
                         "INSERT INTO sales.customers VALUES (1,'Acme Ltd',1),(2,'Globex',2)")
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
        srcdb = cfg / "secrets/srcdb"
        srcdb.write_text(pg.url("postgres", "srcdb"))
        srcdb.chmod(0o600)

        # the real Gateway, and the profiles the Agent tokens will name
        gw = realgw.RealGateway(work, target, [(1, "Acme Ltd", 50000), (2, "Globex", 12000)])
        gtf = cfg / "gw.token"
        gw_url = gw.start(gtf)
        gwtoken = gtf.read_text().strip()
        check("the real oo-gateway is up over TLS", gw_url.startswith("https://localhost:") and len(gwtoken) > 20, gw_url)
        (cfg / "profiles").mkdir(mode=0o700)
        (cfg / "profiles").chmod(0o700)
        ca = cfg / "gw-ca.pem"
        shutil.copy(gw.ca, ca)
        ca.chmod(0o600)
        bad = cfg / "gw-bad.token"
        bad.write_text("ooct_" + "x" * 43 + "\n")
        bad.chmod(0o600)
        for name, tokfile in (("salesgw", gtf), ("wrongtok", bad)):
            prof = cfg / f"profiles/{name}.json"
            prof.write_text(json.dumps({"schema_version": 1, "gateway": gw_url, "token_file": str(tokfile), "ca_file": str(ca)}))
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
        (drive / "Customers/澄海物流.md").write_text("---\ntype: organization\n---\n\n# 澄海物流\n\n合同每年三月续约，联系人：王芳。\n")
        code, d, err = cli.json("docs", "drive", "add", "--name", "default", "--root", str(drive))
        check("drive registered", code == 0 and d, err)
        deadline = time.time() + 120
        while time.time() < deadline:
            code, files, _ = cli.json("docs", "files", "ls", "--drive", "default")
            items = (files or {}).get("files", [])
            if len(items) == 3 and all(i["state"] == "published" for i in items):
                break
            time.sleep(0.5)
        else:
            raise SystemExit(f"files did not publish: {files}")
        check("every file published", True)

        # database source: link rule, Agent DB binding, projection (engine calls with the control token)
        api = l2.Api(port, control)
        drive_id = d["id"] if "id" in d else d["drive"]["id"]
        st, b = api.request("PUT", "/v1/docs/agent-db-binding", {"secret_ref": f"file:{srcdb}"})
        check("link source bound", st == 200, b)
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
        doc = next(i for i in s["items"] if i["side"] == "doc" and i.get("kind", "entity") == "entity")
        db = next(i for i in s["items"] if i["side"] == "db")
        check("doc item cites a file", doc["provenance"]["kind"] == "file" and doc["provenance"]["path"].endswith("acme.md"))
        check("db item cites a table row", db["provenance"]["kind"] == "table_row" and db["provenance"]["table"] == "sales.customers"
              and db["provenance"]["pk"] == "1" and str(db["provenance"]["publication_id"]).startswith("pub"), db)
        pub_id = db["provenance"]["publication_id"]
        check("urns are stable", doc["urn"].startswith("oo:doc/") and db["urn"] == "oo:db/salesgw/sales.customers/1")
        code, e, err = ag.json("ontology", "entity", doc["urn"], env=aenv, state=False)
        link = next((link for link in (e or {}).get("links", []) if link.get("db_table") == "sales.customers"), None)
        check("doc entity: facts cited, linked to the row", code == 0 and e["facts"] and all(f["provenance"] for f in e["facts"]) and link, (e, err))
        check("linked row comes with its provenance", link["row"]["columns"]["name"] == "Acme Ltd" and link["row"]["provenance"]["kind"] == "table_row")
        check("the linked row came through the Gateway, not the engine's table", link["row"]["columns"]["credit_limit"] == "50000"
              and link["row"]["provenance"]["publication_id"] == pub_id, link["row"])
        code, e, err = ag.json("ontology", "entity", db["urn"], env=aenv, state=False)
        check("row entity links back to the document", code == 0 and any(l["target"]["urn"].startswith("oo:doc/") for l in e["links"]), (e, err))
        code, q, err = ag.json("ontology", "query", "SELECT credit_limit FROM sales.customers WHERE name='Acme Ltd'", env=aenv, state=False)
        check("query cites the publication", code == 0 and q["rows"] == [["50000"]] and q["provenance"]["publication_id"] == pub_id, (q, err))
        code, out, err = ag.run("ontology", "search", "Acme", env=aenv, state=False)
        check("human output has [source] lines", code == 0 and out.count("[source]") >= 2, out)

        # full text over facts and blocks: English and Chinese, each hit with its file anchor
        code, s3, err = ag.json("ontology", "search", "contract renews March", env=aenv, state=False)
        blk = next((i for i in (s3 or {}).get("items", []) if i.get("kind") == "block"), None)
        check("full text finds a block by its words, with the file anchor", code == 0 and blk and blk["provenance"]["kind"] == "file"
              and blk["provenance"]["path"].endswith("acme.md") and blk["provenance"]["block_id"].startswith("blk_")
              and "renews every March" in blk["text"], (s3, err))
        code, s4, err = ag.json("ontology", "search", "三月续约", env=aenv, state=False)
        blk = next((i for i in (s4 or {}).get("items", []) if i.get("kind") == "block"), None)
        check("full text finds Chinese text", code == 0 and blk and blk["provenance"]["path"].endswith("澄海物流.md")
              and blk["provenance"]["anchor"] and "三月续约" in blk["text"], (s4, err))
        code, s5, err = ag.json("ontology", "search", "续约三月", env=aenv, state=False)
        check("a Chinese phrase is not a bag of characters", code == 0 and not [i for i in s5["items"] if i.get("kind") == "block"], (s5, err))
        code, s6, err = ag.json("ontology", "search", "澄海物流", env=aenv, state=False)
        kinds = {i.get("kind", "entity") for i in (s6 or {}).get("items", [])}
        check("a Chinese name finds the entity, its facts and its blocks", code == 0 and {"entity", "fact", "block"} <= kinds, (kinds, err))
        check("every document hit cites a file", all(i["provenance"]["kind"] == "file" and i["provenance"].get("path")
                                                        for i in s6["items"] if i["side"] == "doc"))
        code, s7, err = ag.json("ontology", "search", "contract renews March", env=aenv, state=False)
        check("the same query ranks the same way", code == 0 and [i["urn"] for i in s7["items"]] == [i["urn"] for i in s3["items"]])

        # the Gateway profile of the token decides which Gateway token the database side uses
        badp = home / "badprofile.tok"
        check("token naming a profile with a wrong Gateway token", cli.json(
            "agent", "token", "create", "--name", "badprofile", "--scope", "docs:read", "--scope", "db:read", "--gw-profile", "wrongtok",
            "--drive", "default", "--output-file", str(badp))[0] == 0)
        benv = {"OO_ENDPOINT": endpoint, "OO_AGENT_TOKEN_FILE": str(badp)}
        code, s8, err = ag.json("ontology", "search", "Acme", env=benv, state=False)
        check("the wrong profile gets no rows, the documents still answer", code == 0 and s8["sides"]["db"]["status"] == "error"
              and s8["sides"]["docs"]["status"] == "ok" and {i["side"] for i in s8["items"]} == {"doc"}, (s8, err))
        code, o, err = ag.json("ontology", "query", "SELECT credit_limit FROM sales.customers", env=benv, state=False)
        check("the wrong profile cannot query", code != 0 and "ONTOLOGY_GATEWAY_REFUSED" in err, err)

        # the schema surface: tables, columns, relationships and document links, scoped by the token, paged
        code, sc, err = ag.json("ontology", "schema", env=aenv, state=False)
        tables = {t["qualified_name"]: t for t in (sc or {}).get("tables", [])}
        check("schema lists the published tables with their columns", code == 0 and sc["protocol"] == "oo.ontology.schema/v1"
              and sorted(tables) == ["sales.customers", "sales.orders"] and sc["total_tables"] == 2 and sc["next_cursor"] is None, (sc, err))
        cust = {c["name"]: c for c in tables["sales.customers"]["columns"]}
        check("schema columns carry names, types and URNs (the Gateway lists them by name)", sorted(cust) == ["credit_limit", "id", "name"]
              and [c["name"] for c in tables["sales.customers"]["columns"]] == sorted(cust) and cust["id"]["type"] == "int8" and cust["name"]["type"] == "text"
              and tables["sales.customers"]["urn"] == "oo:db/salesgw/sales.customers"
              and cust["id"]["urn"] == "oo:db/salesgw/sales.customers#id", tables)
        check("a published table without a physical table still lists its columns, untyped",
              [c["name"] for c in tables["sales.orders"]["columns"]] == ["customer_id", "id", "total"]
              and tables["sales.orders"]["columns_status"] == "ok", tables["sales.orders"])
        rel = next((r for r in sc["relationships"] if r["id"] == "orders_to_customers"), None)
        check("schema shows the foreign key between the tables", rel and rel["from"]["table"] == "sales.orders" and rel["from"]["columns"] == ["customer_id"]
              and rel["to"]["table"] == "sales.customers" and rel["to"]["columns"] == ["id"] and rel["provenance"] == "declared_fk"
              and rel["cardinality"] == "many_to_one" and rel["urn"] == "oo:db/salesgw/relationship/orders_to_customers", sc["relationships"])
        dl = next((x for x in sc["doc_links"] if x["row_urn"] == db["urn"]), None)
        check("schema links the table's row to its document entity", dl and dl["target"]["urn"] == doc["urn"] and dl["table_urn"] == "oo:db/salesgw/sales.customers"
              and sc["sides"]["docs"]["status"] == "ok" and sc["sides"]["relationships"]["status"] == "ok", sc["doc_links"])
        check("schema cites the published pack", sc["provenance"]["kind"] == "gateway_schema" and sc["provenance"]["pack_digest"], sc["provenance"])
        code, p1, err = ag.json("ontology", "schema", "--limit", "1", env=aenv, state=False)
        code2, p2, err2 = ag.json("ontology", "schema", "--limit", "1", "--cursor", (p1 or {}).get("next_cursor") or "t1", env=aenv, state=False)
        seen = [t["qualified_name"] for t in p1["tables"] + p2["tables"]]
        check("schema pages the sorted table list", code == 0 and code2 == 0 and seen == ["sales.customers", "sales.orders"]
              and p1["next_cursor"] and p2["next_cursor"] is None, (p1, p2, err, err2))
        check("a relationship comes once, in the page of its from table", [r["id"] for r in p1["relationships"]] == []
              and [r["id"] for r in p2["relationships"]] == ["orders_to_customers"], (p1["relationships"], p2["relationships"]))
        code, o, err = ag.json("ontology", "schema", "--cursor", "garbage", env=aenv, state=False)
        check("a bad cursor is a 400", code != 0 and "ONTOLOGY_BAD_REQUEST" in err, err)
        code, out, err = ag.run("ontology", "schema", env=aenv, state=False)
        check("schema human output names tables, the key and the source", code == 0 and "sales.orders" in out and "rel orders_to_customers" in out
              and "[source] gateway salesgw schema" in out, out)
        code, o, err = ag.json("ontology", "schema", env={"OO_ENDPOINT": endpoint, "OO_AGENT_TOKEN_FILE": str(ro)}, state=False)
        check("docs-only token cannot read the database schema", code != 0 and "ONTOLOGY_SCOPE_DENIED" in err, err)
        code, o, err = ag.json("ontology", "schema", env={"OO_ENDPOINT": endpoint, "OO_AGENT_TOKEN_FILE": str(badp)}, state=False)
        check("the wrong profile cannot read the schema", code != 0 and "ONTOLOGY_GATEWAY_" in err, err)
        st, b = raw_call(port, bot.read_text().strip(), "POST", "/v1/ontology/schema", {})
        check("the schema route is read-only", st == 404, (st, b))

        # MCP, the same tools, the same token
        mcp = Mcp(bindir / "oo", {"PATH": base["PATH"], "HOME": str(work / "agent-home")}, endpoint, bot)
        r = mcp.rpc("tools/list", {})
        check("mcp lists the unified tools", [t["name"] for t in r["result"]["tools"]] == ["ontology_search", "ontology_entity", "ontology_sources", "ontology_query", "ontology_schema"])
        err_flag, res = mcp.tool("ontology_search", {"q": "Acme"})
        check("mcp search answers from both sources", not err_flag and {i["side"] for i in res["items"]} == {"doc", "db"}, res)
        err_flag, res = mcp.tool("ontology_search", {"q": "三月续约"})
        check("mcp full text finds Chinese blocks with the file anchor", not err_flag and any(
            i.get("kind") == "block" and i["provenance"]["path"].endswith("澄海物流.md") for i in res["items"]), res)
        err_flag, res = mcp.tool("ontology_query", {"sql": "SELECT credit_limit FROM sales.customers WHERE name='Acme Ltd'"})
        check("mcp query cites its source", not err_flag and res["provenance"]["publication_id"] == pub_id, res)

        # the schema over MCP: the same answer as the CLI, paged the same way, scoped by the same token
        err_flag, ms = mcp.tool("ontology_schema", {})
        check("mcp schema equals the CLI's answer", not err_flag and ms == sc, (ms, sc))
        err_flag, m1 = mcp.tool("ontology_schema", {"limit": 1})
        err_flag2, m2 = mcp.tool("ontology_schema", {"limit": 1, "cursor": (m1 or {}).get("next_cursor") or "t1"})
        check("mcp schema pages the sorted table list", not err_flag and not err_flag2
              and [t["qualified_name"] for t in m1["tables"] + m2["tables"]] == ["sales.customers", "sales.orders"] and m2["next_cursor"] is None, (m1, m2))
        err_flag, res = mcp.tool("ontology_schema", {"cursor": "garbage"})
        check("mcp schema refuses a bad cursor", err_flag and "ONTOLOGY_BAD_REQUEST" in json.dumps(res), res)
        err_flag, res = mcp.tool("ontology_schema", {"offset": 1})
        check("mcp schema takes only its two arguments", err_flag, res)
        mcp_docs = Mcp(bindir / "oo", {"PATH": base["PATH"], "HOME": str(work / "agent-home")}, endpoint, ro)
        try:
            err_flag, res = mcp_docs.tool("ontology_schema", {})
            check("mcp: a docs-only token cannot read the database schema", err_flag and "ONTOLOGY_SCOPE_DENIED" in json.dumps(res), res)
        finally:
            mcp_docs.close()

        cli.json("agent", "token", "revoke", "badprofile")

        # negatives
        code, o, err = ag.json("ontology", "query", "DELETE FROM sales.customers", env=aenv, state=False)
        check("a write is refused", code != 0 and "ONTOLOGY_READ_ONLY" in err, err)
        code, o, err = ag.json("ontology", "query", "SELECT 1; DROP TABLE sales.customers", env=aenv, state=False)
        check("a second statement is refused", code != 0 and "ONTOLOGY_READ_ONLY" in err, err)
        check("the databases are untouched", gw.psql(f"SELECT count(*) FROM _oo.{realgw.PHYSICAL}").strip() == "2"
              and pg.psql("srcdb", "SELECT count(*) FROM sales.customers") == "2")
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
        log = (gw.work / "gateway.stderr").read_text()
        paths = {ln.split("path=")[1].split()[0] for ln in log.splitlines() if "path=" in ln}
        check("the Gateway saw only the proxy's calls", paths and paths <= {"/v1/query", "/v1/freshness", "/v1/tables", "/v1/tables/item", "/v1/knowledge/item"}, paths)
        check("the Gateway admitted the queries in its audit WAL", gw.audit_records() > 0)
        check("no secret in the Gateway log", gwtoken not in log and gw.password not in log)

        # secrets on disk
        needles = [tok.read_text().strip(), gwtoken, gw.password, bot.read_text().strip(), ro.read_text().strip(), gtok, control]
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
        if gw:
            gw.stop()
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
