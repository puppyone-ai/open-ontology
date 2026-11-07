#!/usr/bin/env python3
"""Opt-in T4/T5 on lane H's owned PostgreSQL fixture via metered egress."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import shutil
import signal
import sys
import threading
import time

import selfbuild_budget as builder

ROOT = Path(__file__).resolve().parents[3]


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


fixture = load("oo_test_fixture", ROOT / "tools/oo-test/runner.py")
egress_module = load("model_egress", ROOT / "tools/model-egress/model_egress.py")


class LaneRun(fixture.Run):
    """Use oo-test unchanged, with this lane's resource ownership."""
    def __init__(self, *args):
        super().__init__(*args)
        self.name = "oo-g-" + self.id
        self.volume = self.name + "-data"

    def command(self, command, **kwargs):
        command = ["oo.lane=g" if arg == "oo.lane=h" else arg for arg in command]
        return super().command(command, **kwargs)

    def cleanup(self):
        failures = []
        for kind, name, created in (
            ("container", self.name, self.created_container),
            ("volume", self.volume, self.created_volume),
        ):
            if not created:
                continue
            selector = "{{json .Config.Labels}}" if kind == "container" else "{{json .Labels}}"
            code, raw = self.command(
                ["docker", kind, "inspect", "--format", selector, name], required=False)
            try:
                labels = json.loads(raw)
            except ValueError:
                labels = {}
            if code or labels.get("oo.lane") != "g" or labels.get("oo.run_id") != self.id:
                failures.append(name)
                continue
            if kind == "container":
                self.command(["docker", "logs", name], required=False)
                self.command(["docker", "stats", "--no-stream", name], required=False)
            command = ["docker", kind, "rm"] + (["-f"] if kind == "container" else []) + [name]
            if self.command(command, required=False)[0]:
                failures.append(name)
        for path in self.private.iterdir():
            path.unlink()
        self.private.rmdir()
        receipt = getattr(self, "receipt", {})
        receipt.update(lane="g", cleanup_failed=failures,
                       elapsed_seconds=round(time.monotonic() - self.started, 3))
        fixture.write_json(self.path / "resource-receipt.json", receipt)
        return failures


CATALOG = """
SELECT json_build_object('tables',json_agg(t ORDER BY t->>'dataset')) FROM (
 SELECT json_build_object(
  'dataset',n.nspname||'.'||c.relname,
  'primary_key',COALESCE((SELECT json_agg(a.attname ORDER BY k.ord)
    FROM pg_constraint p CROSS JOIN LATERAL unnest(p.conkey) WITH ORDINALITY k(num,ord)
    JOIN pg_attribute a ON a.attrelid=c.oid AND a.attnum=k.num
    WHERE p.conrelid=c.oid AND p.contype='p'),'[]'::json),
  'columns',(SELECT json_agg(json_build_object('name',a.attname,
    'data_type',format_type(a.atttypid,a.atttypmod),'nullable',NOT a.attnotnull)
    ORDER BY a.attnum) FROM pg_attribute a
    WHERE a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped),
  'foreign_keys',COALESCE((SELECT json_agg(json_build_object(
    'referenced_dataset',rn.nspname||'.'||rc.relname,
    'columns',(SELECT json_agg(a.attname ORDER BY k.ord)
       FROM unnest(p.conkey) WITH ORDINALITY k(num,ord)
       JOIN pg_attribute a ON a.attrelid=c.oid AND a.attnum=k.num),
    'referenced_columns',(SELECT json_agg(a.attname ORDER BY k.ord)
       FROM unnest(p.confkey) WITH ORDINALITY k(num,ord)
       JOIN pg_attribute a ON a.attrelid=rc.oid AND a.attnum=k.num)))
    FROM pg_constraint p JOIN pg_class rc ON rc.oid=p.confrelid
    JOIN pg_namespace rn ON rn.oid=rc.relnamespace
    WHERE p.conrelid=c.oid AND p.contype='f'),'[]'::json)
 ) t FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE n.nspname='public' AND c.relkind IN ('r','p','v')
) inventory;
"""


def catalog(run, path):
    _, raw = run.sql(CATALOG)
    document = json.loads(raw)
    builder.write_private(path, document)
    return document


def meter_rows(path):
    return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []


def coverage(document, result):
    expected = {builder.stable_id("table", t["dataset"]) for t in document["tables"]}
    expected |= {builder.stable_id("column", t["dataset"], c["name"])
                 for t in document["tables"] for c in t["columns"]}
    if result["unresolved"] or not expected <= result["accepted"].keys():
        raise ValueError("acceptance_incomplete_inventory")
    summary = result["review"]["summary"]
    if summary["unmapped_without_reason"]:
        raise ValueError("acceptance_unaccounted_declared_fk")
    return dict(tables=len(document["tables"]), items=len(expected),
                columns=sum(len(t["columns"]) for t in document["tables"]),
                declared_fk=summary["declared_fk_count"],
                mapped_fk=summary["mapped_fk_count"],
                fk_rejections=summary["rejection_reasons"],
                orphan_ratio=summary["orphan_entity_ratio"],
                components=summary["connected_components"])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", required=True, help="private egress stack JSON")
    parser.add_argument("--output", required=True)
    parser.add_argument("--tables", type=int, default=660)
    parser.add_argument("--retained-receipts", type=Path)
    parser.add_argument("--base-result", type=Path, help="reuse a completed snapshot for small T4 runs")
    parser.add_argument("--connected-mutations", action="store_true")
    parser.add_argument("--execute", action="store_true")
    args = parser.parse_args()
    if not args.execute:
        parser.error("--execute is required for paid model calls and owned fixture creation")
    if args.tables != 660:
        parser.error("this bounded acceptance runs only the requested 660-table generator")
    os.umask(0o077)
    out = Path(args.output).resolve()
    out.mkdir(parents=True, exist_ok=True)
    dbpath = out / "postgres-17"
    dbpath.mkdir()
    # Row volume is irrelevant to schema T4/T5; this is not T1/T2 throughput evidence.
    options = argparse.Namespace(platform=None, health_timeout=None, scale=1000, tables=args.tables)
    run = LaneRun(dbpath, "postgres", "17", options)
    egress, httpd, thread = None, None, None
    report = dict(status="failed", generator_tables=args.tables, row_scale=1000, cycles={})
    def interrupt(*_):
        raise KeyboardInterrupt()
    signal.signal(signal.SIGTERM, interrupt)
    try:
        run.setup()
        run.seed()
        run.verify_shapes()
        run.gold()
        document = catalog(run, out / "input.json")
        stack = json.loads(Path(args.config).read_text())
        meter = out / "usage.jsonl"
        egress = egress_module.Egress(stack["model"], meter, timeout=600)
        probe = egress.probe()
        builder.write_private(out / "probe.json", probe)
        httpd = egress_module.server(egress, "127.0.0.1", 0)
        thread = threading.Thread(target=httpd.serve_forever, daemon=True)
        thread.start()
        config = dict(model=stack["model"]["model"], transport="model-egress",
                      endpoint=f"http://127.0.0.1:{httpd.server_port}/v1",
                      capability_probe=probe, request_timeout_seconds=660)
        draft = out / "draft"
        if args.retained_receipts:
            destination = draft / "receipts"
            destination.mkdir(parents=True, exist_ok=True)
            for receipt in args.retained_receipts.glob("*.json"):
                shutil.copyfile(receipt, destination / receipt.name)
        before = len(meter_rows(meter))
        runner = builder.Runner(config, draft)
        previous_snapshot = json.loads(args.base_result.read_text()) if args.base_result else None
        initial = runner.run(document["tables"], [], previous=previous_snapshot)
        report["T5"] = dict(coverage(document, initial),
                            model_calls=initial["model_calls"],
                            metered_calls=len(meter_rows(meter)) - before,
                            reused_receipts=sum(r["cache"] == "hit" for r in runner.receipts))
        report["T5"]["resumed_snapshot"] = previous_snapshot is not None
        if report["T5"]["model_calls"] != report["T5"]["metered_calls"]:
            raise ValueError("acceptance_meter_mismatch")
        previous = initial
        mutations = [
            ("data_only", "UPDATE unique_only SET value_no=value_no+1;", None),
            ("add_column", "ALTER TABLE unique_only ADD COLUMN added_note TEXT;", "public.unique_only"),
            ("type_change", "ALTER TABLE unique_only ALTER COLUMN value_no TYPE TEXT USING value_no::TEXT;", "public.unique_only"),
            ("new_table", "CREATE TABLE acceptance_new (id BIGINT PRIMARY KEY); "
             "GRANT SELECT ON acceptance_new TO fixture_reader;", "public.acceptance_new"),
        ]
        if args.connected_mutations:
            mutations[:3] = [
                ("data_only", "UPDATE customers SET note='fixture update' WHERE id=1;", None),
                ("add_column", "ALTER TABLE customers ADD COLUMN added_note TEXT;", "public.customers"),
                ("type_change", "ALTER TABLE customers ALTER COLUMN note TYPE VARCHAR(200);", "public.customers"),
            ]
        for name, sql, affected in mutations:
            run.sql(sql, admin=True)
            current = catalog(run, out / (name + "-input.json"))
            before = len(meter_rows(meter))
            result = builder.Runner(config, out / name).run(current["tables"], [], previous=previous)
            measured = len(meter_rows(meter)) - before
            evidence = dict(coverage(current, result), model_calls=result["model_calls"],
                            metered_calls=measured, work=result["work"])
            report["cycles"][name] = evidence
            if measured != result["model_calls"]:
                raise ValueError("acceptance_meter_mismatch")
            if affected is None:
                if measured or any(result["work"].values()):
                    raise ValueError("acceptance_data_only_called_model")
            else:
                affected_communities = {c["id"] for c in result["graph"]["communities"]
                                        if affected in c["tables"]}
                if set(result["work"]["communities"]) != affected_communities or measured == 0:
                    raise ValueError("acceptance_unscoped_schema_work")
                if result["work"]["merge_items"] != [builder.stable_id("entity", affected)]:
                    raise ValueError("acceptance_unscoped_merge")
                affected_edges = {e["id"] for e in result["graph"]["cut_edges"]
                                  if affected in (e["from"], e["to"])}
                if set(result["work"]["cut_edges"]) != affected_edges:
                    raise ValueError("acceptance_unscoped_cut_edges")
            previous = result
        report["status"] = "passed"
    except (Exception, KeyboardInterrupt) as error:
        # Provider and fixture errors are fixed codes; never serialize credentials.
        report["error_type"] = type(error).__name__
        report["error"] = str(error) if isinstance(error, (ValueError, RuntimeError)) else "acceptance_failed"
    finally:
        if httpd:
            httpd.shutdown()
            httpd.server_close()
            thread.join()
        if egress:
            egress.close()
        if run.cleanup():
            report["status"] = "failed"
            report["cleanup"] = "failed"
        builder.write_private(out / "result.json", report)
        fixture.checksums(out)
    print(json.dumps(report))
    return 0 if report["status"] == "passed" else 1


if __name__ == "__main__":
    sys.exit(main())
