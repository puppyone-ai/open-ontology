#!/usr/bin/env python3
"""Owned lane-A TLS matrix; lane-H synthetic seed and replay-only model."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import secrets
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(ROOT / "tools/model-egress/tests"))
from test_egress import Replay

spec = importlib.util.spec_from_file_location(
    "seed", ROOT / "testing/engines/seed-common/generate.py")
seed = importlib.util.module_from_spec(spec)
spec.loader.exec_module(seed)
PINS = {13: "13.23", 14: "14.24", 15: "15.19", 16: "16.15", 17: "17.11", 18: "18.6"}
SECRET_VALUES = []


def command(argv, output, data=None, env=None, expected=0):
    result = subprocess.run(argv, input=data, capture_output=True, env=env, timeout=180)
    recorded = result.stdout + result.stderr
    for value in SECRET_VALUES:
        recorded = recorded.replace(value.encode(), b"[REDACTED]")
    output.write_bytes(recorded)
    if result.returncode != expected:
        raise RuntimeError(f"{argv[0]} exit {result.returncode}; see {output.name}")
    return result.stdout


def private_file(path, value):
    with path.open("x") as stream:
        stream.write(value)
    path.chmod(0o600)


def matrix(major, base):
    out = base / f"pg{major}"
    out.mkdir()
    private = out / "private"
    private.mkdir(mode=0o700)
    home = private / "home"
    store = home / ".config/open-ontology/secrets"
    store.mkdir(parents=True, mode=0o700)
    admin, reader, key = (secrets.token_hex(24) for _ in range(3))
    SECRET_VALUES.extend((admin, reader, key))
    private_file(private / "admin", admin)
    private_file(store / "reader", reader)
    private_file(store / "model", key)
    command(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes",
             "-keyout", str(private / "ca.key"), "-out", str(private / "ca.crt"),
             "-days", "2", "-subj", "/CN=OO fixture CA"], out / "ca.log")
    command(["openssl", "req", "-newkey", "rsa:2048", "-nodes",
             "-keyout", str(private / "server.key"), "-out", str(private / "server.csr"),
             "-subj", "/CN=localhost"], out / "csr.log")
    (private / "ext").write_text(
        "basicConstraints=critical,CA:FALSE\nsubjectAltName=DNS:localhost,IP:127.0.0.1\n"
        "keyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n")
    command(["openssl", "x509", "-req", "-in", str(private / "server.csr"),
             "-CA", str(private / "ca.crt"), "-CAkey", str(private / "ca.key"),
             "-CAcreateserial", "-out", str(private / "server.crt"), "-days", "2",
             "-extfile", str(private / "ext")], out / "sign.log")
    for path in private.glob("*"):
        if path.is_file():
            path.chmod(0o600)
    private_file(store / "ca", (private / "ca.crt").read_text())
    image = json.loads(command(
        ["docker", "image", "inspect", f"postgres:{PINS[major]}-bookworm"],
        out / "image.log"))[0]
    name = f"oo-a-doctor-{major}-{secrets.token_hex(4)}"
    replay = Replay()
    created = False
    cases = []
    try:
        command(["docker", "create", "--name", name, "--label", "oo.lane=a",
                 "--label", "oo.run_id=" + base.name, "--memory", "1g", "--cpus", "2",
                 "-p", "127.0.0.1::5432", "--tmpfs",
                 "/var/lib/postgresql:rw" if major >= 18 else "/var/lib/postgresql/data:rw",
                 "-v", f"{private}:/run/fixture:ro",
                 "-e", "POSTGRES_PASSWORD_FILE=/tmp/admin", "--entrypoint", "bash",
                 image["Id"], "-c",
                 "cp /run/fixture/server.* /tmp/; cp /run/fixture/admin /tmp/admin; "
                 "chown postgres:postgres /tmp/server.* /tmp/admin; "
                 "chmod 600 /tmp/server.key /tmp/admin; "
                 "exec docker-entrypoint.sh postgres -c ssl=on "
                 "-c ssl_cert_file=/tmp/server.crt -c ssl_key_file=/tmp/server.key"],
                out / "create.log")
        created = True
        command(["docker", "start", name], out / "start.log")
        for _ in range(90):
            ready = subprocess.run(["docker", "exec", name, "pg_isready", "-U", "postgres"],
                                   capture_output=True, timeout=10)
            if ready.returncode == 0:
                break
            time.sleep(1)
        else:
            raise RuntimeError("fixture not ready")
        sql = ["docker", "exec", "-i", name, "psql", "-X", "-U", "postgres",
               "-v", "ON_ERROR_STOP=1", "-At"]
        setup = "".join(seed.generate("postgres", PINS[major], 1000, 3))
        setup += (
            f"\nCREATE ROLE fixture_reader LOGIN PASSWORD '{reader}';\n"
            "REVOKE CREATE ON SCHEMA public FROM PUBLIC;\n"
            "REVOKE TEMPORARY ON DATABASE postgres FROM PUBLIC;\n"
            "ALTER ROLE fixture_reader SET default_transaction_read_only=on;\n"
            "GRANT USAGE ON SCHEMA public TO fixture_reader;\n"
            "GRANT SELECT ON ALL TABLES IN SCHEMA public TO fixture_reader;\n"
            "GRANT pg_read_all_stats TO fixture_reader;\nANALYZE;\n")
        command(sql, out / "seed.log", setup.encode())
        port = int(command(["docker", "port", name, "5432/tcp"],
                           out / "port.log").decode().strip().rsplit(":", 1)[1])
        config = {
            "version": "oo.stack/v1",
            "sources": [{"id": "fixture", "engine": "postgres", "host": "localhost",
                         "port": port, "database": "postgres", "user": "fixture_reader",
                         "password": {"secret_ref": "reader"}, "tls_ca": {"secret_ref": "ca"},
                         "scope": {"include": ["public.events", "public.order_lines",
                                               "public.string_keys"]}}],
            "model": {"provider": "openai-compatible",
                      "base_url": f"http://127.0.0.1:{replay.httpd.server_port}/v1",
                      "model": "fixture-model", "key": {"secret_ref": "model"}}}
        config_path = out / "stack.json"
        env = dict(os.environ, HOME=str(home))
        snapshot_sql = (
            "SELECT count(*),sum(id) FROM events; SELECT count(*) FROM order_lines;"
            "SHOW wal_level; SELECT rolreplication,rolsuper FROM pg_roles "
            "WHERE rolname='fixture_reader';")
        before = command(sql, out / "before.log", snapshot_sql.encode())

        def doctor(case, expected):
            config_path.write_text(json.dumps(config))
            raw = command([str(ROOT / "target/debug/oo"), "doctor", "--config",
                           str(config_path), "--json"], out / f"{case}.json",
                          env=env, expected=expected)
            for secret in (admin, reader, key):
                assert secret.encode() not in raw
            report = json.loads(raw)
            assert report["read_only"] is True
            assert report["model"]["status"] == "ok"
            assert report["model"]["report"]["context_window"] == 272000
            assert report["model"]["report"]["max_output_tokens"] == 128000
            cases.append({"case": case, "exit": expected})
            return report

        # The adapter deliberately rejects all role-level startup overrides,
        # even read-only=on, before installing its own read-only session guards.
        guarded = doctor("role-settings-blocked", 1)
        assert "session_identity" in guarded["sources"][0]["error"]
        command(sql, out / "reset-startup.log",
                b"ALTER ROLE fixture_reader RESET default_transaction_read_only;")
        good = doctor("healthy", 0)
        assert good["sources"][0]["probe"]["version"]["major"] == major
        assert len(good["sources"][0]["chunk_plans"]) == 3
        assert good["sources"][0]["chunk_plans"][0]["estimated_rows"] == 1000
        assert good["sources"][0]["chunk_plans"][0]["estimated_bytes"] is None
        assert good["sources"][0]["dba_requirements"]  # C1 setup advice, not executed.
        config["sources"][0]["sync"] = {"mode": "C1"}
        assert doctor("c1-needs-setup", 1)["sources"][0]["status"] == "blocked"
        config["sources"][0].pop("sync")
        selected = config["sources"][0]["scope"]["include"]
        config["sources"][0]["scope"]["include"] = ["public.no_key"]
        keyless = doctor("keyless", 0 if major == 13 else 1)
        if major >= 14:
            assert "ctid_requires_exported_snapshot" in (
                keyless["sources"][0]["chunk_plans"][0]["error"])
        else:
            assert keyless["sources"][0]["chunk_plans"][0]["strategy"]["strategy"] == "single_read"
        config["sources"][0]["scope"]["include"] = selected
        command(sql, out / "revoke.log",
                b"REVOKE SELECT ON public.events FROM fixture_reader;")
        denied = doctor("missing-grant", 1)
        assert any("GRANT SELECT" in (r.get("dba_statement") or "")
                   for r in denied["sources"][0]["dba_requirements"])
        command(sql, out / "restore.log",
                b"GRANT SELECT ON public.events TO fixture_reader;")
        config["sources"].append(dict(config["sources"][0], id="pending", engine="mysql"))
        mixed = doctor("mixed-sources", 1)
        assert mixed["sources"][0]["status"] == "ok"
        assert mixed["sources"][1]["error"] == "engine not available in this build"
        config["sources"].pop()
        config["sources"][0]["scope"]["include"] = ["*"]
        assert "scope_error" in doctor("wildcard-blocked", 1)["sources"][0]
        for family in ["mysql", "oracle"]:
            config["sources"][0]["engine"] = family
            assert doctor(family + "-pending", 1)["sources"][0]["error"] == (
                "engine not available in this build")
        after = command(sql, out / "after.log", snapshot_sql.encode())
        assert before == after
        for path in out.iterdir():
            if path.is_file():
                assert all(s.encode() not in path.read_bytes() for s in (admin, reader, key))
        (out / "result.json").write_text(json.dumps({
            "engine": "postgres", "version": PINS[major], "cases": cases,
            "fixture": "lane-H generator, scale=1000 tables=3 (reduced smoke)",
            "model": "synthetic replay; NOT configured live model acceptance",
            "source_guard": "non-superuser, SELECT-only grants; selected data/settings unchanged; adapter installs session read-only guard",
            "secret_scan": "pass", "image_id": image["Id"],
            "architecture": image["Architecture"]}, indent=2))
    finally:
        replay.close()
        if created:
            labels = json.loads(command(
                ["docker", "inspect", "--format", "{{json .Config.Labels}}", name],
                out / "labels.log"))
            assert labels["oo.lane"] == "a" and labels["oo.run_id"] == base.name
            command(["docker", "rm", "-f", "-v", name], out / "cleanup.log")
        hashes = {p.name: hashlib.sha256(p.read_bytes()).hexdigest()
                  for p in out.iterdir() if p.is_file()}
        (out / "SHA256SUMS.json").write_text(json.dumps(hashes, indent=2))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--versions", default="13,14,15,16,17,18")
    args = parser.parse_args()
    if not args.run_id or any(c not in "abcdefghijklmnopqrstuvwxyz0123456789-"
                              for c in args.run_id):
        parser.error("invalid run id")
    base = ROOT / "evidence" / args.run_id
    base.mkdir(exist_ok=True)
    for major in map(int, args.versions.split(",")):
        matrix(major, base)
