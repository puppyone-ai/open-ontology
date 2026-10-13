#!/usr/bin/env python3
"""Capture all original typed columns per actual partition, including zero rows.

Source-only proof/producer input preparation. This does not write a target,
seal an admission, or acknowledge a partition. Original static fixture gold
is an independent oracle for this unchanged baseline window only.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess

import fresh_capture as f


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--output", type=Path, required=True)
    p.add_argument("--gold", type=Path)
    p.add_argument("--capture-partitions", nargs="+")
    p.add_argument("--generation-start", type=int, default=1)
    a = p.parse_args()
    out = a.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    source = f.CH
    (out / "reader.user").write_text("oo_dl01w1_40_reader\n")

    def run(label, args):
        r = subprocess.run(args, capture_output=True)
        for suffix, body in [("stdout", r.stdout), ("stderr", r.stderr), ("exit", (str(r.returncode) + "\n").encode())]:
            (out / (label + "." + suffix)).write_bytes(body)
        if r.returncode:
            raise RuntimeError(label + " failed; original output retained")

    base = ["docker", "run", "--rm", "--pull", "never", "--network", f.NET,
        "--mount", "type=volume,source=oo-dl01w1-40-target,target=/target,readonly",
        "--mount", f"type=bind,source={out},target=/evidence",
        "--mount", f"type=bind,source={f.SECRETS}/reader.password,target=/source-reader.password,readonly"]
    columns = [json.loads(line)["name"] for line in (f.OUT / "native-columns.stdout").read_text().splitlines()]
    gold_path = a.gold or f.ROOT / "testing/dl01-driver-preflight/clickhouse/fixtures/gold-final.json"
    gold = json.loads(gold_path.read_text())
    def capture(label, sql, arrow=False):
        argv = base + ["--entrypoint", "/target/driver/debug/ch-reader", f.RUST_IMAGE, "arrow" if arrow else "read",
            "--endpoint", "http://" + source + ":8123", "--user-file", "/evidence/reader.user",
            "--password-file", "/source-reader.password", "--sql", sql,
            "--row-cap", "100000", "--byte-cap", "20000000", "--timeout-ms", "8000",
            "--query-id", "oo-dl01w1-40-c4-full-baseline-" + label, "--out", "/evidence/" + label + ".json"]
        argv += ["--ipc-out", "/evidence/" + label + ".arrow"] if arrow else ["--body-out", "/evidence/" + label + ".body"]
        run(label, argv)
    metadata_sql = {
        "parts": "SELECT partition,name,rows,bytes_on_disk,modification_time FROM system.parts WHERE database='oo_dl01w40' AND table='readings' AND active ORDER BY partition,name FORMAT JSONEachRow",
        "uuid": "SELECT toString(uuid) FROM system.tables WHERE database='oo_dl01w40' AND name='readings' FORMAT TSV",
        "mutations": "SELECT count() FROM system.mutations WHERE database='oo_dl01w40' AND table='readings' AND is_done=0 FORMAT TSV",
        "columns": "SELECT name,type FROM system.columns WHERE database='oo_dl01w40' AND table='readings' ORDER BY position FORMAT JSONEachRow",
    }
    for label, sql in metadata_sql.items():
        capture(label, sql)
    if (out / "mutations.body").read_text().strip() != "0":
        raise RuntimeError("mutation_pending; no partition ACK")
    parts = [json.loads(line) for line in (out / "parts.body").read_text().splitlines()]
    partition_ids = sorted({str(item["partition"]) for item in parts})
    capture_ids = a.capture_partitions or partition_ids + ["209912"]
    known_ids = {"202401", "202402", "202403", "209912"}
    if set(capture_ids) - known_ids or len(set(capture_ids)) != len(capture_ids):
        raise RuntimeError("capture scope differs from the retained owned fixture")
    if a.capture_partitions is None and partition_ids != ["202401", "202403"]:
        raise RuntimeError("baseline fixture changed; independent static gold cannot validate this window")
    if a.capture_partitions is not None and (not set(partition_ids) <= {"202401", "202402", "202403"}
            or "202401" not in partition_ids):
        raise RuntimeError("update/delete window source partitions differ")
    summaries = []
    for partition in capture_ids:
        expected = [row for row in gold if row["ts"].replace("-", "")[:6] == partition]
        (out / (partition + "-gold.json")).write_text(json.dumps(expected, indent=2) + "\n")
        sql = "SELECT " + ",".join(columns) + " FROM oo_dl01w40.readings FINAL WHERE _partition_id='" + partition + "' ORDER BY reading_id FORMAT ArrowStream"
        capture(partition, sql, True)
        run(partition + "-allcells", base + ["--entrypoint", "/target/driver/debug/ch-decode", f.RUST_IMAGE,
            "compare", "--schema", "readings", "--ipc", "/evidence/" + partition + ".arrow",
            "--gold", "/evidence/" + partition + "-gold.json", "--out", "/evidence/" + partition + "-allcells.json"])
        result = json.loads((out / (partition + ".json")).read_text())
        verdict = json.loads((out / (partition + "-allcells.json")).read_text())
        summaries.append({"partition": partition, "source_rows": result["rows"], "record_batches": result["batches"],
            "arrow_sha256": hashlib.sha256((out / (partition + ".arrow")).read_bytes()).hexdigest(),
            "all_original_fields": len(result["fields"]), "independent_allcells_passed": verdict["passed"], "acknowledged": False})
    # The partition scope must still be the source observation after every FINAL read.
    # A concurrent mutation cannot be turned into a trusted replacement witness.
    for label, sql in metadata_sql.items():
        capture(label + "-after", sql)
        if (out / (label + ".body")).read_bytes() != (out / (label + "-after.body")).read_bytes():
            raise RuntimeError(label + " changed during capture; no partition ACK")
    observed_columns = [json.loads(line)["name"] for line in (out / "columns.body").read_text().splitlines()]
    if observed_columns != columns:
        raise RuntimeError("source schema changed; no partition ACK")
    column_rows = [json.loads(line) for line in (out / "columns.body").read_text().splitlines()]
    schema_fingerprint = "\n".join(item["name"] + "\t" + item["type"] for item in column_rows)
    observed = []
    for summary in summaries:
        partition = summary["partition"]
        active = [item for item in parts if str(item["partition"]) == partition]
        signature = None
        if active:
            signature = "|".join((partition, str(sum(int(item["rows"]) for item in active)),
                str(sum(int(item["bytes_on_disk"]) for item in active)),
                max(str(item["modification_time"]) for item in active),
                ",".join(sorted(str(item["name"]) for item in active))))
        observed.append({"partition_id": partition, "table_epoch": (out / "uuid.body").read_text().strip(),
            "schema_fingerprint": schema_fingerprint, "mutations_pending": False,
            "current_signature": signature, "arrow_sha256": summary["arrow_sha256"],
            "row_count": summary["source_rows"], "previous_signature_source": "authoritative_target_ack_required"})
    (out / "source-observation.json").write_text(json.dumps({
        "protocol": "oo.clickhouse.c4-source-observation/v1", "source_id": "clickhouse_268",
        "logical_schema": "oo_dl01w40", "logical_table": "readings", "partitions": observed,
        "observation_only": True, "acknowledged": False}, indent=2) + "\n")
    # Materialize source-bound admission inputs. The prior ACK belongs to Loader.
    pin = f.OUT / "scheduled-capture/pin-1"
    schema = json.loads((pin / "schema-preimage.json").read_text())
    if [item["name"] for item in schema[0]["columns"]] != observed_columns:
        raise RuntimeError("registered native schema differs from source")
    driver = "/tmp/oo-dl01w1-40-gpt-producer/ch-reader"
    driver_sha = f.DRIVER_SHA
    for generation, part in enumerate(observed, start=a.generation_start):
        partition = part["partition_id"]
        folder = out / ("admission-" + partition)
        folder.mkdir()
        shutil.copyfile(out / (partition + ".arrow"), folder / (partition + ".arrow"))
        for name in ("schema-preimage.json", "type-preimage.json", "config.json"):
            shutil.copyfile(pin / name, folder / name)
        # Control generations order partition applies; they are not source CDC offsets.
        scope = {"source_id": "clickhouse_268", "source_generation": generation,
            "logical_schema": "oo_dl01w40", "logical_table": "readings",
            "partition_id": partition, "observed_partition_ids": sorted(known_ids),
            "complete": True, "table_epoch": part["table_epoch"],
            "schema_fingerprint": schema_fingerprint,
            "schema_digest": hashlib.sha256((folder / "schema-preimage.json").read_bytes()).hexdigest(),
            "type_digest": hashlib.sha256((folder / "type-preimage.json").read_bytes()).hexdigest(),
            "arrow_sha256": part["arrow_sha256"], "row_count": part["row_count"],
            "current_signature": part["current_signature"], "mutations_pending": False}
        snapshot = "oo-dl01w1-40-c4-observed-" + partition
        capturer = {"protocol": "oo.clickhouse.capture-tick/v1", "source_engine": "clickhouse",
            "source_id": "clickhouse_268", "source_generation": generation,
            "label": snapshot, "snapshot": {"kind": "token", "value": snapshot},
            "database": "oo_dl01w40", "logical_schema": "oo_dl01w40",
            "logical_table": "readings", "table_epoch": part["table_epoch"],
            "cross_table_consistency": "B", "product_reader_write_attempts": 0,
            "snapshot_token": False, "partition_scope": scope,
            "tables": [{"name": "readings", "arrow": partition + ".arrow",
                "cursor_safety": "no_cursor", "consistency": "S0", "route": "C4",
                "delete_mode": "partition_replace", "tombstones": [],
                "row_count": part["row_count"]}]}
        witness = {"protocol": "oo.source-admission.capture-witness/v1",
            "source_engine": "clickhouse", "source_id": "clickhouse_268",
            "source_generation": generation, "snapshot": capturer["snapshot"], "run_id": snapshot,
            "logical_schema": "oo_dl01w40", "logical_table": "readings",
            "capturer_path": str(folder / "capturer.json"),
            "arrow_path": str(folder / (partition + ".arrow")),
            "driver_path": driver,
            "schema_preimage_path": str(folder / "schema-preimage.json"),
            "type_preimage_path": str(folder / "type-preimage.json")}
        report = json.loads((pin / "report.json").read_text())
        report["capture_witness"] = str(folder / "witness.json")
        report["driver"]["digest"] = driver_sha
        for name, body in (("capturer", capturer), ("witness", witness), ("report", report)):
            (folder / (name + ".json")).write_text(json.dumps(body, indent=2) + "\n")
    (out / "summary.json").write_text(json.dumps({"source_container": source, "table_uuid": (out / "uuid.body").read_text().strip(),
        "partitions": summaries, "source_only": True, "target_replacement_run": False, "c4_accepted": False}, indent=2) + "\n")
    print(json.dumps(summaries))


if __name__ == "__main__":
    main()
