#!/usr/bin/env python3
"""Lane G: owned oo-load Agent DB -> live thousand-table T5 -> data-only T4."""
import argparse
import copy
import json
import os
from pathlib import Path
import signal
import threading
import time

import live_selfbuild_acceptance as live
from test_load_mtls import Fixture, configure_tls

builder = live.builder


class LimitedEgress(live.egress_module.Egress):
    """Reserve before dispatch, including probes, failures and concurrent repairs."""
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.admissions = 0
        self.reserved_tokens = 0
        self.deadline = time.monotonic() + 3600
        self.limit_lock = threading.Lock()

    def call(self, request):
        # Byte count deliberately overestimates local tokenization. Proxy overhead
        # is an allowance, not a provider-enforced billing/currency guarantee.
        reserve = len(builder.canonical(request).encode()) + 10000 + request["max_tokens"]
        with self.limit_lock:
            if (self.admissions >= 80 or self.reserved_tokens + reserve > 2_000_000
                    or time.monotonic() >= self.deadline):
                raise live.egress_module.Refusal("USAGE_LIMIT", 429)
            self.admissions += 1
            self.reserved_tokens += reserve
        yield from super().call(request)


def agent_sql(run, sql):
    return run.command(["docker", "exec", "-i", "-u", "postgres", run.name,
                        "psql", "-X", "-v", "ON_ERROR_STOP=1", "-At",
                        "-U", "postgres", "-d", "agent"], data=sql)[1]


def loaded_inventory(source, target, receipts):
    """Use physical Agent columns; join source FK facts by verified load identity."""
    physical = {t["dataset"]: t for t in target["tables"]}
    expected = {t["dataset"]: t for t in source["tables"]}
    if len(receipts) != len(expected) or len(physical) != len(expected):
        raise ValueError("acceptance_load_count")
    tables, seen = [], set()
    for receipt in receipts:
        name = receipt["table"]["schema"] + "." + receipt["table"]["name"]
        if name not in expected or name in seen or not receipt["receipts"]:
            raise ValueError("acceptance_load_identity")
        seen.add(name)
        original, actual = expected[name], physical[receipt["target"]]
        if [c["name"] for c in original["columns"]] != [c["name"] for c in actual["columns"]]:
            raise ValueError("acceptance_load_columns")
        table = copy.deepcopy(original)
        table["columns"] = actual["columns"]
        # Views do not preserve PK/FK constraints. Source metadata is explicit
        # provenance, never a claim that physical target constraints exist.
        tables.append(table)
    return {"tables": sorted(tables, key=lambda t: t["dataset"]),
            "basis": "Agent DB physical columns; source PK/FK metadata bound by oo-load receipts"}


def bounded_communities(result, max_output):
    communities = result["graph"]["communities"]
    batches = [b for c in communities for b in c["batches"]]
    if not batches or any(b["input_tokens"] > b["input_available"]
                          or b["output_reserve"] > max_output for b in batches):
        raise ValueError("acceptance_community_budget")
    return {"communities": len(communities), "batches": len(batches),
            "all_within_budget": True,
            "max_input_tokens": max(b["input_tokens"] for b in batches),
            "max_output_reserve": max(b["output_reserve"] for b in batches)}


def prepare(run):
    p = run.private
    configure_tls(run, p)
    (p / "extensions").write_text(
        "basicConstraints=critical,CA:FALSE\nsubjectAltName=DNS:localhost\n"
        "keyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n")
    run.command(["openssl", "x509", "-req", "-in", str(p / "server-csr"),
                 "-CA", str(p / "source-ca"), "-CAkey", str(p / "ca-key"),
                 "-CAcreateserial", "-out", str(p / "server-cert"), "-days", "2",
                 "-extfile", str(p / "extensions")])
    run.setup()
    run.seed()
    run.verify_shapes()
    run.gold()
    # SourceEngine rejects startup GUCs and sets read-only transactions itself.
    # The principal retains SELECT-only grants, independent of this session GUC.
    run.sql(f"ALTER ROLE fixture_reader PASSWORD '{run.reader_secret}'; "
            "ALTER ROLE fixture_reader RESET default_transaction_read_only; "
            "GRANT pg_read_all_stats TO fixture_reader;", admin=True)
    run.sql("CREATE DATABASE agent;", admin=True)
    for path in p.iterdir():
        path.chmod(0o600)
    p.chmod(0o700)
    _, address = run.command(["docker", "port", run.name, "5432/tcp"])
    return int(address.strip().rsplit(":", 1)[1])


