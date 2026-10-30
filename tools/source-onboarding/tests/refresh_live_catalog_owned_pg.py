#!/usr/bin/env python3
"""Real fixed-binary Gateway + TLS/CLI + new owned PG. Target fixture, NOT CDC.

Usage: python3 refresh_live_catalog_owned_pg.py --evidence DIR
  --onboarding PINNED_BIN --cli PINNED_OO --builder COMPATIBLE_BUILDER
  --gateway FIXED_GATEWAY

Adapted from crates/oo-gateway/tests/published_cut_owned_pg.py and its Rust
fixture. Only this newly created cluster/runtime is started/stopped. No issued
token API, production release activation, customer data or existing process.
"""
import argparse
import base64
import concurrent.futures
import copy
import fcntl
import hashlib
import json
import os
from pathlib import Path
import secrets
import socket
import ssl
import subprocess
import tempfile
import threading
import time
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[3]
PG = Path("/opt/homebrew/opt/postgresql@17/bin")
FIXED_GATEWAY_SHA = "23ad423096d4cc31daa0e0042dc2afcf7d616f1bfaa1419b9c878779f4ea640c"
TABLES = ["items", "other"]
D = "c" * 64
COLUMN = "pc_" + "b" * 60
ROLE = "oo_r_f22"
JOIN = "SELECT a.amount AS left_amount,b.amount AS right_amount FROM app.items a JOIN app.other b ON a.amount=b.amount"


def encode(v):
    return json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()


def sha(v):
    return hashlib.sha256(v if isinstance(v, bytes) else encode(v)).hexdigest()


def write(path, value):
    data = value if isinstance(value, bytes) else encode(value) + b"\n"
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    with path.open("wb") as f:
        os.fchmod(f.fileno(), 0o600)
        f.write(data)
        f.flush()
        os.fsync(f.fileno())


def replace(path, data):
    tmp = path.with_name(".fixture-replace")
    write(tmp, data)
    os.replace(tmp, path)
    fd = os.open(path.parent, os.O_RDONLY)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def wait(test, message, timeout=30):
    deadline = time.monotonic() + timeout
    while not test():
        assert time.monotonic() < deadline, message
        time.sleep(0.025)


