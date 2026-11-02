#!/usr/bin/env python3
"""Real jobs claim/heartbeat and Oracle identity/opaque-open adapter test.

Does not touch the Oracle server or claim CDC/publication acceptance. Uses
Oracle owner's existing files read-only and a fresh DB on the owned C1 PG.
"""
from pathlib import Path
import hashlib
import json
import os
import subprocess
import time
import uuid

ROOT = Path(__file__).resolve().parents[5]
EV = Path(os.environ["OO_C1_INTEGRATION_EVIDENCE"])
HOME = Path("/tmp/oo-mysql-c1-20261027")
PG = "oo-mysql-c1-20261027-pg17"
STAMP = str(time.time_ns())
OUT = EV / ("oracle-registry-" + STAMP)
OUT.mkdir()
DB = "c1oracle_" + STAMP
OLD = HOME / "formal-1793051270229661000"
RUNTIME = "c1rt_1793051270229661000"
BIN = Path(os.environ.get("OO_C1_LOADER", ROOT / "product/target/debug/oo-pipeline"))


def command(label, argv, data=None, success=True):
    p = subprocess.run(list(map(str, argv)), input=data, capture_output=True, cwd=ROOT)
    (OUT / (label + ".stdout")).write_bytes(p.stdout)
    (OUT / (label + ".stderr")).write_bytes(p.stderr)
    (OUT / (label + ".command.json")).write_text(json.dumps(dict(argv=list(map(str, argv)), exit=p.returncode)) + "\n")
    if success:
        assert p.returncode == 0, label
    return p


def pg(sql, db=DB, role=False):
    if role:
        # Login as the actual non-superuser role, with existing private secret.
        env = os.environ.copy()
        env["PGPASSWORD"] = (OLD / "runtime.pw").read_text().strip()
        p = subprocess.run(["docker", "exec", "-e", "PGPASSWORD", "-i", PG, "psql",
                            "-X", "-h", "127.0.0.1", "-U", RUNTIME, "-d", db, "-At",
                            "-v", "ON_ERROR_STOP=1"], input=sql.encode(), env=env, capture_output=True)
        assert p.returncode == 0, p.stderr.decode()
        return p.stdout.decode().strip()
    return command("pg-" + str(time.time_ns()), ["docker", "exec", "-i", PG, "psql", "-X",
                   "-U", "postgres", "-d", db, "-At", "-v", "ON_ERROR_STOP=1"], sql.encode()).stdout.decode().strip()