def load_agent(run, port, source, output, generation, only=None):
    p = run.private
    names = only or [t["dataset"] for t in source["tables"]]
    builder.write_private(p / "stack.json", {
        "version": "oo.stack/v1",
        "sources": [{"id": "fixture", "engine": "postgres", "host": "localhost",
                     "port": port, "database": "fixture", "user": "fixture_reader",
                     "password": {"secret_ref": "reader-password"},
                     "tls_ca": {"secret_ref": "source-ca"}, "scope": {"include": names}}],
        "model": {"provider": "openai-compatible", "base_url": "https://example.invalid/v1",
                  "model": "gpt-6.1-sol", "key": {"secret_ref": "unused"}}})
    builder.write_private(p / "target.json", {
        "host": "localhost", "port": port, "database": "agent", "user": "postgres",
        "password": {"secret_ref": "password"}, "tls_ca": {"secret_ref": "source-ca"},
        "run_id": generation})
    command = [
        str(Path(os.environ.get("CARGO_TARGET_DIR", live.ROOT / "target")) / "debug/oo-load"),
        "--stack", str(p / "stack.json"), "--source", "fixture",
        "--target", str(p / "target.json"), "--secrets-dir", str(p)]
    receipts = []
    # SourceEngine rechecks the selected catalog on each chunk. Bound that
    # metadata scope without reducing the overall fixture/table coverage.
    for start in range(0, len(names), 32):
        _, raw = run.command(command + ["--tables", ",".join(names[start:start + 32])], timeout=1800)
        receipts.extend(json.loads(line) for line in raw.splitlines() if line.strip())
    builder.write_private(output / (generation + "-receipts.json"), receipts)
    return receipts


def inventory(run, source, receipts, output, name):
    sql = live.CATALOG.replace("n.nspname='public'", "n.nspname='oo_load_data'")
    target = json.loads(agent_sql(run, sql))
    builder.write_private(output / (name + "-physical.json"), target)
    document = loaded_inventory(source, target, receipts)
    builder.write_private(output / (name + ".json"), document)
    return document


