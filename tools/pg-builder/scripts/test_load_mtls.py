#!/usr/bin/env python3
"""Owned PostgreSQL 17: oo-test -> oo-load -> mTLS Builder cards, no model key."""
import argparse
import copy
import json
import os
from pathlib import Path
import sys

from live_selfbuild_acceptance import LaneRun, ROOT

sys.path.insert(0, str(ROOT / "tools/oo-test"))
from t1 import configure_tls


class Fixture(LaneRun):
    def command(self, argv, **kwargs):
        if argv[:2] == ["docker", "create"]:
            argv = argv[:2] + ["-p", "127.0.0.1::5432", "--entrypoint", "bash"] + argv[2:]
            argv += ["-c", "cp /run/oo/server-* /tmp/; cp /run/oo/source-ca /tmp/; "
                     "chown postgres:postgres /tmp/server-*; chmod 600 /tmp/server-key; "
                     "exec docker-entrypoint.sh postgres -c ssl=on "
                     "-c ssl_cert_file=/tmp/server-cert -c ssl_key_file=/tmp/server-key "
                     "-c ssl_ca_file=/tmp/source-ca"]
        return super().command(argv, **kwargs)


def save(path, value):
    path.write_text(json.dumps(value))
    path.chmod(0o600)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run-id", required=True)
    args = parser.parse_args()
    if not args.run_id or not all(c.isalnum() or c in "-_" for c in args.run_id):
        parser.error("invalid run ID")
    os.umask(0o077)
    out = ROOT / "evidence" / args.run_id
    out.mkdir(mode=0o700)
    f = Fixture(out, "postgres", "17",
                argparse.Namespace(platform=None, scale=1000, tables=3, health_timeout=180))
    p = f.private
    summary = {"status": "failed", "live_model_call": "not_run"}
    try:
        configure_tls(f, p)
        (p / "extensions").write_text(
            "basicConstraints=critical,CA:FALSE\nsubjectAltName=DNS:localhost\n"
            "keyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n")
        f.command(["openssl", "x509", "-req", "-in", str(p / "server-csr"),
                   "-CA", str(p / "source-ca"), "-CAkey", str(p / "ca-key"),
                   "-CAcreateserial", "-out", str(p / "server-cert"), "-days", "2",
                   "-extfile", str(p / "extensions")])
        f.command(["openssl", "req", "-newkey", "rsa:2048", "-nodes",
                   "-keyout", str(p / "client-key"), "-out", str(p / "client-csr"),
                   "-subj", "/CN=fixture_reader"])
        (p / "client-ext").write_text("basicConstraints=critical,CA:FALSE\nextendedKeyUsage=clientAuth\n")
        f.command(["openssl", "x509", "-req", "-in", str(p / "client-csr"),
                   "-CA", str(p / "source-ca"), "-CAkey", str(p / "ca-key"),
                   "-CAcreateserial", "-out", str(p / "client-cert"), "-days", "2",
                   "-extfile", str(p / "client-ext")])
        f.setup()
        f.seed()
        _, version = f.sql("SHOW server_version;", admin=True)
        f.sql("ALTER ROLE fixture_reader RESET default_transaction_read_only; "
              f"ALTER ROLE fixture_reader PASSWORD '{f.reader_secret}'; "
              "GRANT pg_read_all_stats TO fixture_reader;", admin=True)
        for path in p.iterdir():
            path.chmod(0o600)
        p.chmod(0o700)
        _, address = f.command(["docker", "port", f.name, "5432/tcp"])
        port = int(address.strip().rsplit(":", 1)[1])
        stack = {
            "version": "oo.stack/v1",
            "sources": [{"id": "fixture", "engine": "postgres", "host": "localhost",
                         "port": port, "database": "fixture", "user": "fixture_reader",
                         "password": {"secret_ref": "reader-password"},
                         "tls_ca": {"secret_ref": "source-ca"},
                         "scope": {"include": ["public.regions"]}}],
            "model": {"provider": "openai-compatible", "base_url": "https://example.invalid/v1",
                      "model": "gpt-6.1-sol", "key": {"secret_ref": "unused"}},
        }
        save(p / "stack.json", stack)
        target = {"host": "localhost", "port": port, "database": "fixture", "user": "postgres",
                  "password": {"secret_ref": "password"}, "tls_ca": {"secret_ref": "source-ca"},
                  "run_id": "builder-fixture"}
        save(p / "target.json", target)
        f.command(["cargo", "build", "--manifest-path", "tools/oo-load/Cargo.toml", "--locked"],
                  timeout=1200)
        load_command = [
            str(ROOT / "target/debug/oo-load"), "--stack", str(p / "stack.json"),
            "--source", "fixture", "--target", str(p / "target.json"), "--secrets-dir", str(p)]
        f.command(load_command)
        target["run_id"] = "builder-fixture-2"
        save(p / "target.json", target)
        _, loaded = f.command(load_command)
        loaded = json.loads(loaded)
        _, generations = f.sql("SELECT count(*) FROM _oo_load.publications;", admin=True)
        assert int(generations.strip()) == 2
        # Use catalog discovery, not assumptions about the loader's hashed view names.
        _, view = f.sql("SELECT relname FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace "
                       "WHERE n.nspname='oo_load_data' AND c.relkind='v';", admin=True)
        view = view.strip()
        assert view and "\n" not in view
        f.sql(f'GRANT USAGE ON SCHEMA oo_load_data TO fixture_reader; '
              f'GRANT USAGE ON SCHEMA _oo_load TO fixture_reader; '
              f'GRANT SELECT ON _oo_load.current, _oo_load.publications TO fixture_reader; '
              f'GRANT SELECT ON ALL TABLES IN SCHEMA oo_load_data TO fixture_reader; '
              f'COMMENT ON VIEW oo_load_data."{view}" IS \'Published fixture regions\'; '
              f'COMMENT ON COLUMN oo_load_data."{view}".id IS \'Source region key\';', admin=True)
        # Switch only this fixture to cert-only TCP authentication, after loading.
        f.command(["docker", "exec", "-i", "-u", "postgres", f.name, "bash", "-c",
                   'cat > "$PGDATA/pg_hba.conf"'],
                  data="local all all trust\nhostssl all all all cert\nhostnossl all all all reject\n")
        f.sql("SELECT pg_reload_conf();", admin=True)
        f.command(["cargo", "build", "--manifest-path", "tools/pg-builder/Cargo.toml", "--locked"],
                  timeout=1200)
        binary = str(ROOT / "target/debug/pg-builder")
        config = {
            "protocol": "oo.pg-builder.config/v1", "adapter": "loopback_http_local_dev",
            "namespace": "pg-builder", "purpose": "generate", "sample_policy": "none",
            "temperature": 0, "reasoning_effort": "low",
            "endpoint": {"origin": "http://127.0.0.1:8317", "path": "/v1/chat/completions",
                         "model_id": "gpt-6.1-sol"},
            "secret": {"mode": "model-egress"}, "declared_price": {"status": "unknown"},
            "budget": {"deadline_ms": 180000, "max_completion_tokens": 8192, "max_in_flight": 1,
                       "max_prompt_tokens": 50000, "max_request_bytes": 262144,
                       "max_response_bytes": 262144, "max_tables": 16, "max_total_tokens": 60000},
            "postgres": {"host": "localhost", "port": port, "database": "fixture",
                         "username": "fixture_reader", "password_file": str(p / "reader-password"),
                         "sslrootcert": {"secret_ref": str(p / "source-ca")},
                         "sslcert": {"secret_ref": str(p / "client-cert")},
                         "sslkey": {"secret_ref": str(p / "client-key")},
                         "catalog_bin": "unused-for-publications",
                         "tables": [{"schema": "oo_load_data", "table": view}]},
        }
        cfg = p / "builder.json"

        def invoke(stage, extra=(), ok=True):
            code, _ = f.command([binary, stage, "--config", str(cfg), *extra,
                                 "--out", str(out / (stage + ".json"))], required=ok)
            assert (code == 0) == ok

        save(cfg, config)
        cat_args = ["--source-id", "fixture", "--secret-ref", "secret://fixture"]
        for stage in ("catalog", "profile", "comments"):
            invoke(stage, cat_args if stage == "catalog" else ())
        captured = json.loads((out / "catalog.json").read_text())
        assert len(captured["tables"]) == 1 and captured["tables"][0]["relkind"] == "v"
        assert captured["tables"][0]["name"] == view
        assert captured["tables"][0]["comment"]["byte_len"] == len("Published fixture regions")
        assert captured["tables"][0]["columns"][0]["comment"]["byte_len"] == len("Source region key")
        assert captured["tables"][0]["primary_key"] == [{"attnum": 1, "name": "id"}]
        invoke("seal", ["--catalog", str(out / "catalog.json")])
        invoke("cards", [arg for name in ("catalog", "profile", "comments", "seal")
                         for arg in ("--" + name, str(out / (name + ".json")))])
        cases = {}
        for stage in ("catalog", "profile", "comments"):
            no_cert = copy.deepcopy(config)
            del no_cert["postgres"]["sslcert"]
            del no_cert["postgres"]["sslkey"]
            save(cfg, no_cert)
            invoke(stage, cat_args if stage == "catalog" else (), ok=False)
            cases[stage + "_without_cert"] = "refused"
        save(cfg, config)
        (p / "client-key").chmod(0o644)
        invoke("profile", ok=False)
        (p / "client-key").chmod(0o600)
        cases["public_key_file"] = "refused"
        for name in ("wrong_ca", "wrong_hostname", "missing_key", "inline_key"):
            bad = copy.deepcopy(config)
            if name == "wrong_ca":
                bad["postgres"]["sslrootcert"]["secret_ref"] = str(p / "client-cert")
            elif name == "wrong_hostname":
                bad["postgres"]["host"] = "127.0.0.1"
            elif name == "missing_key":
                del bad["postgres"]["sslkey"]
            else:
                bad["postgres"]["sslkey"] = "inline-not-allowed"
            save(cfg, bad)
            invoke("profile", ok=False)
            cases[name] = "refused"
        mixed = copy.deepcopy(config)
        mixed["postgres"]["tables"].append({"schema": "_oo_load", "table": "current"})
        save(cfg, mixed)
        invoke("catalog", cat_args, ok=False)
        cases["private_storage_selection"] = "refused"
        mixed["postgres"]["tables"] = [{"schema": "_oo_load", "table": "current"}]
        save(cfg, mixed)
        invoke("catalog", cat_args, ok=False)
        cases["private_storage_only"] = "refused"
        summary.update(status="cards_ready", engine="PostgreSQL", version=version.strip(),
                       datasets=1, retained_generations=2,
                       load_receipts=len(loaded["receipts"]), negative_cases=cases,
                       keys="primary_key_preserved_from_active_plan",
                       draft_replay="not_run",
                       live_model_call="not_run_no_authorized_credentials")
    finally:
        summary["cleanup_failures"] = f.cleanup()
        save(out / "summary.json", summary)
    print(json.dumps(summary))


if __name__ == "__main__":
    main()
