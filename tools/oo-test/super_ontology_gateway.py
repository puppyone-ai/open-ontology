"""The real OO Gateway for the Super Ontology end to end (D83, WP3b).

A private native PostgreSQL 17 cluster (scram passwords, TLS, loopback only) holds the published cut of one logical table
`sales.customers`: the physical `_oo` tables, the mapping and freshness tables, the publication fence and a NOLOGIN role that
may read only the published columns. A role pack is compiled by the fixed `role-pack-bundle` binary, a client token is issued
by the real `issue-token`, and the real `oo-gateway` serves HTTPS on 127.0.0.1 with a private CA. The profile the Agent token
names points at it, so every database row the e2e sees came through `oo-gateway` (SQL guard, role session, audit WAL).

It follows the owned fixture of crates/oo-gateway/src/f22_owned_pg_tests.rs. Nothing here is a Gateway stand-in; what is not
the full `oo up` stack is only the cluster (native, not a container) and the hand-made publication (no Builder run).
"""

import hashlib
import json
import os
import secrets
import shutil
import socket
import subprocess
import time
from pathlib import Path

PROJECT, SLUG, ROLE, LOGIN = "e2e", "sales", "oo_r_sales", "oo_gw_login"
DIGEST = "c" * 64
AUDIENCE = "https://gateway.loopback.invalid/v1"
PHYSICAL = "pt_" + "c" * 60
COLS = {"id": "pc_" + "1" * 60, "name": "pc_" + "2" * 60, "credit_limit": "pc_" + "3" * 60}


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def run(args, **kw):
    return subprocess.run([str(a) for a in args], check=True, capture_output=True, text=True, timeout=300,
                          env=dict(os.environ, LC_ALL="en_US.UTF-8"), **kw)