def verify_rows(run, source, receipts):
    names = {r["table"]["schema"] + "." + r["table"]["name"]: r["target"] for r in receipts}
    source_counts, target_counts = [], []
    for table in source["tables"]:
        name = table["dataset"]
        source_counts.append(f"SELECT '{name}' AS name, count(*) AS rows FROM {name}")
        target_counts.append(f"SELECT '{name}' AS name, count(*) AS rows FROM {names[name]}")
    query = lambda parts: "SELECT json_object_agg(name,rows) FROM (" + " UNION ALL ".join(parts) + ") counts;"
    left = json.loads(run.sql(query(source_counts))[1])
    right = json.loads(agent_sql(run, query(target_counts)))
    if left != right:
        raise ValueError("acceptance_load_rows")
    builder.write_private(run.path.parent / "load-row-counts.json", right)
    return {"tables": len(right), "rows": sum(right.values()), "all_match": True}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--execute", action="store_true")
    args = parser.parse_args()
    if not args.execute:
        parser.error("--execute is required for paid calls and an owned fixture")
    if args.config.is_symlink() or args.config.stat().st_mode & 0o777 != 0o600:
        parser.error("config must be a regular owner-private 0600 file")
    stack = json.loads(args.config.read_text())
    if stack["model"]["model"] != "gpt-6.1-sol":
        parser.error("acceptance requires gpt-6.1-sol")
    os.umask(0o077)
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    (out / "postgres-17").mkdir()
    run = Fixture(out / "postgres-17", "postgres", "17",
                  argparse.Namespace(platform=None, scale=1000, tables=1000, health_timeout=180))
    report = {"status": "failed", "limits": {"calls": 80, "reserved_tokens": 2_000_000,
              "dispatch_seconds": 3600, "per_request_timeout_seconds": 300,
              "currency_cost": "unknown; no provider price configured"}}
    print(builder.canonical(report["limits"]), flush=True)
    egress = httpd = thread = None
    def interrupt(*_):
        raise KeyboardInterrupt()
    signal.signal(signal.SIGTERM, interrupt)
    try:
        port = prepare(run)
        report["version"] = run.sql("SHOW server_version;")[1].strip()
        # oo-load admits physical tables, not source views. The generator's
        # order_totals view is excluded explicitly, never counted as a table.
        source = json.loads(run.sql(live.CATALOG.replace("('r','p','v')", "('r','p')"))[1])
        builder.write_private(out / "source.json", source)
        if len(source["tables"]) < 1000:
            raise ValueError("acceptance_not_thousand_tables")
        receipts = load_agent(run, port, source, out, "initial")
        report["load"] = verify_rows(run, source, receipts)
        document = inventory(run, source, receipts, out, "agent")
        egress = LimitedEgress(stack["model"], out / "usage.jsonl", timeout=300)
        probe = egress.probe()
        builder.write_private(out / "probe.json", probe)
        httpd = live.egress_module.server(egress, "127.0.0.1", 0)
        thread = threading.Thread(target=httpd.serve_forever, daemon=True)
        thread.start()
        config = {"model": "gpt-6.1-sol", "transport": "model-egress",
                  "endpoint": f"http://127.0.0.1:{httpd.server_port}/v1",
                  "capability_probe": probe, "request_timeout_seconds": 330,
                  "seconds_per_batch": [1, 300]}
        if "price_table" in stack:
            config["price_table"] = stack["price_table"]
        before = len(live.meter_rows(out / "usage.jsonl"))
        initial = builder.Runner(config, out / "draft").run(document["tables"], [])
        report["T5"] = dict(live.coverage(document, initial),
                            **bounded_communities(initial, probe["max_output_tokens"]),
                            model_calls=initial["model_calls"],
                            metered_calls=len(live.meter_rows(out / "usage.jsonl")) - before)
        if report["T5"]["model_calls"] != report["T5"]["metered_calls"]:
            raise ValueError("acceptance_meter_mismatch")
        run.sql("UPDATE unique_only SET value_no=value_no+1;", admin=True)
        refreshed = load_agent(run, port, source, out, "data-only", ["public.unique_only"])
        receipts = [r for r in receipts if r["table"]["name"] != "unique_only"] + refreshed
        report["data_only_load"] = verify_rows(run, source, receipts)
        current = inventory(run, source, receipts, out, "data-only")
        view = refreshed[0]["target"]
        if int(agent_sql(run, f"SELECT value_no FROM {view};").strip()) != 2:
            raise ValueError("acceptance_data_not_refreshed")
        before = len(live.meter_rows(out / "usage.jsonl"))
        result = builder.Runner(config, out / "data-only").run(current["tables"], [], previous=initial)
        report["T4"] = dict(live.coverage(current, result), model_calls=result["model_calls"],
                           metered_calls=len(live.meter_rows(out / "usage.jsonl")) - before,
                           work=result["work"], refreshed_value=2)
        if report["T4"]["model_calls"] or report["T4"]["metered_calls"] or any(result["work"].values()):
            raise ValueError("acceptance_data_only_called_model")
        report["status"] = "passed"
    except (Exception, KeyboardInterrupt) as error:
        report["error_type"] = type(error).__name__
        report["error"] = str(error) if isinstance(error, (ValueError, RuntimeError)) else "acceptance_failed"
    finally:
        if httpd:
            httpd.shutdown()
            httpd.server_close()
            thread.join()
        if egress:
            report["budget"] = {"admissions": egress.admissions,
                                "reserved_tokens": egress.reserved_tokens}
            egress.close()
        report["cleanup_failures"] = run.cleanup()
        if report["cleanup_failures"]:
            report["status"] = "failed"
        builder.write_private(out / "result.json", report)
        live.fixture.checksums(out)
    print(builder.canonical(report))
    return 0 if report["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
