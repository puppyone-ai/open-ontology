#!/usr/bin/python3
"""Real C5 source producer for the native scheduler, with durable pin replay.

Runs the pinned reader and sealer. It never connects to the target or claims
jobs, and supplies no lease or successful gate. The operator selects the
owned fixture and approved columns in a private, immutable configuration.
"""
import argparse
import fcntl
import hashlib
import json
from pathlib import Path
import subprocess
import sys

from fresh_witness import TOKENS


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def save(path, obj):
    path.write_text(json.dumps(obj, indent=2) + "\n")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", type=Path, required=True)
    args = parser.parse_args()
    conf = json.loads(args.config.read_text())
    job = json.load(sys.stdin)
    if (job.get("protocol") != "oo.jobs.native-producer/v1" or job.get("source_id") != "clickhouse_268"
            or job.get("method") != "C5" or job.get("cycle") not in ("detect", "pull", "reconcile")
            or not isinstance(job.get("pin"), int) or job["pin"] < 1
            or job.get("binding") != conf["binding"]):
        raise RuntimeError("producer job/config binding mismatch")
    for name in ("driver", "sealer"):
        if sha(conf[name]) != conf[name + "_sha256"]:
            raise RuntimeError(name + " identity mismatch")
    root = Path(conf["output"]).resolve()
    root.mkdir(parents=True, exist_ok=True)
    with (root / "producer.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        out = root / ("pin-" + str(job["pin"]))
        out.mkdir(exist_ok=True)
        request_path = out / "request.json"
        if request_path.exists():
            request = json.loads(request_path.read_text())
            if sha(request["arrow_path"]) != request["input_digest"]:
                raise RuntimeError("saved pin Arrow changed")
            print(json.dumps(request))
            return

        def run(name, argv):
            result = subprocess.run(argv, capture_output=True)
            (out / (name + ".stdout")).write_bytes(result.stdout)
            (out / (name + ".stderr")).write_bytes(result.stderr)
            (out / (name + ".exit")).write_text(str(result.returncode) + "\n")
            if result.returncode:
                raise RuntimeError(name + " refused; raw bytes retained")

        completed_seal = out / "sealed/admission-receipt.json"
        if completed_seal.exists():
            # Recover the source-side crash window after sealer rename but
            # before request persistence. Reuse that exact capture, never
            # reread a changing source for the same bound pin.
            receipt = json.loads(completed_seal.read_text())
            capturer = json.loads((out / "capturer.json").read_text())
            registry = json.loads((out / "sealed/registry.json").read_text())
            bindings = registry.get("bindings", [])
            if (receipt.get("source_generation") != job["pin"] or receipt.get("input_digest") != sha(out / "full.arrow")
                    or receipt.get("catalog_sha256") != sha(out / "sealed/catalog.json")
                    or len(bindings) != 1
                    or bindings[0].get("admission_receipt_sha256") != sha(completed_seal)
                    or bindings[0].get("capture_receipt_sha256") != sha(out / "capturer.json")):
                raise RuntimeError("saved sealed pin changed")
            columns = json.loads((out / "schema-preimage.json").read_text())[0]["columns"]
            snap = capturer["snapshot"]
            snapshot = snap["value"]
        else:
            (out / "reader.user").write_text("oo_dl01w1_40_reader\n")
            base = [conf["driver"], "--endpoint", conf["endpoint"], "--user-file", str(out / "reader.user"),
                    "--password-file", conf["source_password_file"], "--row-cap", str(conf["hard_limit"]),
                "--byte-cap", "20000000", "--timeout-ms", "8000"]

            def read(name, sql, arrow=False):
                command = [base[0], "arrow" if arrow else "read", *base[1:], "--sql", sql,
                           "--query-id", "oo-dl01w1-40-native-p" + str(job["pin"]) + "-" + name,
                           "--out", str(out / (name + ".json"))]
                if arrow:
                    command += ["--ipc-out", str(out / "full.arrow")]
                else:
                    command += ["--body-out", str(out / (name + ".body"))]
                run(name, command)
                return json.loads((out / (name + ".json")).read_text())

            read("schema", "SELECT name,type FROM system.columns WHERE database='oo_dl01w40' AND table='readings' ORDER BY position FORMAT JSONEachRow")
            actual = [json.loads(line) for line in (out / "schema.body").read_text().splitlines()]
            if actual != conf["schema_columns"]:
                raise RuntimeError("schema_changed; capture paused before data read")
            read("mutations", "SELECT count() FROM system.mutations WHERE database='oo_dl01w40' AND table='readings' AND is_done=0 FORMAT TSV")
            if (out / "mutations.body").read_text().strip() != "0":
                raise RuntimeError("mutation_pending; DELETES_LAGGED; no sealed request")
            names = [item["name"] for item in actual]
            result = read("capture", "SELECT " + ",".join(names) + " FROM oo_dl01w40.readings FINAL ORDER BY reading_id FORMAT ArrowStream", True)
            rows = int(result["rows"])
            if rows > conf["hard_limit"]:
                raise RuntimeError("SOURCE_BUDGET_EXHAUSTED")
            read("keys", "SELECT toString(reading_id) FROM oo_dl01w40.readings FINAL ORDER BY reading_id FORMAT TSV")
            keys = (out / "keys.body").read_text().splitlines()
            if len(keys) != rows or len(set(keys)) != rows:
                raise RuntimeError("source changed during S0 read or duplicate key")
            columns = []
            for field in result["fields"]:
                token, precision, scale = TOKENS[field["data_type"]]
                item = {"name": field["name"], "logical_name": field["name"], "arrow_type": token, "nullable": field["nullable"]}
                if precision is not None:
                    item["precision"] = precision
                if scale is not None:
                    item["scale"] = scale
                if item["name"] == "reading_id":
                    item["key"] = True
                columns.append(item)
            snapshot = "oo-dl01w1-40-native-pin-" + str(job["pin"])
            snap = {"kind": "token", "value": snapshot}
            capturer = {"protocol": "oo.clickhouse.capture-tick/v1", "source_engine": "clickhouse", "source_id": "clickhouse_268",
                "source_generation": job["pin"], "label": snapshot, "snapshot": snap, "database": "oo_dl01w40", "logical_schema": "oo_dl01w40",
                "cross_table_consistency": "B", "product_reader_write_attempts": 0, "snapshot_token": False,
                "tables": [{"name": "readings", "arrow": "full.arrow", "cursor_safety": "no_cursor", "consistency": "S0", "route": "C5",
                    "delete_mode": "reconcile", "tombstones": [], "rows": keys, "row_count": rows}]}
            witness = {"protocol": "oo.source-admission.capture-witness/v1", "source_engine": "clickhouse", "source_id": "clickhouse_268",
                "source_generation": job["pin"], "snapshot": snap, "run_id": snapshot, "logical_schema": "oo_dl01w40", "logical_table": "readings",
                "capturer_path": str(out / "capturer.json"), "arrow_path": str(out / "full.arrow"), "driver_path": conf["driver"],
                "schema_preimage_path": str(out / "schema-preimage.json"), "type_preimage_path": str(out / "type-preimage.json")}
            report = json.loads(Path(conf["report_template"]).read_text())
            config = json.loads(Path(conf["admission_config"]).read_text())
            report["driver"]["digest"] = conf["driver_sha256"]
            report["capture_witness"] = str(out / "witness.json")
            report["capture_methods"] = []
            for name, body in {"capturer": capturer, "witness": witness, "schema-preimage": [{"table": "readings", "columns": columns}],
                    "type-preimage": [{"table": "readings", **{k: v for k, v in item.items() if k in ("name", "arrow_type", "precision", "scale")}} for item in columns],
                    "report": report, "config": config}.items():
                save(out / (name + ".json"), body)
            run("seal", [conf["sealer"], "seal-native", "--report", str(out / "report.json"), "--config", str(out / "config.json"), "--out", str(out / "sealed")])
        receipt = json.loads((out / "sealed/admission-receipt.json").read_text())
        request = {"protocol": "oo.pipeline.native-update/v1", "source_engine": "clickhouse", "source_id": "clickhouse_268",
            "source_generation": job["pin"], "logical_schema": "oo_dl01w40", "logical_table": "readings", "batch_id": snapshot,
            "capture_method": "c5_diff", "delete_detection": "reconcile", "consistency_grade": "S0", "cross_table_consistent": False,
            "position_codec": "snapshot-zero/v1", "position_lineage": "clickhouse.c5.full", "applied_pos": "0", "snapshot": snap,
            "columns": columns, "arrow_path": str(out / "full.arrow"), "tombstones": [], "row_identity": "unique", "key_column": "reading_id",
            "full_snapshot": True, "mirror": True, "admission_path": str(out / "sealed/admission-receipt.json"),
            "admission_expect": {"declared_receipt_sha256": sha(out / "sealed/admission-receipt.json")}, "target": conf["target"],
            "audit_digest": sha(out / "capture.json")}
        for name in ("config_digest", "schema_digest", "type_digest", "input_digest"):
            request[name] = receipt[name]
        if job["pin"] > 1:
            request["parent_source_generation"] = job["pin"] - 1
        save(request_path.with_suffix(".pending"), request)
        request_path.with_suffix(".pending").replace(request_path)
        print(json.dumps(request))


if __name__ == "__main__":
    try:
        main()
    except Exception as exc:
        print("clickhouse native producer: " + str(exc), file=sys.stderr)
        raise SystemExit(2)