class RealGateway:
    def __init__(self, work, bindir, rows):
        """`bindir` holds oo-gateway, oo-sql-parser-worker, issue-token and role-pack-bundle; `rows` are (id, name, credit)."""
        self.work, self.bin, self.rows = Path(work).resolve() / "gw", Path(bindir), rows
        self.dir = self.work / "cfg"  # the operator directory of oo-gateway
        self.pg = self.work / "pgdata"
        self.port = free_port()
        self.password = secrets.token_hex(24)
        self.proc = None
        self.addr = None
        self.token = None

    # ---- the cluster ------------------------------------------------------------------------------------------------
    def _psql(self, db, sql, user="postgres"):
        p = subprocess.run(["psql", "-X", "-qAt", "-v", "ON_ERROR_STOP=1", "-h", "127.0.0.1", "-p", str(self.port), "-U", user, "-d", db],
                           input=sql, text=True, capture_output=True, env=dict(os.environ, PGPASSWORD=self.password))
        if p.returncode:
            raise SystemExit("gateway fixture psql failed: " + p.stderr.strip()[:400])
        return p.stdout

    def psql(self, sql):
        return self._psql("gw_e2e", sql)

    def _cluster(self):
        self.dir.mkdir(parents=True, mode=0o700)
        self.work.chmod(0o700)
        pw = self.work / "pwfile"
        pw.write_text(self.password)
        pw.chmod(0o600)
        run(["initdb", "-D", self.pg, "-U", "postgres", "--auth-local=trust", "--auth-host=scram-sha-256", "--pwfile", pw, "-E", "UTF8", "--no-locale"])
        ca_key, ca = self.work / "ca.key", self.work / "ca.crt"
        run(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", ca_key, "-out", ca, "-days", "2",
             "-subj", "/CN=oo-e2e-gateway-ca", "-addext", "basicConstraints=critical,CA:TRUE"])
        for name in ("pg", "gw"):
            key, csr, crt = self.work / f"{name}.key", self.work / f"{name}.csr", self.work / f"{name}.crt"
            run(["openssl", "req", "-newkey", "rsa:2048", "-nodes", "-keyout", key, "-out", csr, "-subj", "/CN=localhost"])
            ext = self.work / f"{name}.ext"
            ext.write_text("basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\n"
                           "extendedKeyUsage=serverAuth\nsubjectAltName=DNS:localhost,IP:127.0.0.1\n")
            run(["openssl", "x509", "-req", "-in", csr, "-CA", ca, "-CAkey", ca_key, "-CAcreateserial", "-out", crt, "-days", "2", "-extfile", ext])
            key.chmod(0o600)
        with (self.pg / "postgresql.conf").open("a") as f:
            f.write(f"\nlisten_addresses='127.0.0.1'\nport={self.port}\nunix_socket_directories=''\nssl=on\n"
                    f"ssl_cert_file='{self.work / 'pg.crt'}'\nssl_key_file='{self.work / 'pg.key'}'\nmax_connections=30\n")
        run(["pg_ctl", "-D", self.pg, "-l", self.work / "postgres.log", "-w", "start"])

    # ---- the published cut -------------------------------------------------------------------------------------------
    def _compile_pack(self):
        effective = {"protocol": "oo.builder.effective-oos/v1", "project": PROJECT, "ontology_version": "oos-1", "catalog_version": "cat-1",
                     "policy": {"id": "policy", "version": "pv1"}, "knowledge": {"id": "knowledge", "version": "kv1"},
                     "ontology": {"spec_version": "oos/v1", "name": PROJECT, "locale": "en-US", "include_schemas": ["sales"],
                                  "default_tier_preference": ["raw"], "publish": {"include_unreviewed": []},
                                  "classification": {"auto_accept": "none"}, "lifecycle": {"deprecation_min_releases": 1},
                                  "compile": {"per_role": True, "budgets": {"readme_tokens": 2000, "table_file_tokens": 3000},
                                              "sample_values": {"default": "none", "max_per_column": 0}}},
                     "datasets": [{"schema_version": "oos/v1", "dataset": "sales.customers", "label": "Customers", "tier": "raw",
                                   "primary_key": ["id"],
                                   "columns": [{"name": n, "data_type": t, "semantic_type": s, "pii": "none",
                                                "classification": {"state": "confirmed", "by": "fixture/test"}, "provenance": "declared"}
                                               for n, t, s in (("id", "bigint", "identifier"), ("name", "text", "entity_name"), ("credit_limit", "bigint", "quantity"))]}]}
        for k in ("relationships", "enums", "glossary", "entities", "metrics", "examples"):
            effective[k] = []
        visibility = {"protocol": "oo.builder.role-visibility/v1", "project": PROJECT, "ontology_version": "oos-1", "catalog_version": "cat-1",
                      "policy": {"id": "policy", "version": "pv1"}, "knowledge": {"id": "knowledge", "version": "kv1"},
                      "principal": "agent", "role": ROLE, "issued_for_compiler": "oo-builder/role-pack/v1",
                      "datasets": [{"dataset": "sales.customers", "columns": list(COLS), "row_security": "none", "global_stats": "denied"}],
                      "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 128}}
        req = self.work / "pack-request.json"
        req.write_text(json.dumps({"protocol": "oo.builder.role-pack-command/v1", "effective_oos": effective, "visibility": visibility}))
        out = self.work / "pack-bundle.json"
        run([self.bin / "role-pack-bundle", "--request", req, "--output", out])
        bundle = json.loads(out.read_text())
        return bundle

    def _prepare(self):
        self._psql("postgres", f"""CREATE DATABASE gw_e2e;
            CREATE ROLE oo_token_admin LOGIN SUPERUSER PASSWORD '{self.password}';
            CREATE ROLE oo_token_runtime LOGIN NOINHERIT PASSWORD '{self.password}';
            CREATE ROLE {LOGIN} LOGIN NOINHERIT PASSWORD '{self.password}';
            CREATE ROLE {ROLE} NOLOGIN NOINHERIT;
            GRANT {ROLE} TO {LOGIN} WITH INHERIT FALSE, SET TRUE;""")
        bundle = self._compile_pack()
        digest = self._pack_digest(bundle)
        pack_dir = self.dir / "packs"
        pack_dir.mkdir(mode=0o700)
        (pack_dir / f"{ROLE}.json").write_text(self._pack_artifact(bundle))
        cols = ", ".join(f"{p} {t}" for p, t in ((COLS["id"], "int8"), (COLS["name"], "text"), (COLS["credit_limit"], "int8"), ("secret", "text")))
        values = ", ".join("({}, {}, {}, 'hidden')".format(i, "'" + n.replace("'", "''") + "'", c) for i, n, c in self.rows)
        maps = ",\n".join(f"('{PROJECT}','{DIGEST}',1,'c','sales','customers','{n}','{p}')" for n, p in COLS.items())
        self.psql(f"""CREATE SCHEMA _oo; CREATE SCHEMA oo_publish_target;
            CREATE TABLE _oo.{PHYSICAL}({cols});
            INSERT INTO _oo.{PHYSICAL} VALUES {values};
            ALTER TABLE _oo.{PHYSICAL} ENABLE ROW LEVEL SECURITY;
            ALTER TABLE _oo.{PHYSICAL} FORCE ROW LEVEL SECURITY;
            CREATE POLICY e2e_read ON _oo.{PHYSICAL} FOR SELECT TO {ROLE} USING (true);
            CREATE TABLE _oo.physical_mappings(source text,config_digest text,run_generation int8,
                level text,logical_namespace text,logical_table text,logical_column text,physical_name text);
            INSERT INTO _oo.physical_mappings VALUES
                ('{PROJECT}','{DIGEST}',1,'t','sales','customers','','{PHYSICAL}'),
                {maps};
            CREATE FUNCTION _oo.lookup_physical_mapping(text,text,int8,text,text,text,text) RETURNS text
            LANGUAGE sql STABLE AS $fn$ SELECT physical_name FROM _oo.physical_mappings
                WHERE source=$1 AND config_digest=$2 AND run_generation=$3
                AND level=left($4,1) AND logical_namespace=$5 AND logical_table=$6 AND logical_column=$7 $fn$;
            CREATE TABLE _oo.table_freshness(source text,schema_name text,table_name text,physical_name text,
                source_generation int8,target_generation int8,state text,source_committed_at timestamptz,applied_at timestamptz);
            INSERT INTO _oo.table_freshness VALUES('{PROJECT}','sales','customers','{PHYSICAL}',1,1,'ready',NULL,now());
            CREATE TABLE oo_publish_target.fence(role_name text,schema_name text,relation_name text,
                state text,opened_pack_digest text,opened_version text);
            INSERT INTO oo_publish_target.fence VALUES('{ROLE}','_oo','{PHYSICAL}','open','{digest}','release1');
            GRANT USAGE ON SCHEMA _oo TO {ROLE},{LOGIN};
            GRANT USAGE ON SCHEMA oo_publish_target TO {LOGIN};
            GRANT SELECT ({",".join(COLS.values())}) ON _oo.{PHYSICAL} TO {ROLE};
            GRANT SELECT ON _oo.physical_mappings,_oo.table_freshness,oo_publish_target.fence TO {LOGIN};
            ALTER ROLE {LOGIN} SET default_transaction_read_only=on;
            REVOKE ALL ON FUNCTION pg_catalog.pg_advisory_lock(bigint),pg_catalog.pg_advisory_xact_lock(bigint) FROM PUBLIC;""")
        return digest

    @staticmethod
    def _pack_artifact(bundle):
        # the bundle holds the pack as a JSON object; the compact canonical text is what `pack_sha256` names
        text = json.dumps(bundle["pack"], separators=(",", ":"), ensure_ascii=False, sort_keys=True)
        assert hashlib.sha256(text.encode()).hexdigest() == bundle["pack_sha256"], "pack text is not the canonical one"
        return text

    @staticmethod
    def _pack_digest(bundle):
        return bundle["pack_digest"]

    def _configure(self):
        d = self.dir
        (d / "endpoint.json").write_text(json.dumps({"host": "127.0.0.1", "port": self.port, "database": "gw_e2e"}))
        for name in ("password", "admin-password"):  # the second turns on the token lifecycle, so a client certificate is optional
            (d / name).write_text(self.password)
            (d / name).chmod(0o600)
        shutil.copy(self.work / "ca.crt", d / "ca.crt")
        shutil.copy(self.work / "gw.crt", d / "gw.crt")
        shutil.copy(self.work / "gw.key", d / "gw.key")
        (d / "gw.key").chmod(0o600)
        reviewed = d / "reviewed/reader"
        reviewed.mkdir(parents=True)
        (reviewed / "policy.json").write_text(json.dumps({
            "protocol": "oo.operator-preview.policy/v1", "project": PROJECT, "role": SLUG, "login": LOGIN, "config_digest": DIGEST, "generation": 1,
            "table": {"namespace": "sales", "name": "customers", "columns": [
                {"name": n, "semantic_type": s, "status": "confirmed", "pii": {"class": "none"}}
                for n, s in (("id", "identifier"), ("name", "entity_name"), ("credit_limit", "quantity"))]}}))
        (reviewed / "visibility.json").write_text(json.dumps({
            "protocol": "oo.builder.role-visibility/v1", "project": PROJECT, "role": ROLE, "knowledge": {"version": "kv1"}, "policy": {"version": "pv1"},
            "datasets": [{"dataset": "sales.customers", "columns": list(COLS)}]}))
        (d / "wal").mkdir()
        (d / "config.json").write_text(json.dumps({
            "listen": "127.0.0.1:0", "audience": AUDIENCE, "runtime_user": "oo_token_runtime", "password_file": "password", "ca_file": "ca.crt",
            "endpoint_file": "endpoint.json", "pack_dir": "packs", "body_limit": 1048576, "request_timeout_ms": 10000, "global_concurrency": 2,
            "deployment_mode": "all_in_one", "data_endpoint_file": "endpoint.json", "data_password_file": "password", "data_ca_file": "ca.crt",
            "data_user": LOGIN, "wal_dir": "wal", "reviewed_dir": "reviewed",
            "transport": "tls", "tls_key_file": "gw.key", "tls_chain_file": "gw.crt", "client_ca_file": "ca.crt", "handshake_timeout_ms": 5000,
            "token_admin_password_file": "admin-password", "token_admin_workload": "urn:oo:workload:e2e/admin"}))

    def _issue_token(self, out):
        base = [self.dir / "endpoint.json", self.dir / "password", self.dir / "ca.crt"]
        run([self.bin / "issue-token", "install", *base])
        grant = self.work / "grant.json"
        grant.write_text(json.dumps({"kind": "client", "project_id": PROJECT, "principal_id": "agent", "role": ROLE, "policy_version": "pv1",
                                     "knowledge_version": "kv1", "audience_url": AUDIENCE}))
        p = run([self.bin / "issue-token", "issue", *base, grant, out])
        Path(out).chmod(0o600)
        return p.stdout.strip()

    # ---- life cycle ---------------------------------------------------------------------------------------------------
    def start(self, token_file):
        """Cluster, cut, token and gateway. Returns the https URL; the token is written to `token_file` (0600)."""
        self.work.mkdir(parents=True)
        self._cluster()
        self._prepare()
        self._configure()
        self._issue_token(token_file)
        self.token_file = Path(token_file)
        err = open(self.work / "gateway.stderr", "w")
        self.proc = subprocess.Popen([str(self.bin / "oo-gateway"), str(self.dir)], stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=err,
                                     start_new_session=True)
        for _ in range(150):
            if self.proc.poll() is not None:
                raise SystemExit("oo-gateway exited: " + (self.work / "gateway.stderr").read_text()[:400])
            if (self.dir / "listen.addr").is_file():
                break
            time.sleep(0.1)
        self.addr = (self.dir / "listen.addr").read_text().strip()
        return f"https://localhost:{self.addr.rsplit(':', 1)[1]}"

    @property
    def ca(self):
        return self.work / "ca.crt"

    def audit_records(self):
        """Lines of the Gateway's audit WAL: one per admitted query."""
        n = 0
        for f in sorted((self.dir / "wal").glob("**/*")):
            if f.is_file():
                n += sum(1 for line in f.read_text(errors="replace").splitlines() if line.strip())
        return n

    def stop(self):
        if self.proc and self.proc.poll() is None:
            self.proc.terminate()
            try:
                self.proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self.proc.kill()
        if (self.pg / "postmaster.pid").exists():
            subprocess.run(["pg_ctl", "-D", str(self.pg), "-m", "fast", "-w", "stop"], capture_output=True)
