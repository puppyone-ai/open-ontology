#!/usr/bin/env python3
"""Prove SQL catalog capabilities in a new, TLS-only local PostgreSQL fixture."""

import argparse
import getpass
import json
import os
from pathlib import Path
import secrets
import shutil
import socket
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--evidence-dir", type=Path)
    args = parser.parse_args()
    product = Path(__file__).resolve().parents[3]
    pg_bin = Path(shutil.which("pg_ctl") or "").resolve().parent
    for name in ("initdb", "pg_ctl", "psql", "postgres"):
        if not (pg_bin / name).is_file():
            raise SystemExit(f"missing installed PostgreSQL binary: {name}")
    if args.evidence_dir:
        root = args.evidence_dir.resolve()
        root.mkdir(mode=0o700)
    else:
        root = Path(tempfile.mkdtemp(prefix="oo-sql-capabilities-", dir="/tmp")).resolve()
    root.chmod(0o700)
    data, sockets = root / "data", root / "socket"
    sockets.mkdir(mode=0o700)
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        port = probe.getsockname()[1]
    assert port > 1024 and port != 5432
    nonce = secrets.token_hex(16)
    database = "oo_sql_capabilities_" + nonce[:12]
    reader = "oo_sql_capabilities_reader"
    owner = getpass.getuser()
    assert owner.replace("_", "").isalnum()
    password = secrets.token_hex(32)
    env = {key: value for key, value in os.environ.items() if not key.startswith("PG")}
    env["CARGO_BUILD_JOBS"] = "1"
    lifecycle = {"fixture_root": str(root), "port": port, "database": database,
                 "existing_instances_connected": False, "customer_services_touched": False}

    def run(args, text=None):
        with (root / "runner.log").open("a") as log:
            subprocess.run([str(arg) for arg in args], input=text, text=True, cwd=product,
                           env=env, stdout=log, stderr=subprocess.STDOUT, check=True,
                           timeout=900)

    def private(name, text):
        path = root / name
        path.write_text(text)
        path.chmod(0o600)
        return path

    print(f"Owned fixture: {root}", flush=True)
    run([pg_bin / "initdb", "-D", data, "--auth-local=peer", "--auth-host=scram-sha-256",
         "--encoding=UTF8", "--no-locale", "-U", owner])
    run(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1",
         "-subj", "/CN=Owned SQL Capabilities CA", "-addext", "basicConstraints=critical,CA:TRUE",
         "-addext", "keyUsage=critical,keyCertSign,cRLSign",
         "-keyout", root / "ca.key", "-out", root / "ca.crt"])
    run(["openssl", "req", "-new", "-newkey", "rsa:2048", "-nodes", "-subj", "/CN=localhost",
         "-keyout", root / "server.key", "-out", root / "server.csr"])
    (root / "server.key").chmod(0o600)
    extensions = private("server.ext", "basicConstraints=critical,CA:FALSE\n"
                         "keyUsage=critical,digitalSignature,keyEncipherment\n"
                         "extendedKeyUsage=serverAuth\nsubjectAltName=DNS:localhost,IP:127.0.0.1\n")
    run(["openssl", "x509", "-req", "-days", "1", "-in", root / "server.csr",
         "-CA", root / "ca.crt", "-CAkey", root / "ca.key", "-CAcreateserial",
         "-extfile", extensions, "-out", root / "server.crt"])
    with (data / "postgresql.conf").open("a") as out:
        out.write(f"\nlisten_addresses='127.0.0.1'\nport={port}\n"
                  f"unix_socket_directories='{sockets}'\nssl=on\n"
                  f"ssl_cert_file='{root / 'server.crt'}'\nssl_key_file='{root / 'server.key'}'\n"
                  f"oo.sql_capabilities_nonce='{nonce}'\nmax_connections=10\nshared_buffers='32MB'\n")
    (data / "pg_hba.conf").write_text(
        f"local all {owner} peer\n"
        f"hostssl {database} {reader} 127.0.0.1/32 scram-sha-256\n"
        "host all all 0.0.0.0/0 reject\nhost all all ::/0 reject\n"
    )
    psql = [pg_bin / "psql", "-X", "-v", "ON_ERROR_STOP=1", "-h", sockets,
            "-p", port, "-U", owner]
    started = False
    try:
        run([pg_bin / "pg_ctl", "-D", data, "-l", root / "postgres.log", "-w", "start"])
        started = True
        run(psql + ["-d", "postgres"], f'CREATE DATABASE "{database}";')
        run(psql + ["-d", database], f"""
REVOKE ALL ON DATABASE "{database}" FROM PUBLIC;
CREATE ROLE {reader} LOGIN PASSWORD '{password}' NOSUPERUSER NOCREATEDB NOCREATEROLE
    NOINHERIT NOREPLICATION NOBYPASSRLS;
ALTER ROLE {reader} SET default_transaction_read_only=on;
ALTER ROLE {reader} SET statement_timeout='5s';
ALTER ROLE {reader} SET timezone='UTC';
GRANT CONNECT ON DATABASE "{database}" TO {reader};
CREATE SCHEMA capability_fixture;
CREATE SCHEMA logistics;
CREATE TABLE capability_fixture.shipments (
    id int4 PRIMARY KEY, small int2, big int8, amount numeric(20,6), fraction float8,
    txt text, code varchar(20), fixed char(6), day date, clock time,
    stamp timestamp, zoned timestamptz, span interval,
    flag bool, doc json, binary_doc jsonb, token uuid, payload bytea, secret text
);
INSERT INTO capability_fixture.shipments VALUES
    (1,-2,-200,-2.500001,-2.5,'a','a','a','2026-10-28','01:02:03',
        '2026-10-28 00:00:00','2026-10-28 08:00:00+08','-2 days',
        true,'{{}}','{{}}','00000000-0000-0000-0000-000000000001','abc','hidden'),
    (2,4,400,4.500002,4.5,'z','z','z','2026-10-29','23:59:59.999999',
        '2026-10-28 23:59:59.999999','2026-10-28 23:59:59.999999+00','4 days',
        false,'{{}}','{{}}','00000000-0000-0000-0000-000000000002','def','hidden'),
    (3,4,400,4.500002,4.5,'','a ','a ','2026-10-29','12:00:00',
        '2026-10-29 00:00:00','2026-10-29 00:00:00+00','1 month',
        true,'{{}}','{{}}','00000000-0000-0000-0000-000000000003','ghi','hidden'),
    (4,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,
        NULL,NULL,NULL,NULL,NULL,'hidden');
CREATE VIEW logistics.shipment AS SELECT * FROM capability_fixture.shipments;
CREATE TABLE capability_fixture.patterns(id int4 PRIMARY KEY,label text,code varchar(40),secret text);
INSERT INTO capability_fixture.patterns VALUES
    (1,'Alpha','Alpha','hidden'),(2,'alpha','alpha','hidden'),
    (3,'A_B','A_B','hidden'),(4,'A%B','A%B','hidden'),
    (5,'back\\slash','back\\slash','hidden'),(6,'\u4e2d\u6587a','\u4e2d\u6587a','hidden'),
    (7,'','', 'hidden'),(8,NULL,NULL,'hidden');
CREATE VIEW logistics.pattern_lookup AS SELECT * FROM capability_fixture.patterns;
CREATE VIEW logistics.daily_orders AS
    SELECT flag AS order_status,stamp AS upd_time FROM capability_fixture.shipments;
CREATE TABLE capability_fixture.balances(balance_time timestamp,total_amount numeric(20,6),
    paid_amount numeric(20,6),today_remain_amount numeric(20,6));
INSERT INTO capability_fixture.balances VALUES
    ('2026-10-28',0,0,0),('2026-10-29',10,-2,3),('2026-10-29',-4,NULL,0),
    ('2026-10-30',NULL,NULL,NULL);
CREATE VIEW logistics.balances AS SELECT * FROM capability_fixture.balances;
CREATE FUNCTION logistics.min(timestamp) RETURNS timestamp
    LANGUAGE sql IMMUTABLE AS 'SELECT $1';
CREATE FUNCTION logistics.date_trunc(text,timestamp) RETURNS timestamp
    LANGUAGE sql IMMUTABLE AS 'SELECT $2';
CREATE FUNCTION logistics.date(timestamp) RETURNS date
    LANGUAGE sql IMMUTABLE AS 'SELECT $1::date';
CREATE DOMAIN logistics.date AS date;
CREATE FUNCTION logistics.text_ne(text,text) RETURNS bool
    LANGUAGE sql IMMUTABLE AS 'SELECT true';
CREATE OPERATOR logistics.<> (LEFTARG=text, RIGHTARG=text, FUNCTION=logistics.text_ne);
GRANT USAGE ON SCHEMA capability_fixture,logistics TO {reader};
GRANT SELECT(id,small,big,amount,fraction,txt,code,fixed,day,clock,stamp,zoned,span,
    flag,doc,binary_doc,token,payload) ON capability_fixture.shipments,logistics.shipment TO {reader};
GRANT SELECT(id,label,code) ON capability_fixture.patterns,logistics.pattern_lookup TO {reader};
GRANT SELECT(order_status,upd_time) ON logistics.daily_orders TO {reader};
GRANT SELECT ON capability_fixture.balances,logistics.balances TO {reader};
CREATE VIEW capability_fixture.role_shipments AS SELECT id,flag FROM capability_fixture.shipments WHERE flag IS TRUE;
CREATE VIEW logistics.filtered AS SELECT id,flag FROM capability_fixture.role_shipments;
GRANT SELECT ON capability_fixture.role_shipments,logistics.filtered TO {reader};
""")
        config = {
            "port": port, "database": database, "user": reader, "nonce": nonce,
            "ca_file": str(root / "ca.crt"), "password_file": str(private("reader.pass", password)),
            "report_path": str(root / "actual-db-report.json"),
        }
        env["OO_SQL_CAPABILITIES_PG_CONFIG"] = str(private("fixture.json", json.dumps(config)))
        command = ["cargo", "test", "--offline", "--locked", "-p", "oo-gateway", "--lib",
                   "policy::catalog::capabilities_pg_tests::owned_sql_capabilities",
                   "--", "--exact", "--ignored", "--nocapture", "--test-threads=1"]
        lifecycle["command"] = command
        run(command)
        report = json.loads((root / "actual-db-report.json").read_text())
        assert report["actualDB"]["database"] == database
        negative_command = command.copy()
        negative_command[7] = "policy::catalog::capabilities_pg_tests::owned_day_catalog_refuses_modified_builtin"
        mutations = [
            ("security_definer",
             "ALTER FUNCTION pg_catalog.date(timestamp) SECURITY DEFINER;",
             "ALTER FUNCTION pg_catalog.date(timestamp) SECURITY INVOKER;"),
            ("stable_not_immutable",
             "ALTER FUNCTION pg_catalog.date_trunc(text,timestamp) STABLE;",
             "ALTER FUNCTION pg_catalog.date_trunc(text,timestamp) IMMUTABLE;"),
            ("custom_function_settings",
             "ALTER FUNCTION pg_catalog.date_trunc(text,timestamp) SET search_path TO logistics;",
             "ALTER FUNCTION pg_catalog.date_trunc(text,timestamp) RESET ALL;"),
            ("to_char_volatile",
             "ALTER FUNCTION pg_catalog.to_char(timestamp,text) VOLATILE;",
             "ALTER FUNCTION pg_catalog.to_char(timestamp,text) STABLE;"),
        ]
        report["mutated_builtin_refusals"] = []
        for label, mutate, restore in mutations:
            run(psql + ["-d", database], mutate)
            try:
                run(negative_command)
                report["mutated_builtin_refusals"].append(label)
            finally:
                run(psql + ["-d", database], restore)
        env["OO_SQL_CAST_MUTATION"] = "1"
        for label, mutate, restore in [
            ("cast_security_definer",
             "ALTER FUNCTION pg_catalog.int8(numeric) SECURITY DEFINER;",
             "ALTER FUNCTION pg_catalog.int8(numeric) SECURITY INVOKER;"),
            ("cast_input_volatile",
             "ALTER FUNCTION pg_catalog.numeric_in(cstring,oid,integer) VOLATILE;",
             "ALTER FUNCTION pg_catalog.numeric_in(cstring,oid,integer) IMMUTABLE;"),
            ("cast_io_override",
             "CREATE FUNCTION capability_fixture.bad_cast(text) RETURNS integer LANGUAGE sql IMMUTABLE AS 'SELECT 1'; CREATE CAST (text AS integer) WITH FUNCTION capability_fixture.bad_cast(text);",
             "DROP CAST (text AS integer); DROP FUNCTION capability_fixture.bad_cast(text);"),
        ]:
            run(psql + ["-d", database], mutate)
            try:
                run(negative_command)
                report["mutated_builtin_refusals"].append(label)
            finally:
                run(psql + ["-d", database], restore)
        del env["OO_SQL_CAST_MUTATION"]
        (root / "actual-db-report.json").write_text(json.dumps(report, indent=2))
        lifecycle["test_passed"] = True
        print(json.dumps({"actualDB": report["actualDB"],
                          "function_count": report["function_count"],
                          "query_count": len(report["queries"]),
                          "refusal_count": len(report["refusals"]),
                          "mutated_builtin_refusals": report["mutated_builtin_refusals"],
                          "report": str(root / "actual-db-report.json")}, indent=2), flush=True)
    finally:
        if started or (data / "postmaster.pid").exists():
            pidfile = data / "postmaster.pid"
            assert pidfile.read_text().splitlines()[1] == str(data), "owned PG path changed"
            run([pg_bin / "pg_ctl", "-D", data, "-m", "fast", "-w", "stop"])
            lifecycle["owned_cluster_stopped"] = not pidfile.exists()
        (root / "lifecycle.json").write_text(json.dumps(lifecycle, indent=2))


if __name__ == "__main__":
    main()