class Fixture:
    def __init__(self, args):
        self.args = args
        self.root = Path(tempfile.mkdtemp(prefix="live-catalog-owned-", dir=args.evidence)).resolve()
        self.runtime = self.root / "runtime"
        self.runtime.mkdir(mode=0o700)
        self.pg = self.root / "pg"
        self.port, self.gwport = free_port(), free_port()
        self.origin = f"https://127.0.0.1:{self.gwport}"
        self.password = secrets.token_hex(24)
        self.token = "ooct_" + base64.urlsafe_b64encode(secrets.token_bytes(32)).decode().rstrip("=")
        self.env = dict(os.environ, PGPASSWORD=self.password)
        self.gateway = None
        self.started_pg = False
        self.staged = {}
        self.records = []
        self.cleanup = {"gateway_started": False, "postgres_started": False}
        self.catalog_path = self.runtime / "reviewed/library/library.json"
        self.process_files = []

    def command(self, args, label, data=None, env=None, check=True, timeout=120):
        proc = subprocess.run([str(x) for x in args], input=data, capture_output=True,
                              env=env, timeout=timeout)
        write(self.root / f"{label}.stdout", proc.stdout)
        write(self.root / f"{label}.stderr", proc.stderr)
        write(self.root / f"{label}.exit.json", {"exit": proc.returncode})
        if check:
            assert proc.returncode == 0, f"{label}: {proc.returncode} {proc.stderr.decode()}"
        return proc

    def sql(self, statement, label="sql"):
        return self.command(self.psql(), label, statement.encode(), self.env).stdout

    def psql(self):
        return [PG / "psql", "-X", "-qAt", "-v", "ON_ERROR_STOP=1", "-h", "127.0.0.1",
                "-p", str(self.port), "-U", "postgres", "-d", "live_catalog_owned"]

    def start(self):
        assert sha(Path(self.args.gateway).read_bytes()) == FIXED_GATEWAY_SHA
        write(self.root / "password", self.password.encode())
        self.command([PG / "initdb", "-D", self.pg, "-U", "postgres", "--auth-local=trust",
                      "--auth-host=scram-sha-256", "--pwfile", self.root / "password"], "initdb")
        self.command(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes",
                      "-keyout", self.root / "ca.key", "-out", self.root / "ca.crt", "-days", "2",
                      "-subj", "/CN=Owned live catalog fixture CA",
                      "-addext", "basicConstraints=critical,CA:TRUE",
                      "-addext", "keyUsage=critical,keyCertSign,cRLSign",
                      "-addext", "subjectKeyIdentifier=hash"], "tls-ca")
        for kind in ["server", "client"]:
            self.command(["openssl", "req", "-newkey", "rsa:2048", "-nodes",
                          "-keyout", self.root / f"{kind}.key", "-out", self.root / f"{kind}.csr",
                          "-subj", f"/CN={kind}"], f"tls-{kind}-csr")
            ext = ("basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\n"
                   "subjectKeyIdentifier=hash\nauthorityKeyIdentifier=keyid:always\n"
                   + ("extendedKeyUsage=serverAuth\nsubjectAltName=DNS:localhost,IP:127.0.0.1\n"
                      if kind == "server" else
                      "extendedKeyUsage=clientAuth\nsubjectAltName=URI:urn:oo:workload:owned-live-catalog\n"))
            write(self.root / f"{kind}.ext", ext.encode())
            self.command(["openssl", "x509", "-req", "-in", self.root / f"{kind}.csr",
                          "-CA", self.root / "ca.crt", "-CAkey", self.root / "ca.key",
                          "-CAcreateserial", "-out", self.root / f"{kind}.crt", "-days", "2",
                          "-extfile", self.root / f"{kind}.ext"], f"tls-{kind}-sign")
            (self.root / f"{kind}.key").chmod(0o600)
        with (self.pg / "postgresql.conf").open("a") as f:
            f.write(f"\nlisten_addresses='127.0.0.1'\nport={self.port}\nunix_socket_directories=''\n"
                    f"ssl=on\nssl_cert_file='{self.root / 'server.crt'}'\n"
                    f"ssl_key_file='{self.root / 'server.key'}'\nmax_connections=40\n")
        self.command([PG / "pg_ctl", "-D", self.pg, "-l", self.root / "postgres.log", "-w", "start"], "pg-start")
        self.started_pg = True
        self.cleanup["postgres_started"] = True
        self.pgpid = int((self.pg / "postmaster.pid").read_text().splitlines()[0])
        argv = self.psql()
        argv[-1] = "postgres"
        self.command(argv, "database-setup", f"""CREATE DATABASE live_catalog_owned;
CREATE ROLE oo_token_runtime LOGIN NOINHERIT PASSWORD '{self.password}';
CREATE ROLE oo_f22_login LOGIN NOINHERIT PASSWORD '{self.password}';
CREATE ROLE oo_r_f22 NOLOGIN NOINHERIT;
GRANT oo_r_f22 TO oo_f22_login WITH INHERIT FALSE, SET TRUE;
""".encode(), self.env)
        self.schema()
        self.pack()
        for generation in [1, 2, 3, 4, 5]:
            self.stage(generation)
        self.activate_head(1)
        catalog = self.base_catalog | {"publication": self.staged[1], "bindings": [
            {"dataset": f"app.{t}", "source": "f22", "config_digest": D,
             "generation": 1, "type_digest": D, "input_digest": self.input_digest(1, n)}
            for n, t in enumerate(TABLES)]}
        write(self.catalog_path, catalog)
        for name in ["packs", "wal", "collector"]:
            (self.runtime / name).mkdir(mode=0o700, exist_ok=True)
        write(self.runtime / "packs/library.json", self.artifact)
        endpoint = {"host": "127.0.0.1", "port": self.port, "database": "live_catalog_owned"}
        write(self.runtime / "endpoint.json", endpoint)
        for source, dest in [("password", "password"), ("ca.crt", "ca.crt"),
                             ("server.crt", "tls.crt"), ("server.key", "tls.key")]:
            write(self.runtime / dest, (self.root / source).read_bytes())
        write(self.root / "token", self.token.encode())
        write(self.root / "client.pem", (self.root / "client.crt").read_bytes() + (self.root / "client.key").read_bytes())
        self.runtime_config = {
            "listen": f"127.0.0.1:{self.gwport}", "transport": "tls",
            "audience": self.origin + "/v1", "runtime_user": "oo_token_runtime",
            "password_file": "password", "ca_file": "ca.crt", "endpoint_file": "endpoint.json",
            "pack_dir": "packs", "body_limit": 1048576, "request_timeout_ms": 30000,
            "global_concurrency": 8, "tls_key_file": "tls.key", "tls_chain_file": "tls.crt",
            "client_ca_file": "ca.crt", "handshake_timeout_ms": 10000,
            "data_endpoint_file": "endpoint.json", "data_password_file": "password",
            "data_ca_file": "ca.crt", "data_user": "oo_f22_login", "wal_dir": "wal",
            "reviewed_dir": "reviewed", "audit_collector_dir": "collector",
        }
        write(self.runtime / "config.json", self.runtime_config)
        now = int(time.time() * 1000)
        # Fixture genesis credential only: no token-issuing product/API command.
        self.sql(f"""INSERT INTO oo_token_control.token_record VALUES
('{sha(self.token.encode())}','client','f22','alice','{ROLE}','{{}}','pv1','kv1',
'{self.origin}/v1',NULL,NULL,{now},NULL,{now + 86400000},{now},0);
""", "fixture-auth")
        out = (self.root / "gateway.stdout").open("wb")
        err = (self.root / "gateway.stderr").open("wb")
        self.process_files.extend([out, err])
        self.gateway = subprocess.Popen([self.args.gateway, str(self.runtime)], stdout=out, stderr=err)
        self.cleanup["gateway_started"] = True
        wait(lambda: (self.runtime / "listen.addr").exists() or self.gateway.poll() is not None, "gateway start")
        assert self.gateway.poll() is None, (self.root / "gateway.stderr").read_text()
        self.gwpid = self.gateway.pid
        write(self.root / "ownership.json", {"owned_fixture": "live-catalog-owned/v1",
            "postmaster_pid": self.pgpid, "gateway_pid": self.gwpid, "runtime": str(self.runtime),
            "database": endpoint, "gateway_binary_sha256": FIXED_GATEWAY_SHA,
            "token_sha256": sha(self.token.encode()), "pack_digest": self.artifact["pack_digest"],
            "fixture_is_source_ingestion": False, "release_receipts": "target-fixture genesis, not coordinator E2E"})
        self.context = ssl.create_default_context(cafile=str(self.root / "ca.crt"))
        self.context.load_cert_chain(str(self.root / "client.pem"))

    def schema(self):
        migration = (ROOT / "tools/gateway-token-store/migrations/oo_token_control_v1.sql").read_text()
        self.sql(migration, "auth-schema")
        self.sql(f"INSERT INTO oo_token_control.install_manifest VALUES(true,'{sha(migration.encode())}');", "auth-manifest")
        self.sql("""
CREATE SCHEMA _oo; CREATE SCHEMA oo_publish_target;
CREATE TABLE _oo.physical_mappings(source text,config_digest text,run_generation int8,
 level text,logical_namespace text,logical_table text,logical_column text,physical_name text);
CREATE FUNCTION _oo.lookup_physical_mapping(text,text,int8,text,text,text,text) RETURNS text
 LANGUAGE sql STABLE AS $$ SELECT physical_name FROM _oo.physical_mappings
 WHERE source=$1 AND config_digest=$2 AND run_generation=$3 AND level=left($4,1)
 AND logical_namespace=$5 AND logical_table=$6 AND logical_column=$7 $$;
CREATE TABLE _oo.table_freshness(source text,schema_name text,table_name text,physical_name text,
 source_generation int8,target_generation int8,state text,source_committed_at timestamptz,applied_at timestamptz);
CREATE TABLE oo_publish_target.fence(role_name text,schema_name text,relation_name text,
 state text,opened_pack_digest text,opened_version text);
CREATE TABLE _oo.full_import_run(run_id uuid PRIMARY KEY,source_id text,
 source_generation int8,target_generation int8,config_digest text,schema_digest text,
 type_digest text,input_digest text,shard_id text,sealed_at timestamptz,state text);
CREATE TABLE _oo.full_import_table(run_id uuid,schema_ident text,table_ident text,physical_name text,
 exhausted bool,applied_rows int8,applied_pos numeric,canonical_digest text);
CREATE TABLE _oo.apply_receipts(source text,table_generation text,config_digest text,schema_digest text,
 type_digest text,fence_shard text,fence_generation int8,applied_pos numeric,position_codec text,
 position_lineage text,source_epoch int4,cross_table_consistent bool,partial_txn bool,target_commit_at timestamptz);
CREATE TABLE _oo.full_import_quality(physical_name text,kind text,rejected_rows int8,accepted_rows int8);
CREATE TABLE _oo.source_watermark(source text PRIMARY KEY,source_generation int8,target_generation int8,state text,applied_at timestamptz);
CREATE TABLE _oo.table_quality(schema_name text,table_name text,capture_method text);
CREATE TABLE _oo.native_library_publication(receipt_sha256 text PRIMARY KEY,library_id text,library jsonb,shards jsonb);
CREATE TABLE _oo.native_library_head(library_id text PRIMARY KEY,receipt_sha256 text,manifest_sha256 text,state text);
CREATE FUNCTION _oo.fixture_immutable() RETURNS trigger LANGUAGE plpgsql AS
 $$ BEGIN RAISE EXCEPTION 'immutable owned publication'; END $$;
CREATE TRIGGER immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON _oo.native_library_publication
 FOR EACH STATEMENT EXECUTE FUNCTION _oo.fixture_immutable();
CREATE TRIGGER immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON _oo.full_import_run
 FOR EACH STATEMENT EXECUTE FUNCTION _oo.fixture_immutable();
CREATE TRIGGER immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON _oo.full_import_table
 FOR EACH STATEMENT EXECUTE FUNCTION _oo.fixture_immutable();
GRANT USAGE ON SCHEMA _oo TO oo_r_f22,oo_f22_login;
GRANT USAGE ON SCHEMA oo_publish_target TO oo_r_f22,oo_f22_login;
GRANT SELECT ON ALL TABLES IN SCHEMA _oo,oo_publish_target TO oo_f22_login,oo_r_f22;
ALTER ROLE oo_f22_login SET default_transaction_read_only=on;
""", "metadata-schema")

    def pack(self):
        effective = {"protocol": "oo.builder.effective-oos/v1", "project": "f22",
            "ontology_version": "oos-1", "catalog_version": "cat-1",
            "policy": {"id": "policy", "version": "pv1"}, "knowledge": {"id": "knowledge", "version": "kv1"},
            "ontology": {"spec_version": "oos/v1", "name": "f22", "locale": "en-US",
                "include_schemas": ["app"], "default_tier_preference": ["raw"],
                "publish": {"include_unreviewed": []}, "classification": {"auto_accept": "none"},
                "lifecycle": {"deprecation_min_releases": 1}, "compile": {"per_role": True,
                    "budgets": {"readme_tokens": 2000, "table_file_tokens": 3000},
                    "sample_values": {"default": "none", "max_per_column": 0}}},
            "datasets": [{"schema_version": "oos/v1", "dataset": f"app.{t}", "label": t, "tier": "raw",
                "primary_key": ["amount"], "columns": [{"name": "amount", "data_type": "bigint",
                    "semantic_type": "identifier", "pii": "none", "classification": {
                        "state": "confirmed", "by": "fixture/test"}, "provenance": "declared"}]} for t in TABLES]}
        effective.update({key: [] for key in ["relationships", "enums", "glossary", "entities", "metrics", "examples"]})
        visibility = {"protocol": "oo.builder.role-visibility/v1", "project": "f22",
            "ontology_version": "oos-1", "catalog_version": "cat-1", "policy": effective["policy"],
            "knowledge": effective["knowledge"], "principal": "alice", "role": ROLE,
            "issued_for_compiler": "oo-builder/role-pack/v1",
            "datasets": [{"dataset": f"app.{t}", "columns": ["amount"], "row_security": "none",
                          "global_stats": "denied"} for t in TABLES],
            "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 128}}
        request = {"protocol": "oo.builder.role-pack-command/v1", "effective_oos": effective, "visibility": visibility}
        write(self.root / "pack-input.json", request)
        result = self.command([self.args.builder, "role-pack"], "fixture-pack", encode(request))
        self.artifact = json.loads(result.stdout)["artifact"]
        review = {"review_origin": "operator_pinned_file", "project": "f22", "role": ROLE, "principal": "alice",
                  "columns": {f"app.{t}.amount": {"pii": "none", "semantic_type": "identifier",
                     "reviewer": "fixture/test"} for t in TABLES}, "hidden_columns": []}
        producer = {"protocol": "oo.selfbuild.library-pack/v1", "published": False, "business_owner_signed": False,
            "artifact": self.artifact, "pack_digest": self.artifact["pack_digest"], "effective_sha256": sha(effective),
            "visibility_sha256": sha(visibility), "review_sha256": sha(review)}
        self.base_catalog = {"protocol": "oo.gateway.library-reviewed/v1",
            "envelope": {"contract": "oo.operator.library-pack/v1", "operation": "prepareLocalBuildPack",
                "management_identity": "operator_local", "producer_authenticated": False,
                "published": False, "active": False, "compiled_pack": "not_published", "data_grants": "none",
                "producer": producer, "output_sha256": sha(producer), "review": review},
            "effective": effective, "visibility": visibility, "login": "oo_f22_login", "ceilings": {}}

    def input_digest(self, generation, number):
        return sha(f"owned-input-{generation}-{number}".encode())

    def stage(self, generation):
        tables, statements = [], []
        for n, table in enumerate(TABLES):
            run = f"00000000-0000-4000-8000-{generation * 10 + n:012}"
            physical = f"pt_{generation * 10 + n:060x}"
            input_digest = "NULL" if generation == 3 and n == 0 else f"'{self.input_digest(generation, n)}'"
            type_digest = f"'{'e' * 64}'" if generation == 4 and n == 0 else f"'{D}'"
            statements.append(f"""
CREATE TABLE _oo.{physical}({COLUMN} int8, secret text);
INSERT INTO _oo.{physical} VALUES({generation * 10},'not-public');
GRANT SELECT({COLUMN}) ON _oo.{physical} TO {ROLE};
INSERT INTO _oo.physical_mappings VALUES('f22','{D}',{generation},'t','app','{table}','','{physical}'),
 ('f22','{D}',{generation},'c','app','{table}','amount','{COLUMN}');
INSERT INTO _oo.full_import_run VALUES('{run}','f22',{generation},{generation},'{D}','{D}',
 {type_digest},{input_digest},'fixture',now(),'published');
INSERT INTO _oo.full_import_table VALUES('{run}','app','{table}','{physical}',true,1,0,'{D}');
INSERT INTO _oo.apply_receipts VALUES('f22','{physical}','{D}','{D}','{D}','fixture',{generation},0,
 'snapshot-zero/v1','full-import.snapshot',0,false,false,'2026-10-20T00:00:0{generation}Z');
INSERT INTO _oo.full_import_quality VALUES('{physical}','batch',0,1);
INSERT INTO oo_publish_target.fence VALUES('{ROLE}','_oo','{physical}','open','{self.artifact["pack_digest"]}','release{generation}');
""")
            tables.append({"schema": "app", "table": table, "run_id": run, "physical_name": physical,
                "source_generation": generation, "target_generation": generation, "accepted_rows": 1,
                "canonical_digest": D, "applied_pos": "0", "capture_method": "c5_diff",
                "delete_detection": "reconcile", "consistency_grade": "B"})
        statements.append(f"INSERT INTO _oo.physical_mappings VALUES('f22','{D}',{generation},'n','app','','','pn_{'a' * 60}');")
        receipt = {"protocol": "oo.pipeline.native-publication-receipt/v1", "collection_mode": "per_table",
            "run_id": tables[0]["run_id"], "run_ids": [t["run_id"] for t in tables], "source_id": "f22",
            "source_generation": generation, "target_generation": generation, "position_codec": "snapshot-zero/v1",
            "position_lineage": "configured.mysql", "applied_pos": "0", "consistency": "per_table",
            "consistency_grade": "B", "cross_table_consistent": False, "tables": tables}
        manifest = {"protocol": "oo.library.membership/v1", "library_id": "fixture",
                    "sources": [{"source_id": "f22", "schema": "app", "tables": TABLES}]}
        library = {"protocol": "oo.pipeline.native-library-publication/v1", "library_id": "fixture",
            "manifest_sha256": sha(manifest), "manifest": manifest, "shards": ["f22"], "expected_tables": 2}
        raw = {"library": library, "shards": [{"source_id": "f22", "anchor_run": receipt["run_id"],
                                           "receipt_sha256": sha(receipt), "receipt": receipt}]}
        p = {"protocol": "oo.pipeline.native-library-publication/v1", "run_id": receipt["run_id"],
             "receipt_sha256": sha(raw), "receipt": raw}
        self.staged[generation] = p
        statements.append(f"INSERT INTO _oo.native_library_publication VALUES('{sha(raw)}','fixture',"
                          f"'{encode(library).decode()}'::jsonb,'{encode(raw['shards']).decode()}'::jsonb);")
        self.sql("BEGIN;" + "".join(statements) + "COMMIT;", f"stage-{generation}")
        published = {"prepared": {"precheck": "rolled_back", "native_mapping": {"publication": p}},
            "release": {"knowledge_published": True, "pack_digest": self.artifact["pack_digest"],
                "native_publication": {"receipt_sha256": p["receipt_sha256"], "library_id": "fixture",
                                       "ready": True, "state": "ready"}}}
        write(self.root / f"publication-{generation}.json", published)

    def activate_head(self, generation):
        p = self.staged[generation]
        self.sql(f"""INSERT INTO _oo.native_library_head VALUES('fixture','{p["receipt_sha256"]}',
 '{p["receipt"]["library"]["manifest_sha256"]}','ready') ON CONFLICT(library_id) DO UPDATE
 SET receipt_sha256=EXCLUDED.receipt_sha256,state='ready';""", f"activate-head-{generation}-{len(self.records)}")

    def config(self, generation):
        return {"protocol": "oo.library.refresh-live-catalog/v1", "gateway_root": str(self.runtime),
            "publication_receipt": str(self.root / f"publication-{generation}.json"),
            "expected_pack_digest": self.artifact["pack_digest"], "expected_manifest_sha256": self.artifact["manifest_sha256"],
            "schema_scope_digest": sha([f"app.{t}" for t in TABLES]), "metadata_argv": [str(x) for x in self.psql()],
            "cli_binary": {"path": self.args.cli, "sha256": sha(Path(self.args.cli).read_bytes())},
            "gateway": self.origin, "token_file": str(self.root / "token"), "ca_file": str(self.root / "ca.crt"),
            "client_pem": str(self.root / "client.pem"), "queries": [
                {"sql": f"SELECT amount FROM app.{t}", "expected_rows": [[str(generation * 10)]]} for t in TABLES]
                + [{"sql": JOIN, "expected_rows": [[str(generation * 10), str(generation * 10)]]},
                   {"sql": "SELECT COUNT(*) FROM app.items", "expected_rows": [["1"]]}]}

    def call(self, label, config, dry=False, code=0):
        path = self.root / f"{label}.config.json"
        write(path, config)
        args = [self.args.onboarding, "refresh-live-catalog", "--config", path, "--out", self.root / "attempts"]
        if dry:
            args.append("--dry-run")
        before = self.catalog_path.read_bytes()
        st = self.catalog_path.stat()
        result = self.command(args, label, env=self.env, check=False)
        record = {"label": label, "exit": result.returncode, "dry_run": dry,
                  "catalog_before": sha(before), "catalog_after": sha(self.catalog_path.read_bytes())}
        self.records.append(record)
        assert result.returncode == code, (label, result.stdout.decode(), result.stderr.decode())
        if dry or code != 0:
            record["unchanged"] = before == self.catalog_path.read_bytes()
        if dry:
            assert record["unchanged"] and self.catalog_path.stat().st_ino == st.st_ino
            detail = json.loads(result.stdout)["detail"] if code == 0 else None
            if detail:
                assert detail["prepared"] and not detail["activated"] and not detail["verified"]
        if code != 0:
            return record
        response = json.loads(result.stdout)
        assert response["success"] and response["pack_digest"] == config["expected_pack_digest"]
        assert response["schema_scope_digest"] == config["schema_scope_digest"]
        if dry:
            assert "publication_binding" not in response
        else:
            assert response["publication_binding"] == response["detail"]["verification"]["publication_binding"]
        return response

    def http(self, binding="current", sql=JOIN):
        headers = {"authorization": f"Bearer {self.token}", "content-type": "application/json",
                   "x-oo-expected-publication-id": binding}
        req = urllib.request.Request(self.origin + "/v1/query",
            data=encode({"sql": sql, "format": "json"}), headers=headers)
        try:
            with urllib.request.urlopen(req, context=self.context, timeout=30) as response:
                return {"status": response.status, "binding": response.headers.get("x-oo-publication-id"),
                        "vector": response.headers.get("x-oo-data-vector-sha256"), "body": json.loads(response.read())}
        except urllib.error.HTTPError as response:
            return {"status": response.code, "body": json.loads(response.read())}

    def exercise(self):
        old = self.http()
        write(self.root / "old-cut-query.json", old)
        assert old["status"] == 200 and old["body"]["rows"] == [["10", "10"]], old
        old_count = self.http(old["binding"], "SELECT COUNT(*) FROM app.items")
        assert old_count["body"]["rows"] == [["1"]]
        old_bytes = self.catalog_path.read_bytes()
        self.call("same-old-dry", self.config(1), dry=True)
        r = self.call("wronghead", self.config(2), dry=True, code=2)
        assert r["unchanged"] and "wrong_head" in (self.root / "wronghead.stderr").read_text()
        assert self.call("wronghead-actual", self.config(2), code=2)["unchanged"]
        for generation, label in [(3, "missing-input-digest"), (4, "mismatched-type-digest")]:
            self.activate_head(generation)
            r = self.call(label, self.config(generation), dry=True, code=2)
            assert r["unchanged"] and "immutable_run_table" in (self.root / f"{label}.stderr").read_text()
            assert self.call(label + "-actual", self.config(generation), code=2)["unchanged"]
        self.activate_head(2)
        self.sql("UPDATE oo_publish_target.fence SET opened_pack_digest='" + "0" * 64
                 + "' WHERE opened_version='release2';", "wrong-target-pack")
        assert self.call("wrong-target-pack", self.config(2), dry=True, code=2)["unchanged"]
        assert self.call("wrong-target-pack-actual", self.config(2), code=2)["unchanged"]
        self.sql(f"UPDATE oo_publish_target.fence SET opened_pack_digest='{self.artifact['pack_digest']}'"
                 " WHERE opened_version='release2';", "restore-target-pack-fixture")
        for label, field, value in [("wrong-pack", "expected_pack_digest", "0" * 64),
                                    ("truncated-scope", "schema_scope_digest", sha(["app.items"]))]:
            c = self.config(2)
            c[field] = value
            assert self.call(label, c, dry=True, code=2)["unchanged"]
            assert self.call(label + "-actual", c, code=2)["unchanged"]
        for label, pointer in [("release-not-published", "release"), ("raw-receipt-hash", "hash")]:
            c = self.config(2)
            p = json.loads(Path(c["publication_receipt"]).read_text())
            if pointer == "release":
                p["release"]["knowledge_published"] = False
            else:
                p["prepared"]["native_mapping"]["publication"]["receipt"]["library"]["expected_tables"] = 1
            path = self.root / f"{label}.publication.json"
            write(path, p)
            c["publication_receipt"] = str(path)
            assert self.call(label, c, dry=True, code=2)["unchanged"]
        c = self.config(2)
        truncated = json.loads(Path(c["publication_receipt"]).read_text())
        p = truncated["prepared"]["native_mapping"]["publication"]
        library, shard = p["receipt"]["library"], p["receipt"]["shards"][0]
        library["manifest"]["sources"][0]["tables"] = ["items"]
        library["manifest_sha256"] = sha(library["manifest"])
        library["expected_tables"] = 1
        shard["receipt"]["tables"] = shard["receipt"]["tables"][:1]
        shard["receipt"]["run_ids"] = shard["receipt"]["run_ids"][:1]
        shard["receipt_sha256"] = sha(shard["receipt"])
        p["receipt_sha256"] = sha(p["receipt"])
        truncated["release"]["native_publication"]["receipt_sha256"] = p["receipt_sha256"]
        path = self.root / "truncated-valid-hashes.publication.json"
        write(path, truncated)
        c["publication_receipt"] = str(path)
        c["schema_scope_digest"] = sha(["app.items"])
        assert self.call("truncated-valid-hashes", c, dry=True, code=2)["unchanged"]
        assert "exact_scope" in (self.root / "truncated-valid-hashes.stderr").read_text()
        assert self.call("truncated-valid-hashes-actual", c, code=2)["unchanged"]
        with (self.runtime / ".refresh-live-catalog.lock").open("r+") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            assert self.call("lock-busy", self.config(2), dry=True, code=2)["unchanged"]
        # The active target is new but the old catalog is still intact; dry-run must
        # not require a now-drifting old-cut query to succeed.
        self.call("new-cut-prepared", self.config(2), dry=True)
        assert self.catalog_path.read_bytes() == old_bytes
        self.cas_rejection()
        self.cas_rejection(dry=False)
        assert self.catalog_path.read_bytes() == old_bytes
        self.activate_head(1)
        self.sql("""INSERT INTO _oo.source_watermark VALUES('f22',999,999,'pending',now());
INSERT INTO _oo.table_freshness VALUES('f22','app','items','pt_not_published',999,999,'pending','2099-01-01Z',now());
INSERT INTO _oo.table_quality VALUES('app','items','pending-poison');""", "poison-mutable-freshness")
        pending = self.http(old["binding"])
        write(self.root / "pending-query.json", pending)
        assert pending["status"] == 200 and pending["body"]["rows"] == [["10", "10"]]
        assert pending["binding"] == old["binding"] and pending["vector"] == old["vector"]
        stop = threading.Event()
        reads = []
        disk_reads = []

        def reader():
            while not stop.is_set():
                reads.append(self.http())
                time.sleep(0.01)

        def disk_reader():
            while not stop.is_set():
                data = json.loads(self.catalog_path.read_bytes())
                disk_reads.append((data["publication"]["receipt_sha256"],
                                   tuple(b["generation"] for b in data["bindings"])))
                time.sleep(0.002)

        with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
            futures = [pool.submit(reader) for _ in range(2)] + [pool.submit(disk_reader)]
            try:
                wait(lambda: len(reads) >= 4, "old reads")
                self.activate_head(2)
                wait(lambda: any(r["status"] != 200 for r in reads), "drift reads")
                result = self.call("hot-reload", self.config(2))
                assert result["catalog_replaced"] and result["detail"]["verified"]
                wait(lambda: any(r["body"].get("rows") == [["20", "20"]] for r in reads), "new reads")
            finally:
                stop.set()
                for future in futures:
                    future.result(timeout=60)
        write(self.root / "concurrent-http.json", reads)
        write(self.root / "concurrent-catalog.json", disk_reads)
        allowed = {(self.staged[g]["receipt_sha256"], (g, g)) for g in [1, 2]}
        assert set(disk_reads) <= allowed and set(disk_reads) == allowed
        for response in reads:
            if response["status"] == 200:
                assert response["body"]["rows"] in [[["10", "10"]], [["20", "20"]]], response
            else:
                assert "PUBLICATION_DRIFT" in json.dumps(response["body"]), response
        new = self.http()
        assert new["status"] == 200 and new["body"]["rows"] == [["20", "20"]]
        assert new["binding"] != old["binding"]
        new_count = self.http(new["binding"], "SELECT COUNT(*) FROM app.items")
        assert new_count["body"]["rows"] == old_count["body"]["rows"] == [["1"]]
        stale = self.http(old["binding"], "SELECT COUNT(*) FROM app.items")
        write(self.root / "stale-binding.json", stale)
        write(self.root / "equal-count-different-cut.json", {"old": old_count, "new": new_count, "stale": stale})
        assert stale["status"] != 200 and "PUBLICATION_DRIFT" in json.dumps(stale)
        new_bytes, inode = self.catalog_path.read_bytes(), self.catalog_path.stat().st_ino
        replay = self.call("replay", self.config(2))
        assert not replay["catalog_replaced"] and replay["detail"]["same_receipt"]
        assert self.catalog_path.read_bytes() == new_bytes and self.catalog_path.stat().st_ino == inode
        new_catalog, old_catalog = json.loads(new_bytes), json.loads(old_bytes)
        for key in old_catalog:
            if key not in ["publication", "bindings"]:
                assert new_catalog[key] == old_catalog[key], key
        # Deliberately fail a post-rename oracle on a later forward cut. Keep the
        # actual installed cut; no hidden rollback or false verification success.
        self.activate_head(5)
        bad = self.config(5)
        bad["queries"][0]["expected_rows"] = [["wrong"]]
        failed = self.call("post-rename-query-failure", bad, code=2)
        assert not failed["unchanged"]
        attempts = sorted((self.root / "attempts").glob("attempt-*"))
        failed_receipt = json.loads((attempts[-1] / "result.json").read_text())
        assert failed_receipt["catalog_replaced"] and not failed_receipt["rollback_attempted"]
        assert json.loads(self.catalog_path.read_bytes())["publication"] == self.staged[5]
        final = self.call("recover-same-receipt", self.config(5))
        assert not final["catalog_replaced"] and final["detail"]["verified"]
        assert self.gateway.poll() is None and self.gateway.pid == self.gwpid
        assert int((self.pg / "postmaster.pid").read_text().splitlines()[0]) == self.pgpid
        acceptance = {"success": True, "gateway_pid_before": self.gwpid, "gateway_pid_after": self.gateway.pid,
            "gateway_restarts": 0, "token_rotations": 0, "pack_digest": self.artifact["pack_digest"],
            "gateway_binary_sha256": FIXED_GATEWAY_SHA, "old_binding": old["binding"], "new_binding": new["binding"],
            "catalog_read_count": len(disk_reads), "http_read_count": len(reads),
            "old_reads": sum(r["body"].get("rows") == [["10", "10"]] for r in reads),
            "new_reads": sum(r["body"].get("rows") == [["20", "20"]] for r in reads),
            "explicit_drift_refusals": sum(r["status"] != 200 for r in reads),
            "only_complete_old_or_new": True, "cases": self.records,
            "post_rename_failure_retained_no_rollback": True,
            "equal_count_does_not_authorize_stale_cut": True,
            "source_time": final["detail"]["verification"]["freshness"].get("source_time"),
            "not_customer_deployment": True, "not_source_ingestion_or_full_cdc": True}
        write(self.root / "acceptance.json", acceptance)
        return acceptance

    def cas_rejection(self, dry=True):
        label = "cas-dry" if dry else "cas-actual"
        with (self.root / f"{label}-block.stderr").open("wb") as err:
            block = subprocess.Popen(self.psql(), stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                     stderr=err, env=self.env)
            try:
                block.stdin.write(b"BEGIN; LOCK TABLE _oo.native_library_head IN ACCESS EXCLUSIVE MODE; SELECT 'LOCKED';\n")
                block.stdin.flush()
                assert block.stdout.readline().strip() == b"LOCKED"
                config = self.root / f"{label}.config.json"
                write(config, self.config(2))
                out = self.root / f"{label}-attempts"
                args = [self.args.onboarding, "refresh-live-catalog", "--config", str(config), "--out", str(out)]
                if dry:
                    args.append("--dry-run")
                cli = subprocess.Popen(args, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=self.env)
                wait(lambda: list(out.glob("attempt-*/original.json")), "CAS original captured")
                original = self.catalog_path.read_bytes()
                changed = original + b"\n "
                replace(self.catalog_path, changed)
                block.stdin.write(b"ROLLBACK;\n")
                block.stdin.close()
                assert block.wait(timeout=10) == 0
                stdout, stderr = cli.communicate(timeout=40)
                write(self.root / f"{label}.stdout", stdout)
                write(self.root / f"{label}.stderr", stderr)
                write(self.root / f"{label}.exit.json", {"exit": cli.returncode})
                assert cli.returncode == 2 and b"live_catalog_cas" in stderr, stderr
                assert self.catalog_path.read_bytes() == changed
                self.records.append({"label": f"CAS-external-replacement-{label}", "exit": cli.returncode,
                    "external_catalog_preserved": True, "before": sha(original), "external": sha(changed)})
                replace(self.catalog_path, original)  # Test fixture restoration, not tool rollback.
            finally:
                if block.poll() is None:
                    block.stdin.close()
                    block.wait(timeout=15)

    def stop(self):
        if self.gateway is not None:
            if self.gateway.poll() is None:
                self.gateway.terminate()  # Only the child we created, after acceptance.
            self.cleanup["gateway_exit"] = self.gateway.wait(timeout=30)
            self.cleanup["gateway_reaped"] = True
        for f in self.process_files:
            f.close()
        if self.started_pg:
            result = self.command([PG / "pg_ctl", "-D", self.pg, "-m", "fast", "-w", "stop"], "pg-stop", check=False)
            self.cleanup["pg_ctl_stop_exit"] = result.returncode
            self.cleanup["postmaster_pid_file_absent"] = not (self.pg / "postmaster.pid").exists()
        write(self.root / "cleanup.json", self.cleanup)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for key in ["evidence", "onboarding", "cli", "builder", "gateway"]:
        parser.add_argument(f"--{key}", required=True)
    args = parser.parse_args()
    os.umask(0o077)
    args.evidence = str(Path(args.evidence).resolve())
    Path(args.evidence).mkdir(mode=0o700, parents=True, exist_ok=True)
    for key in ["onboarding", "cli", "builder", "gateway"]:
        setattr(args, key, str(Path(getattr(args, key)).resolve()))
    fixture = Fixture(args)
    print(f"OWNED_EVIDENCE={fixture.root}", flush=True)
    try:
        fixture.start()
        acceptance = fixture.exercise()
        print(json.dumps(acceptance, indent=2))
    finally:
        fixture.stop()


if __name__ == "__main__":
    main()
