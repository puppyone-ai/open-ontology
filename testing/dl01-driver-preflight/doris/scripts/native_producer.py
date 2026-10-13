#!/usr/bin/python3
"""Native scheduler producer: real Doris R0 capture, trusted seal, loader input.

Runs inside the Linux worker namespace. It receives a scheduler binding and pin
on stdin and emits only the actual native-update request on stdout. Credentials
are read by the reader/loader from mounted private files, never by the Agent.
"""
import argparse
import fcntl
import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path
from uuid import UUID

DEPENDENCY_SHA = "981e51e0cefd70d2cea97fe4c612ed6c5b4ff175045701e768a1a06ba828bce1"
if hashlib.sha256(Path(__file__).with_name("capture_receipt.py").read_bytes()).hexdigest() != DEPENDENCY_SHA:
    raise SystemExit("Doris producer schema dependency changed")
from capture_receipt import TABLES, column_decl

READER_SHA = "613612c3245a8bbb83d7534d75b6babc2aedd351ed91c9d7948c8df2b5570e31"
PARTITION_READER_SHA = "cccd1134f02b08137466cf876a27c9f1981cdfef64e7644e1aa5571ea9ce474e"
VALIDATOR_SHA = "1628c96da4bb3e71f17ab3fba4385c27d810e5cac89e424f6e777bc69dcbcfc0"


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def document(path, body):
    path.write_text(json.dumps(body, indent=2) + "\n")


def durable_bytes(path, data):
    temporary = path.with_suffix(".pending")
    with temporary.open("wb") as handle:
        handle.write(data)
        handle.flush()
        os.fsync(handle.fileno())
    os.replace(temporary, path)
    directory = os.open(path.parent, os.O_RDONLY)
    try:
        os.fsync(directory)
    finally:
        os.close(directory)


def execute(args, output, name):
    result = subprocess.run(args, capture_output=True, timeout=300)
    for suffix, data in (("stdout", result.stdout), ("stderr", result.stderr),
                         ("exit", (str(result.returncode) + "\n").encode())):
        (output / f"{name}.{suffix}").write_bytes(data)
    if result.returncode:
        raise ValueError("capture or admission refused")