def main():
    strict_index = os.environ.get("OO_ORACLE_BUFFER_CONTEXT")
    buffer_index = None
    if strict_index:
        index_path = Path(strict_index)
        index_raw = index_path.read_bytes()
        buffer_index = json.loads(index_raw)
        assert hashlib.sha256(BIN.read_bytes()).hexdigest() == BIN.parent.name
        # Source-owner validation independently checks event IDs/END against the
        # frozen SQLite spool. No source connection or source directory writes.
        command("source-buffer-verify", ["env", "PYTHONDONTWRITEBYTECODE=1", "python3",
            ROOT / "product/tools/oracle-preview/buffer_cursor.py", "verify",
            "--index", index_path, "--sha256", hashlib.sha256(index_raw).hexdigest()])
        context_ref = buffer_index["context"]
        raw = Path(context_ref["path"]).read_bytes()
        assert len(raw) == context_ref["bytes"] and hashlib.sha256(raw).hexdigest() == context_ref["sha256"]
        buffer_context = json.loads(raw)
        payloads = {}
        for ref in buffer_index["transactions"]:
            raw = Path(ref["path"]).read_bytes()
            assert len(raw) == ref["bytes"] and hashlib.sha256(raw).hexdigest() == ref["sha256"]
            payload = json.loads(raw)
            assert payload["buffer_context"] == context_ref
            payloads[payload["batch"]["sequence"]] = payload
        (OUT / "buffer-input-pins.json").write_text(json.dumps(dict(
            index_path=str(index_path), index_sha256=hashlib.sha256(index_raw).hexdigest(),
            context=context_ref, loader_sha256=BIN.parent.name), indent=2) + "\n")
    pg(f"CREATE DATABASE {DB};", "postgres")
    command("jobs", ["python3", ROOT / "product/crates/oo-jobs/tools/deploy.py",
                     "--container", PG, "--database", DB, "--loader-role", RUNTIME])
    admin = dict(host="127.0.0.1", port=25487, database=DB, runtime_principal="postgres",
                 password_file=str(HOME / "secrets/pg-super.pw"), ca_file=str(HOME / "certs/ca.pem"))
    for name in ["install-native-update", "install-native-mirror-rebind",
                 "install-native-claim-selected", "install-native-c1-typed"]:
        command(name, [BIN, name], json.dumps(dict(protocol="oo.pipeline.install-native-update/v1", target=admin)).encode())
    target = dict(admin, runtime_principal=RUNTIME, password_file=str(OLD / "runtime.pw"),
                  shard="oracle_registry_" + STAMP, owner=str(uuid.uuid4()))
    project = str(uuid.uuid4())
    task = pg(f"SELECT task_id FROM jobs.enqueue('{project}','native_capture',1,'{{}}','oracle-registry-v1',"
              f"'{hashlib.sha256(BIN.read_bytes()).hexdigest()}','oracle-registry',clock_timestamp(),3);", role=True)
    claim = pg(f"SELECT row_to_json(x) FROM jobs.claim_selected('{project}','{task}',ARRAY['native_capture'],'oracle-adapter') x;", role=True)
    assert json.loads(claim)["status"] == "claimed"
    heartbeat = pg(f"SELECT status FROM jobs.heartbeat('{project}','{task}','oracle-adapter',1)", role=True)
    assert heartbeat == "extended"
    oracle = EV.parent / "oracle"
    manifest = json.loads((oracle / "real-key-budget-r3/baseline/manifest.json").read_text())
    buffer_mode = os.environ.get("OO_ORACLE_BUFFER_TEST") == "1" or bool(strict_index)
    bundle_path = Path(buffer_context["bundle"]["path"]) if strict_index else EV.parent / "oracle-integration/ready/c1/manifest.json"
    bundle_raw = bundle_path.read_bytes() if buffer_mode else b""
    bundle = json.loads(bundle_raw) if buffer_mode else None
    if buffer_mode:
        intake = buffer_context if strict_index else json.loads((EV.parent / "oracle-integration/ready/ROOT-INPUT.json").read_text())
        assert hashlib.sha256(bundle_raw).hexdigest() == intake["bundle"]["sha256"]
        batch_bytes = (bundle_path.parent / "batches.jsonl").read_bytes()
        assert hashlib.sha256(batch_bytes).hexdigest() == bundle["artifacts"]["batches.jsonl"]
        batches = [json.loads(line)["batch"] for line in batch_bytes.splitlines()]
        assert len(batches) == bundle["batch_count"]
    values = (bundle["boundary"]["identity"] if buffer_mode else manifest["binding"]["identity"]).split("|")
    identity = dict(engine="oracle", dbid=values[0], resetlogs_change=values[1],
                    pdb_guid=values[2], pdb_name=values[3], db_unique_name=values[4])
    src = bundle["source_id"] if buffer_mode else "oracle_registry_" + STAMP
    config_digest = bundle["config_sha256"] if buffer_mode else hashlib.sha256(json.dumps(identity, sort_keys=True).encode()).hexdigest()
    schema_digest = hashlib.sha256(json.dumps(bundle["typed_tables"] if buffer_mode else manifest["binding"]["inventory"], sort_keys=True).encode()).hexdigest()
    if strict_index:
        schema_digest = buffer_context["typed_schema_sha256"]
    request = dict(protocol="oo.pipeline.native-c1-typed-stage/v3", operation="register",
                   source_id=src, lease_id=task, lease_generation=1, target=target,
                   source_identity=identity, config_digest=config_digest, schema_digest=schema_digest)
    p = command("register", [BIN, "native-c1-typed-stage"], json.dumps(request).encode())
    registered = json.loads(p.stdout)
    assert registered["engine"] == "oracle" and registered["measured_id"].startswith("oracle.")
    replay = command("register-replay", [BIN, "native-c1-typed-stage"], json.dumps(request).encode())
    assert json.loads(replay.stdout)["source_epoch"] == registered["source_epoch"]
    offset = oracle / "c1-r2/offsets/offsets.dat"
    blob = offset.read_bytes()
    offset_ref = dict(path=str(offset), bytes=len(blob), sha256=hashlib.sha256(blob).hexdigest())
    # Consume the Oracle owner's exact frozen native snapshot, not a mock.
    # Its relationship to the final opaque offset is NOT a proven CDC handoff.
    snapshot_dir = oracle / "c1-r2/independent-final-binary"
    snapshot = json.loads((snapshot_dir / "manifest.json").read_text())
    encoder = oracle / "bin/9f09da894ccce6ac9d36d4ba241f7fa9cd09625a0274490669e568e6ef582339/oracle-chunk-reader"
    assert hashlib.sha256(encoder.read_bytes()).hexdigest() == encoder.parent.name
    columns = [
        {"name": "ID", "oid": 20, "typmod": -1, "nullable": False},
        {"name": "AMOUNT", "oid": 1700, "typmod": (38 << 16) + 9 + 4, "nullable": True},
        {"name": "LABEL", "oid": 25, "typmod": -1, "nullable": True},
        {"name": "PAYLOAD", "oid": 17, "typmod": -1, "nullable": True},
    ]
    spec = {"columns": columns, "keys": [0]}
    opened = dict(protocol=request["protocol"], operation="open", source_engine="oracle",
                  source_id=src, lease_id=task, lease_generation=1, source_epoch=registered["source_epoch"],
                  offset_before=offset_ref, target=target, typed_tables={"OO_CONV_OWNER.C1_EVENTS": spec})
    if buffer_mode:
        cursor = dict(protocol="oo.c1.oracle-buffer-cursor/v1",
                      bundle=dict(path=str(bundle_path), sha256=hashlib.sha256(bundle_raw).hexdigest(), bytes=len(bundle_raw)))
        opened.pop("offset_before")
        opened.update(source_position=cursor, first_batch=batches[0], typed_tables=bundle["typed_tables"])
        if strict_index:
            opened.pop("source_position")
            opened.update(buffer_context=context_ref, checkpoint_kind="buffer_consumption")
    p = command("open-oracle", [BIN, "native-c1-typed-stage"], json.dumps(opened).encode())
    assert json.loads(p.stdout)["outcome"] == "open"
    bad = json.loads(json.dumps(opened))
    if strict_index:
        bad["buffer_context"]["sha256"] = "0" * 64
    elif buffer_mode:
        bad["source_position"]["bundle"]["sha256"] = "0" * 64
    else:
        bad["offset_before"]["sha256"] = "0" * 64
    assert command("tampered-offset", [BIN, "native-c1-typed-stage"], json.dumps(bad).encode(), False).returncode
    bad = dict(request, lease_generation=2)
    assert command("stale-generation", [BIN, "native-c1-typed-stage"], json.dumps(bad).encode(), False).returncode
    # A legacy MySQL open cannot use the registered Oracle engine/epoch.
    bad = dict(opened, source_engine="mysql", measured_id="mysql." + str(uuid.uuid4()),
               config_digest=config_digest, schema_digest=schema_digest, l0="not-a-mysql-gtid", l0_pos="0")
    assert command("no-mysql-impersonation", [BIN, "native-c1-typed-stage"], json.dumps(bad).encode(), False).returncode
    actual = json.loads(pg(f"SELECT jsonb_build_object('engine',i.engine,'measured_id',i.measured_id,"
                           f"'run_engine',r.engine,'native_baseline_applied_pos',r.applied_pos::text,'opaque_start_sha256',r.l0)"
                           f" FROM _oo.source_instance i JOIN _oo.native_c1_baseline_run r USING(source)"
                           f" WHERE i.source='{src}'"))
    assert actual["engine"] == actual["run_engine"] == "oracle"
    assert actual["native_baseline_applied_pos"] == "0"
    assert actual["opaque_start_sha256"] == (hashlib.sha256(bundle_raw).hexdigest() if buffer_mode else offset_ref["sha256"])
    if buffer_mode:
        actual["buffer_start_sha256"] = actual.pop("opaque_start_sha256")
    expected = []
    sequence = 0
    for part in snapshot["parts"] if not buffer_mode else []:
        path = snapshot_dir / part["file"]
        raw = path.read_bytes()
        assert hashlib.sha256(raw).hexdigest() == part["sha256"]
        doc = json.loads(raw)
        expected.extend(doc["rows"])
        sequence += 1
        arrow = OUT / f"part-{sequence}.arrow"
        p = command(f"encode-{sequence}", [encoder], json.dumps(dict(
            action="encode-part", part=str(path), sha256=part["sha256"], output=str(arrow))).encode())
        encoded = json.loads(p.stdout)
        assert encoded["rows"] == part["rows"]
        heartbeat = pg(f"SELECT status FROM jobs.heartbeat('{project}','{task}','oracle-adapter',1)", role=True)
        assert heartbeat == "extended"
        batch = dict(protocol="oo.c1.typed-batch/v3", sequence=sequence,
                     previous_position="0", position="0", kind="fill",
                     parts=[dict(table="OO_CONV_OWNER.C1_EVENTS", spec=spec, path=str(arrow),
                                 sha256=encoded["arrow_sha256"], bytes=encoded["bytes"], rows=part["rows"])])
        apply = dict(protocol=request["protocol"], operation="apply", source_engine="oracle",
                     source_id=src, lease_id=task, lease_generation=1, source_epoch=registered["source_epoch"],
                     target=target, batch=batch)
        p = command(f"apply-{sequence}", [BIN, "native-c1-typed-stage"], json.dumps(apply).encode())
        receipt = json.loads(p.stdout)
        acknowledged = pg(f"SELECT _oo.c1_typed_ack('{src}','{task}',1,1,{sequence},'{receipt['digest']}')", role=True)
        assert acknowledged == "t"
    if buffer_mode:
        for batch in batches:
            sequence = batch["sequence"]
            heartbeat = pg(f"SELECT status FROM jobs.heartbeat('{project}','{task}','oracle-adapter',1)", role=True)
            assert heartbeat == "extended"
            apply = dict(protocol=request["protocol"], operation="apply", source_engine="oracle",
                         source_id=src, lease_id=task, lease_generation=1, source_epoch=registered["source_epoch"],
                         target=target, batch=batch, source_position=cursor)
            if strict_index:
                apply.pop("source_position")
                apply["buffer_context"] = context_ref
                if batch["kind"] == "cdc":
                    assert payloads[sequence]["batch"] == batch
                    apply["source_position"] = payloads[sequence]["source_position"]
                    if batch["position"] == "1":
                        before_receipts = pg(f"SELECT count(*) FROM _oo.c1_typed_receipt WHERE source='{src}'")
                        for key, value in [("end_event_count", 1), ("context_sha256", "0" * 64),
                                           ("protocol", "oo.c1.oracle-logminer-position/v1"),
                                           ("source_offset_write", True)]:
                            bad = json.loads(json.dumps(apply))
                            bad["source_position"][key] = value
                            assert command("reject-" + key, [BIN, "native-c1-typed-stage"],
                                           json.dumps(bad).encode(), False).returncode
                        bad = dict(apply, source_position=cursor)
                        bad.pop("buffer_context")
                        assert command("reject-context-downgrade", [BIN, "native-c1-typed-stage"],
                                       json.dumps(bad).encode(), False).returncode
                        assert pg(f"SELECT count(*) FROM _oo.c1_typed_receipt WHERE source='{src}'") == before_receipts
            p = command(f"buffer-apply-{sequence}", [BIN, "native-c1-typed-stage"], json.dumps(apply).encode())
            receipt = json.loads(p.stdout)
            if batch["kind"] == "cdc":
                replay = command(f"buffer-replay-{sequence}", [BIN, "native-c1-typed-stage"], json.dumps(apply).encode())
                assert json.loads(replay.stdout)["replay"] is True
            if strict_index and batch["position"] == bundle["boundary"]["final_position"]:
                # Exercise the real worker function, not an invented ACK frame.
                receipt_path = OUT / "worker-receipt.json"
                receipt_path.write_text(json.dumps(receipt) + "\n")
                command("worker-buffer-ack", ["env", "OO_ORACLE_ACK_RECEIPT=" + str(receipt_path),
                    "OO_ORACLE_ACK_DATABASE=" + DB, "OO_ORACLE_ACK_ROLE=" + RUNTIME,
                    "OO_ORACLE_ACK_PASSWORD_FILE=" + str(OLD / "runtime.pw"),
                    "cargo", "test", "--locked", "--manifest-path", ROOT / "product/tools/job-worker/Cargo.toml",
                    "--test", "c1_typed_ack_live",
                    "oracle_buffer_checkpoint_cas_precedes_ack", "--", "--ignored", "--exact"])
            else:
                assert pg(f"SELECT _oo.c1_typed_ack('{src}','{task}',1,1,{sequence},'{receipt['digest']}')", role=True) == "t"
            if batch["kind"] == "cdc":
                checkpoint = json.loads(pg(f"SELECT _oo.c1_typed_checkpoint_evidence('{src}','{task}',1,1,{sequence},'{receipt['digest']}')", role=True))
                assert checkpoint["checkpoint_kind"] == "buffer_consumption"
                assert checkpoint["source_offset_advanced"] is False
                assert checkpoint["apply_ordinal"] == batch["position"]
                if strict_index:
                    assert checkpoint["context_sha256"] == context_ref["sha256"]
                    assert checkpoint["source_offset_write"] is False
                    assert checkpoint["buffer_position"]["cursor_after"] == payloads[sequence]["source_position"]["cursor_after"]
                    assert checkpoint["source_evidence_sha256"] == hashlib.sha256(json.dumps(
                        payloads[sequence]["source_position"], sort_keys=True, separators=(",", ":"),
                        ensure_ascii=False).encode()).hexdigest()
                (OUT / f"buffer-checkpoint-{sequence}.json").write_text(json.dumps(checkpoint, indent=2) + "\n")
        for part in snapshot["parts"]:
            raw = (snapshot_dir / part["file"]).read_bytes()
            assert hashlib.sha256(raw).hexdigest() == part["sha256"]
            expected.extend(json.loads(raw)["rows"])
    physical = pg(f"SELECT physical FROM _oo.c1_typed_table WHERE source='{src}' AND task='{task}'")
    rows = pg(f"SELECT json_build_array(c0::text,c1::text,c2::text,encode(c3,'hex')) FROM _oo.{physical}").splitlines()
    target_rows = sorted([json.loads(row) for row in rows], key=lambda row: int(row[0]))
    expected = sorted(expected, key=lambda row: int(row[0]))
    assert target_rows == expected and len(expected) == snapshot["rows"] == 10001
    final_ordinal = pg("SELECT r.position::text FROM _oo.c1_typed_checkpoint c "
                       "JOIN _oo.c1_typed_receipt r USING(source,task,seq)")
    assert final_ordinal == (batches[-1]["position"] if buffer_mode else "0")
    for name, rows in [("frozen-native-source-rows", expected), ("target-rows", target_rows)]:
        (OUT / (name + ".json")).write_text(json.dumps(rows, separators=(",", ":")))
    # Missing checkpoint blocks CDC before COPY/receipt, not a made-up SCN+1.
    missing = dict(apply, batch=dict(protocol="oo.c1.typed-batch/v3", sequence=sequence + 1,
                  previous_position="0", position="1", kind="cdc", parts=[]))
    missing.pop("source_position", None)
    assert command("missing-opaque-cdc", [BIN, "native-c1-typed-stage"], json.dumps(missing).encode(), False).returncode
    # Explicitly fail the adapter-test job via the existing API, not success.
    pg(f"SELECT status FROM jobs.complete_failure('{project}','{task}','oracle-adapter',1,'payload_rejected')", role=True)
    if strict_index:
        for key in ["final_connector_offset", "connector_history"]:
            ref = buffer_context["source_resume"][key]
            raw = Path(ref["path"]).read_bytes()
            assert len(raw) == ref["bytes"] and hashlib.sha256(raw).hexdigest() == ref["sha256"]
    result = dict(real_jobs_claim=True, real_heartbeat=True, real_source_registry=True,
                  identity=identity, target_at_open=actual, database=DB, task=task, offset_read_only=True,
                  oracle_server_contacted=False, cdc_applied=buffer_mode, source_ack=False,
                  publication=False, checkpoint_before_after_pair="not_required_for_buffer_consumption" if buffer_mode else "not_yet_supplied",
                  baseline_rows=sum(p["rows"] for b in batches for p in b["parts"] if b["kind"] == "fill") if buffer_mode else 10001,
                  final_target_rows=10001, typed_differences=0,
                  frozen_source_manifest_sha256=hashlib.sha256((snapshot_dir / "manifest.json").read_bytes()).hexdigest(),
                  source_rows_sha256=hashlib.sha256((OUT / "frozen-native-source-rows.json").read_bytes()).hexdigest(),
                  target_rows_sha256=hashlib.sha256((OUT / "target-rows.json").read_bytes()).hexdigest(),
                  source_capture_restart_verified=False, buffer_baseline_to_cdc_verified=buffer_mode,
                  final_buffer_apply_ordinal=final_ordinal,
                  native_admission=False,
                  frozen_encoder_sha256=encoder.parent.name,
                  buffer_consumption=buffer_mode, source_checkpoint_advanced=False,
                  real_log_events_applied=10004 if buffer_mode else 0,
                  real_log_transactions_applied=4 if buffer_mode else 0,
                  raw_log_origin="frozen real Debezium Oracle LogMiner capture" if buffer_mode else None,
                  bundle_sha256=hashlib.sha256(bundle_raw).hexdigest() if buffer_mode else None,
                  intermediate_vendor_offsets="not required or invented; replay uses immutable buffer cursor",
                  strict_buffer_context=bool(strict_index),
                  real_worker_buffer_ack=bool(strict_index),
                  context_sha256=context_ref["sha256"] if strict_index else None,
                  loader_sha256=hashlib.sha256(BIN.read_bytes()).hexdigest())
    (OUT / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    print(OUT)


if __name__ == "__main__":
    main()