def main():
    parser = argparse.ArgumentParser()
    for field in ("reader", "validator", "evidence", "config", "schema-preimage",
                  "type-preimage", "reader-secrets", "target-secret", "target-host"):
        parser.add_argument("--" + field, required=True)
    parser.add_argument("--table", choices=TABLES, required=True)
    parser.add_argument("--partition-reader")
    parser.add_argument("--base-generation", type=int, default=2)
    parser.add_argument("--source-host", default="172.16.143.10")
    parser.add_argument("--target-ca", default="/tmp/oo-dl01w1-43-native-tls/ca.pem")
    args = parser.parse_args()
    incoming = json.load(sys.stdin)
    if incoming.get("protocol") != "oo.jobs.native-producer/v1" or incoming.get("source_id") != "dorispreview":
        raise ValueError("producer binding")
    binding = str(UUID(incoming["binding"]))
    pin = incoming["pin"]
    if not isinstance(pin, int) or pin < 1 or args.base_generation < 1:
        raise ValueError("producer pin")
    c4 = args.table == "part_day"
    reader = args.partition_reader if c4 else args.reader
    if reader is None:
        raise ValueError("partition reader required for C4")
    if (digest(reader) != (PARTITION_READER_SHA if c4 else READER_SHA)
            or digest(args.validator) != VALIDATOR_SHA):
        raise ValueError("producer executable identity")
    if incoming.get("method") != ("C4" if c4 else "C5"):
        raise ValueError("producer method")
    acknowledged = incoming.get("acknowledged_source_generation")
    if acknowledged is None:
        acknowledged = args.base_generation
    if not isinstance(acknowledged, int) or acknowledged < args.base_generation:
        raise ValueError("acknowledged source generation")
    generation = acknowledged + 1
    pin_dir = Path(args.evidence) / binding / str(pin)
    pin_dir.mkdir(parents=True, exist_ok=True)
    lock = (pin_dir / "producer.lock").open("a")
    fcntl.flock(lock.fileno(), fcntl.LOCK_EX)
    identity = hashlib.sha256(json.dumps(
        {"input": incoming, "argv": sys.argv[1:], "dependency": DEPENDENCY_SHA,
         "producer": digest(__file__)},
        sort_keys=True, separators=(",", ":")).encode()).hexdigest()
    successful = pin_dir / "successful-request.json"
    metadata = pin_dir / "successful-metadata.json"
    if successful.exists():
        saved = successful.read_bytes()
        prior = json.loads(metadata.read_text())
        if prior != {"identity": identity, "request_sha256": hashlib.sha256(saved).hexdigest()}:
            raise ValueError("producer replay identity")
        sys.stdout.buffer.write(saved)
        return
    # Preserve failed attempts. Once a successful request is durably saved,
    # a crash before the worker saves stdout reuses exactly these same bytes.
    for attempt in range(1, 1000):
        output = pin_dir / str(attempt)
        try:
            output.mkdir()
            break
        except FileExistsError:
            continue
    else:
        raise ValueError("producer retry budget")
    label = "n" + str(pin) + "a" + str(attempt) + binding.replace("-", "")[:12]
    document(output / "scheduler-input.json", incoming)
    config = json.loads(Path(args.config).read_text())
    limit = config.get("budget", {}).get("hard_limit")
    if config.get("budget", {}).get("unit") != "Rows" or not isinstance(limit, int) or not 1 <= limit <= 1_000_000:
        raise ValueError("source row budget")
    command = ([reader, "capture-partition-budget", str(output), args.reader_secrets,
                args.source_host, "9030", str(incoming["period_ms"]), label,
                "cycle" + str(pin), str(limit)] if c4 else
               [reader, "capture-tick-budget", str(output), args.reader_secrets,
                args.source_host, "9030", str(incoming["period_ms"]), label, str(limit)])
    execute(command, output, "capture")
    tick_path = output / f"{label}.json"
    tick = json.loads(tick_path.read_text())
    table = next(t for t in tick["tables"] if t["name"] == args.table)
    route = table["route"]
    if route != incoming["method"] or tick["product_reader_write_attempts"] != 0:
        raise ValueError("captured source contract")
    arrow = output / table["arrow"]
    capturer_path = tick_path
    if c4:
        scope = dict(tick["partition_scope"])
        scope.update({"source_id": "dorispreview", "source_generation": generation,
                      "logical_schema": "oo_dl01w1_43", "logical_table": args.table,
                      "schema_digest": digest(args.schema_preimage),
                      "type_digest": digest(args.type_preimage),
                      "arrow_sha256": digest(arrow)})
        capturer = dict(tick)
        capturer.update({"source_id": "dorispreview", "source_generation": generation,
                         "logical_schema": "oo_dl01w1_43", "logical_table": args.table,
                         "partition_scope": scope, "reader_capture_sha256": digest(tick_path)})
        capturer_path = output / "capturer.json"
        document(capturer_path, capturer)
    witness = {"protocol": "oo.source-admission.capture-witness/v1",
               "source_engine": "doris", "source_id": "dorispreview",
               "source_generation": generation,
               "snapshot": {"kind": "capture-tick", "value": label},
               "run_id": "dorispreview-" + label + "-" + str(tick["finished_unix_ms"]),
               "logical_schema": "oo_dl01w1_43", "logical_table": args.table,
               "capturer_path": str(capturer_path), "arrow_path": str(arrow),
               "driver_path": reader, "schema_preimage_path": args.schema_preimage,
               "type_preimage_path": args.type_preimage}
    witness_path = output / "witness.json"
    document(witness_path, witness)
    report = {"protocol": "oo.source-admission.report/v1", "target_id": "dorispreview",
              "source_id": "dorispreview", "secret_ref": config["secret_ref"],
              "driver": {"kind": "native", "name": "doris-reader", "version": "0.1.0",
                         "digest": PARTITION_READER_SHA if c4 else READER_SHA},
              "compat_mode": "doris", "license_flags": ["Apache-2.0"],
              "permissions": config["permissions"],
              "capabilities": [{"name": "BatchFull", "cursor": None, "feed": None,
                                "cdc": None, "token_order": None, "budget_unit": None,
                                "level": "experimental", "write_class": "R0", "consistency": "S0"}],
              "capture_methods": [], "r1w": None,
              "cost_model": {"unit": "Rows", "confidence": "unknown",
                             "estimate": "Doris billing is unpublished; configured observed reader row budget"},
              "capture_witness": str(witness_path)}
    report_path = output / "report.json"
    document(report_path, report)
    seal = output / "sealed"
    execute([args.validator, "seal-native", "--report", str(report_path),
             "--config", args.config, "--out", str(seal)], output, "seal")
    receipt = json.loads((seal / "admission-receipt.json").read_text())
    # Publish the sealer's exact bytes at the operator registration template.
    # This is a byte-for-byte copy, never a producer-authored registry.
    registered_registry = pin_dir / "registry.json"
    durable_bytes(registered_registry, (seal / "registry.json").read_bytes())
    spec = TABLES[args.table]
    request = {"protocol": "oo.pipeline.native-partition/v1" if c4 else "oo.pipeline.native-update/v1", "source_engine": "doris",
               "source_id": "dorispreview", "source_generation": generation,
               "parent_source_generation": generation - 1, "snapshot": witness["snapshot"],
               "logical_schema": "oo_dl01w1_43", "logical_table": args.table,
               "batch_id": label + "-" + args.table,
               "capture_method": "c4_partition" if route == "C4" else "c5_diff",
               "delete_detection": table["delete_mode"], "consistency_grade": table["consistency"],
               "cross_table_consistent": False, "full_snapshot": not c4,
               "row_identity": spec["identity"], "key_columns": ["day"] if c4 else spec["keys"],
               "position_codec": "partition-signature/v1" if c4 else "snapshot-zero/v1",
               "position_lineage": "doris.partition." + args.table if c4 else "doris.snapshot." + args.table,
               "applied_pos": "0", "schema_digest": receipt["schema_digest"],
               "type_digest": receipt["type_digest"], "config_digest": receipt["config_digest"],
               "input_digest": digest(arrow), "audit_digest": digest(tick_path),
               "arrow_path": str(arrow), "admission_path": str(seal / "admission-receipt.json"),
               "registry_path": str(registered_registry),
               "columns": [column_decl(c) for c in spec["columns"]], "tombstones": [],
               "target": {"shard": "dorispreview", "owner": "5106fed0-5bcd-4aaf-8df7-838c95a7d7e2",
                          "host": args.target_host, "port": 5432, "database": "dorisnative",
                          "user": "oo_doris_loader", "password_file": args.target_secret,
                          "ca_file": args.target_ca, "ssl": "verify-full"}}
    document(output / "request.json", request)
    saved = (json.dumps(request, separators=(",", ":")) + "\n").encode()
    meta = {"identity": identity, "request_sha256": hashlib.sha256(saved).hexdigest()}
    # Metadata first, then successful request. A crash between these writes
    # has no successful marker and can safely retain another failed attempt.
    for path, data in ((metadata, (json.dumps(meta) + "\n").encode()), (successful, saved)):
        durable_bytes(path, data)
    sys.stdout.buffer.write(saved)


if __name__ == "__main__":
    try:
        main()
    except Exception:
        print("Doris native producer refused", file=sys.stderr)
        raise SystemExit(1)
