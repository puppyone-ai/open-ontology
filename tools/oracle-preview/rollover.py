#!/usr/bin/env python3
"""Oracle successive buffer lineage. No source/target connections or offset writes."""
import argparse
import contextlib
import hashlib
from itertools import zip_longest
import json
from pathlib import Path
import shutil
import sqlite3
import sys
import tempfile

import bounded_source as b
import buffer_cursor as c
import producer as p

PROTOCOL = "oo.oracle.buffer-rollover/v1"
PREVIOUS = "oo.jobs.oracle-buffer-previous/v1"
RESUME = "oo.jobs.oracle-buffer-resume/v1"
FILL_RESUME = "oo.jobs.oracle-fill-resume/v1"

def document(ref):
    return json.loads(c.file_ref(ref).read_text())

def input_state(ref):
    path = c.file_ref(ref)
    c.verify(path, ref["sha256"])
    index = document(ref)
    context = document(index["context"])
    manifest = p.validate_bundle(c.file_ref(context["bundle"]).parent, context["bundle"]["sha256"])
    return index, context, manifest

def source_prefix(parent_spool, successor_spool):
    """Compare exact original snapshot/END/event bytes, not SCN arithmetic."""
    # Capture owns the disk quota; lineage hashes the full file without loading it.
    paths = [c.file_ref(ref, None) for ref in (parent_spool, successor_spool)]
    if any(Path(str(path)+suffix).exists() for path in paths for suffix in ("-wal", "-shm")):
        raise ValueError("rollover_live_spool")
    with contextlib.ExitStack() as stack:
        dbs = [stack.enter_context(contextlib.closing(
            sqlite3.connect(f"file:{path}?mode=ro&immutable=1", uri=True))) for path in paths]
        for db in dbs:
            db.execute("PRAGMA cache_size=-4096")
        snapshot_hash, count = hashlib.sha256(), 0
        for left, right in zip_longest(*(db.execute("SELECT seq,body FROM snapshots ORDER BY seq") for db in dbs)):
            if left != right or left is None:
                raise ValueError("rollover_snapshot_changed")
            snapshot_hash.update(left[1]+b"\n")
            count += 1
        if count == 0:
            raise ValueError("rollover_snapshot_missing")
        ends = [db.execute("SELECT tx,expected,body FROM boundaries ORDER BY rowid") for db in dbs]
        prefix_hash, events, parent_count, child_count = hashlib.sha256(), 0, 0, 0
        for parent, child in zip_longest(*ends):
            child_count += child is not None
            if parent is None:
                continue
            if parent != child:
                raise ValueError("rollover_transaction_prefix")
            parent_count += 1
            tx, _, end = parent
            prefix_hash.update(end+b"\n")
            queries = [db.execute("SELECT ord,body FROM events WHERE tx=? ORDER BY ord", (tx,)) for db in dbs]
            for left, right in zip_longest(*queries):
                if left != right or left is None:
                    raise ValueError("rollover_event_prefix")
                prefix_hash.update(left[1]+b"\n")
                events += 1
        schemas = [db.execute("SELECT table_id,digest FROM schemas ORDER BY table_id") for db in dbs]
        if any(left != right for left, right in zip_longest(*schemas)):
            raise ValueError("rollover_wire_schema")
    return {"snapshot_rows": count, "snapshot_bytes_sha256": snapshot_hash.hexdigest(),
            "parent_transactions": parent_count, "successor_transactions": child_count,
            "parent_events": events, "parent_transaction_bytes_sha256": prefix_hash.hexdigest()}

def check_journal(context, manifest, expected_table):
    """Independent disk-backed replay checks the exporter journal at its final cut."""
    root = Path(context["bundle"]["path"]).parent
    columns = [m["source_column"] for m in manifest["source_mappings"][expected_table]]
    owner, name = expected_table.split(".", 1)
    names = [column["name"] for column in columns]
    indices = manifest["typed_tables"][expected_table]["keys"]
    table = {"owner": owner, "table": name, "columns": columns, "keys": [names[i] for i in indices]}
    with tempfile.TemporaryDirectory(prefix="oo-oracle-lineage-") as temp, contextlib.ExitStack() as stack:
        state = stack.enter_context(contextlib.closing(sqlite3.connect(str(Path(temp)/"state.sqlite"))))
        state.execute("CREATE TABLE rows(key TEXT PRIMARY KEY,row_json TEXT NOT NULL)")
        raw = stack.enter_context(contextlib.closing(sqlite3.connect(
            f"file:{context['raw_spool']['path']}?mode=ro&immutable=1", uri=True)))
        count = 0
        with state:
            for body, in raw.execute("SELECT body FROM snapshots ORDER BY seq"):
                record = json.loads(body)
                payload = record.get("payload", record)
                if p.event_table(payload["source"], manifest["typed_tables"]) != expected_table:
                    continue
                p.source_matches(payload["source"], table)
                row = p.decode_row(payload["after"], table, p.wire_row(record["schema"], "after"))
                key = b.canonical([row[i] for i in indices]).decode()
                state.execute("INSERT INTO rows VALUES (?,?)", (key, b.canonical(row).decode()))
                count += 1
            for tx, in raw.execute("SELECT tx FROM boundaries ORDER BY rowid"):
                for body, in raw.execute("SELECT body FROM events WHERE tx=? ORDER BY ord", (tx,)):
                    event = json.loads(body)
                    if p.event_table(event["source"], manifest["typed_tables"]) != expected_table:
                        continue
                    p.source_matches(event["source"], table)
                    before = p.decode_row(event["before"], table, p.wire_row(event["schema"], "before")) if event["before"] is not None else None
                    after = p.decode_row(event["after"], table, p.wire_row(event["schema"], "after")) if event["after"] is not None else None
                    op = event["op"]
                    if op not in ("c", "u", "d") or (op != "c" and before is None) or (op != "d" and after is None):
                        raise ValueError("rollover_journal_event")
                    row = before if op == "d" else after
                    key = b.canonical([row[i] for i in indices]).decode()
                    prior = state.execute("SELECT row_json FROM rows WHERE key=?", (key,)).fetchone()
                    if (op == "c" and prior is not None) or (op != "c" and
                            (prior is None or json.loads(prior[0]) != before)):
                        raise ValueError("rollover_journal_before")
                    if prior is not None:
                        state.execute("DELETE FROM rows WHERE key=?", (key,))
                        count -= 1
                    if after is not None:
                        state.execute("INSERT INTO rows VALUES (?,?)", (key, b.canonical(after).decode()))
                        count += 1
        journal = stack.enter_context(contextlib.closing(sqlite3.connect(
            f"file:{root/'producer-journal.sqlite'}?mode=ro&immutable=1", uri=True)))
        if not {row[0] for row in journal.execute("SELECT DISTINCT table_id FROM rows")} <= set(manifest["typed_tables"]):
            raise ValueError("rollover_journal_scope")
        for actual, expected in zip_longest(state.execute("SELECT key,row_json FROM rows ORDER BY key"),
                journal.execute("SELECT key,row_json FROM rows WHERE table_id=? ORDER BY key", (expected_table,))):
            if actual != expected:
                raise ValueError("rollover_journal_difference")
    return count

def prove(parent_ref, successor_ref):
    parent, pc, pm = input_state(parent_ref)
    child, cc, cm = input_state(successor_ref)
    if "rollover" in cc:
        raise ValueError("rollover_generation_scope")
    if "rollover" in pc:
        # A published delta still carries the full capture journal and checkpoints.
        descriptor = document(pc["rollover"])
        captured, captured_context, captured_manifest = input_state(descriptor["successor_capture_index"])
        if (parent["final_cursor"] != captured["final_cursor"]
                or pm.get("rollover") != pc["rollover"]
                or pm.get("capture_bundle") != captured_context["bundle"]
                or any(pc[k] != captured_context[k] for k in
                       ("source_id", "source_identity", "raw_spool", "source_resume", "typed_schema_sha256"))
                or pm["checkpoints"] != captured_manifest["checkpoints"]
                or pm["artifacts"]["producer-journal.sqlite"] !=
                   captured_manifest["artifacts"]["producer-journal.sqlite"]):
            raise ValueError("rollover_parent_capture")
    elif pc["initial_cursor"] != c.cursor(0, None):
        raise ValueError("rollover_parent_required")
    if (cc["initial_cursor"] != c.cursor(0, None)
            or any(pc[k] != cc[k] for k in ("source_id", "source_identity", "typed_schema_sha256", "snapshot_scn"))
            or any(pm[k] != cm[k] for k in ("source_id", "typed_tables", "source_mappings"))
            or pm["boundary"]["identity"] != cm["boundary"]["identity"]
            or not pm["typed_tables"]):
        raise ValueError("rollover_source_schema")
    # DDL is outside this phase; do not reinterpret changed schema history.
    if pc["source_resume"]["connector_history"]["sha256"] != cc["source_resume"]["connector_history"]["sha256"]:
        raise ValueError("rollover_history_drift")
    proof = source_prefix(pc["raw_spool"], cc["raw_spool"])
    if (proof["parent_transactions"] != c.ordinal(parent["final_cursor"])
            or proof["successor_transactions"] != c.ordinal(child["final_cursor"])
            or cm["checkpoints"][:len(pm["checkpoints"])] != pm["checkpoints"]):
        raise ValueError("rollover_prefix_cut")
    proof["parent_rows"] = sum(check_journal(pc, pm, table) for table in sorted(pm["typed_tables"]))
    proof["successor_rows"] = sum(check_journal(cc, cm, table) for table in sorted(cm["typed_tables"]))
    return proof

def parent_authority(parent_ref, ack_ref):
    index, context, manifest = input_state(parent_ref)
    ack = document(ack_ref)
    receipt, instance = ack.get("receipt", {}), ack.get("receipt", {}).get("source_instance", {})
    position = receipt.get("oracle_buffer", {})
    expected = {"checkpoint_kind": "buffer_consumption", "context_sha256": index["context"]["sha256"],
                "bundle_sha256": context["bundle"]["sha256"], "spool_sha256": context["raw_spool"]["sha256"],
                "typed_schema_sha256": context["typed_schema_sha256"], "source_identity": context["source_identity"],
                "final_cursor": index["final_cursor"], "source_offset_write": False,
                "connector_offsets_sha256": context["source_resume"]["final_connector_offset"]["sha256"],
                "connector_history_sha256": context["source_resume"]["connector_history"]["sha256"]}
    end = f"oracle-buffer:{index['context']['sha256']}:{index['final_cursor']['transaction_ordinal']}"
    if (ack.get("protocol") != "oo.oracle.c1-ack-state/v1" or ack.get("source_engine") != "oracle"
            or ack.get("acknowledged") is not True or ack.get("task_state") != "succeeded"
            or ack.get("source_id") != context["source_id"] or receipt.get("source_id") != context["source_id"]
            or type(ack.get("source_generation")) is not int or not 1 <= ack["source_generation"] < 2**63-1
            or receipt.get("source_generation") != ack["source_generation"]
            or type(ack.get("source_epoch")) is not int or ack["source_epoch"] < 1
            or receipt.get("source_epoch") != ack["source_epoch"]
            or type(ack.get("target_generation")) is not int or ack["target_generation"] < 1
            or receipt.get("target_generation") != ack["target_generation"]
            or not isinstance(ack.get("request_digest"), str) or len(ack["request_digest"]) != 64
            or receipt.get("request_digest") != ack["request_digest"]
            or ack.get("applied_pos") != index["final_cursor"]["transaction_ordinal"]
            or receipt.get("l_end") != end
            or any(position.get(k) != v for k, v in expected.items())
            or instance.get("engine") != "oracle" or instance.get("source") != context["source_id"]
            or instance.get("source_epoch") != ack["source_epoch"]
            or instance.get("schema_digest") != context["typed_schema_sha256"]
            or not instance.get("lease_id")):
        raise ValueError("rollover_parent_ack")
    measured = "oracle.{}.{}.{}".format(context["source_identity"]["dbid"],
        context["source_identity"]["resetlogs_change"], context["source_identity"]["pdb_guid"].lower())
    if instance.get("measured_id") != measured:
        raise ValueError("rollover_parent_identity")
    if "rollover" in context:
        descriptor = document(context["rollover"])
        if (descriptor.get("source_generation") != ack["source_generation"]
                or descriptor.get("final_cursor") != index["final_cursor"]
                or descriptor.get("source_id") != context["source_id"]):
            raise ValueError("rollover_parent_generation")
    elif ack["source_generation"] != 1:
        raise ValueError("rollover_parent_generation")
    return {"protocol": PREVIOUS, "engine": "oracle", "source_id": context["source_id"],
            "task_id": instance["lease_id"], "source_generation": ack["source_generation"],
            "target_generation": ack["target_generation"], "source_epoch": ack["source_epoch"],
            "measured_id": measured, "config_digest": instance["config_digest"],
            "schema_digest": context["typed_schema_sha256"], "l_end": end,
            "checkpoint_kind": "buffer_consumption", "buffer_context": index["context"],
            "buffer_index": parent_ref, "buffer_cursor": index["final_cursor"],
            "bundle_sha256": context["bundle"]["sha256"], "parent_ack": ack_ref,
            "request_digest": ack["request_digest"]}

def prepare(parent_ref, successor_ref, ack_ref, out):
    proof = prove(parent_ref, successor_ref)
    previous = parent_authority(parent_ref, ack_ref)
    pi, pc, _ = input_state(parent_ref)
    si, sc, sm = input_state(successor_ref)
    base = c.ordinal(pi["final_cursor"])
    out = Path(out).resolve()
    out.mkdir(mode=0o700, exist_ok=False)
    descriptor = {"protocol": PROTOCOL, "engine": "oracle", "source_id": pc["source_id"],
        "source_generation": previous["source_generation"]+1,
        "parent_source_generation": previous["source_generation"], "previous": previous,
        "parent_index": parent_ref, "successor_capture_index": successor_ref, "parent_ack": ack_ref,
        "initial_cursor": pi["final_cursor"], "final_cursor": si["final_cursor"], "proof": proof,
        "source_resume": {"kind": "vendor_offset_retained_separately",
            "parent_final_offset": pc["source_resume"]["final_connector_offset"],
            "successor_final_offset": sc["source_resume"]["final_connector_offset"],
            "connector_history": sc["source_resume"]["connector_history"],
            "source_offset_write": False, "reconnect_performed_by_this_tool": False}}
    b.atomic(out/"rollover.json", b.canonical(descriptor)+b"\n")
    ref = c.reference(out/"rollover.json")
    bundle = out/"bundle"
    bundle.mkdir(mode=0o700)
    artifacts = {}
    source_root = Path(sc["bundle"]["path"]).parent
    def copy(name):
        source = p.pinned({"path": str(source_root/name), "sha256": sm["artifacts"][name]})
        dest = bundle/name
        dest.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
        shutil.copyfile(source, dest)
        dest.chmod(0o600)
        artifacts[name] = b.sha(dest)
    copy("producer-journal.sqlite")
    # Keep codec schema receipts for empty/unchanged suffixes, not old fill rows.
    for name in sm["artifacts"]:
        if name.endswith(".receipt.json"):
            copy(name)
    frames = []
    for line in (source_root/"batches.jsonl").read_text().splitlines():
        frame = json.loads(line)
        batch = frame["batch"]
        if batch["kind"] != "cdc" or int(batch["position"]) <= base:
            continue
        batch["sequence"] = len(frames)+1
        frame.pop("source_position", None)
        for part in batch["parts"]:
            name = str(Path(part["path"]).relative_to(source_root))
            copy(name)
            part["path"] = str(bundle/name)
        frames.append(frame)
    b.atomic(bundle/"batches.jsonl", b"".join(b.canonical(frame)+b"\n" for frame in frames))
    artifacts["batches.jsonl"] = b.sha(bundle/"batches.jsonl")
    manifest = {**sm, "producer_sha256": b.sha(Path(p.__file__)),
        "boundary": {**sm["boundary"], "initial_position": str(base), "stream_mode": "delta"},
        "rollover": ref, "capture_bundle": sc["bundle"], "artifacts": artifacts,
        "batch_count": len(frames), "encoded_rows": sum(part["rows"] for f in frames for part in f["batch"]["parts"]),
        "target_apply": False, "admitted": False, "published": False}
    b.atomic(bundle/"manifest.json", b.canonical(manifest)+b"\n")
    context = {**sc, "bundle": c.reference(bundle/"manifest.json"), "initial_cursor": pi["final_cursor"],
               "rollover": ref}
    b.atomic(out/"context.json", b.canonical(context)+b"\n")
    context_ref = c.reference(out/"context.json")
    entries = []
    for original, frame in zip(si["transactions"][base:], frames, strict=True):
        payload = document(original)
        position = {**payload["source_position"], "context_sha256": context_ref["sha256"],
                    "bundle_sha256": context["bundle"]["sha256"], "typed_batch_digest": p.batch_digest(frame["batch"])}
        value = {**payload, "buffer_context": context_ref, "batch": frame["batch"], "source_position": position}
        path = out/f"transaction-{frame['batch']['sequence']:08d}.json"
        b.atomic(path, b.canonical(value)+b"\n")
        entries.append(c.reference(path))
    index = {"protocol": c.INDEX, "context": context_ref, "transactions": entries,
             "transaction_count": len(entries), "final_cursor": si["final_cursor"],
             "target_apply": False, "vendor_offset_modified": False}
    b.atomic(out/"index.json", b.canonical(index)+b"\n")
    verify(c.reference(out/"index.json"), ref)
    return {"rollover": ref, "buffer_index": c.reference(out/"index.json"), "bundle": context["bundle"],
            "source_id": pc["source_id"], "transactions": len(entries), "target_apply": False}

def verify(index_ref, rollover_ref):
    descriptor = document(rollover_ref)
    if descriptor.get("protocol") != PROTOCOL or descriptor.get("engine") != "oracle":
        raise ValueError("rollover_protocol")
    proof = prove(descriptor["parent_index"], descriptor["successor_capture_index"])
    previous = parent_authority(descriptor["parent_index"], descriptor["parent_ack"])
    pi, pc, _ = input_state(descriptor["parent_index"])
    si, sc, sm = input_state(descriptor["successor_capture_index"])
    c.verify(c.file_ref(index_ref), index_ref["sha256"])
    index = document(index_ref)
    context = document(index["context"])
    manifest = p.validate_bundle(c.file_ref(context["bundle"]).parent, context["bundle"]["sha256"])
    if (descriptor["source_id"] != pc["source_id"]
            or descriptor["source_generation"] != previous["source_generation"]+1
            or descriptor["parent_source_generation"] != previous["source_generation"] or descriptor["previous"] != previous
            or descriptor["proof"] != proof or descriptor["initial_cursor"] != pi["final_cursor"]
            or descriptor["final_cursor"] != si["final_cursor"]
            or context != {**sc, "bundle": context["bundle"], "initial_cursor": pi["final_cursor"], "rollover": rollover_ref}
            or manifest["rollover"] != rollover_ref or manifest["capture_bundle"] != sc["bundle"]
            or manifest["typed_tables"] != sm["typed_tables"] or manifest["source_mappings"] != sm["source_mappings"]
            or manifest["artifacts"]["producer-journal.sqlite"] != sm["artifacts"]["producer-journal.sqlite"]
            or manifest["boundary"] != {**sm["boundary"], "initial_position": pi["final_cursor"]["transaction_ordinal"], "stream_mode": "delta"}
            or manifest["checkpoints"] != sm["checkpoints"]):
        raise ValueError("rollover_binding")
    expected_offsets = {"kind": "vendor_offset_retained_separately",
        "parent_final_offset": pc["source_resume"]["final_connector_offset"],
        "successor_final_offset": sc["source_resume"]["final_connector_offset"],
        "connector_history": sc["source_resume"]["connector_history"],
        "source_offset_write": False, "reconnect_performed_by_this_tool": False}
    if descriptor["source_resume"] != expected_offsets:
        raise ValueError("rollover_offsets")
    base = c.ordinal(pi["final_cursor"])
    frames = [json.loads(line) for line in (Path(context["bundle"]["path"]).parent/"batches.jsonl").read_text().splitlines()]
    if len(frames) != len(si["transactions"])-base or len(frames) != len(index["transactions"]):
        raise ValueError("rollover_delta_coverage")
    for number, (original, ref, frame) in enumerate(zip(si["transactions"][base:], index["transactions"], frames), 1):
        captured, payload = document(original), document(ref)
        expected_batch = {**captured["batch"], "sequence": number}
        expected_position = {**captured["source_position"], "context_sha256": index["context"]["sha256"],
            "bundle_sha256": context["bundle"]["sha256"], "typed_batch_digest": p.batch_digest(expected_batch)}
        if (p.batch_digest(frame["batch"]) != p.batch_digest(expected_batch)
                or payload["batch"] != frame["batch"] or payload["source_position"] != expected_position
                or payload["buffer_context"] != index["context"]):
            raise ValueError("rollover_delta_payload")
    return {"protocol": "oo.oracle.rollover-validation/v1", "source_id": pc["source_id"],
            "previous": previous, "transactions": len(frames), "target_apply": False,
            "source_offset_write": False}

def initial_fill_plan(config, index, context, manifest):
    """Bind the separate fill sequence space; never infer it from cursor0/SCN."""
    if config.get("rollover") is not None or index["transactions"]:
        if config.get("rollover") is None and config.get("initial_mixed_resume") is True:
            return initial_mixed_plan(config, index, context, manifest)
        return None
    zero = c.cursor(0, None)
    if (context.get("rollover") is not None or context["initial_cursor"] != zero
            or index["final_cursor"] != zero or manifest["boundary"]["final_position"] != "0"):
        raise ValueError("initial_fill_cursor")
    root = p.pinned(config["bundle"]).parent
    frames_path = p.pinned({"path": str(root/"batches.jsonl"),
                           "sha256": manifest["artifacts"]["batches.jsonl"]})
    frames = [json.loads(line)["batch"] for line in frames_path.read_text().splitlines()]
    if not frames or manifest["batch_count"] != len(frames):
        raise ValueError("initial_fill_count")
    tables, batches = {}, []
    for n, batch in enumerate(frames, 1):
        if (batch["protocol"] != "oo.c1.typed-batch/v3" or batch["kind"] != "fill"
                or batch["sequence"] != n or batch["position"] != "0"
                or batch["previous_position"] != "0" or not batch["parts"]):
            raise ValueError("initial_fill_batch")
        for part in batch["parts"]:
            name, rows = part["table"], part["rows"]
            if (name not in manifest["typed_tables"] or part["spec"] != manifest["typed_tables"][name]
                    or type(rows) is not int or rows < 0):
                raise ValueError("initial_fill_table")
            table = tables.setdefault(name, {"last_sequence": n, "rows": 0})
            table["last_sequence"], table["rows"] = n, table["rows"] + rows
        batches.append({"sequence": n, "digest": p.batch_digest(batch)})
    if (set(tables) != set(manifest["typed_tables"])
            or sum(t["rows"] for t in tables.values()) != manifest["final_rows"]):
        raise ValueError("initial_fill_coverage")
    return {"protocol": "oo.oracle.initial-fill/v1", "source_id": manifest["source_id"],
            "config_digest": p.digest(config["admission_config"]),
            "typed_schema_sha256": context["typed_schema_sha256"],
            "buffer_index": config["buffer_index"], "buffer_context": index["context"],
            "bundle_sha256": context["bundle"]["sha256"], "batch_count": len(batches),
            "batches": batches, "tables": tables}


def initial_mixed_plan(config, index, context, manifest):
    """Exact fill prefix followed by complete captured transactions, in distinct spaces."""
    if (context.get("rollover") is not None or context["initial_cursor"] != c.cursor(0, None)
            or manifest["boundary"]["mode"] != "oracle_scn_snapshot_and_log"
            or not index["transactions"]):
        raise ValueError("initial_mixed_binding")
    c.verify(config["buffer_index"]["path"], config["buffer_index"]["sha256"])
    root = p.pinned(config["bundle"]).parent
    frames = [json.loads(line) for line in p.pinned({
        "path": str(root/"batches.jsonl"), "sha256": manifest["artifacts"]["batches.jsonl"]
    }).read_text().splitlines()]
    if len(frames) < 2 or len(frames) != manifest["batch_count"]:
        raise ValueError("initial_mixed_count")
    tables, batches, fill_count, position = {}, [], 0, 0
    for n, frame in enumerate(frames, 1):
        batch = frame["batch"]
        if (frame["kind"] != "typed_batch" or not p.batch_protocol_supported(batch)
                or batch["sequence"] != n or not batch["parts"]):
            raise ValueError("initial_mixed_sequence")
        entry = {"sequence": n, "digest": p.batch_digest(batch), "kind": batch["kind"],
                 "previous_position": batch["previous_position"], "position": batch["position"]}
        if batch["kind"] == "fill":
            if position or batch["position"] != "0" or batch["previous_position"] != "0":
                raise ValueError("initial_mixed_fill_after_cdc")
            fill_count += 1
            for part in batch["parts"]:
                name, rows = part["table"], part["rows"]
                if (name not in manifest["typed_tables"] or part["spec"] != manifest["typed_tables"][name]
                        or type(rows) is not int or rows < 0):
                    raise ValueError("initial_mixed_table")
                item = tables.setdefault(name, {"last_sequence": n, "rows": 0})
                item.update(last_sequence=n, rows=item["rows"]+rows)
        elif batch["kind"] == "cdc" and fill_count:
            if (batch["previous_position"] != str(position) or batch["position"] != str(position+1)
                    or position >= len(index["transactions"])):
                raise ValueError("initial_mixed_transaction_order")
            payload = document(index["transactions"][position])
            if payload["batch"] != batch or payload["buffer_context"] != index["context"]:
                raise ValueError("initial_mixed_transaction_binding")
            entry["source_evidence_sha256"] = p.digest(payload["source_position"])
            position += 1
        else:
            raise ValueError("initial_mixed_kind")
        batches.append(entry)
    snapshot = document(manifest["boundary"]["native_snapshot"])
    if (not fill_count or not position or position != len(index["transactions"])
            or snapshot["complete"] is not True
            or sum(t["rows"] for t in tables.values()) != snapshot["rows"]
            or set(tables) != set(manifest["typed_tables"])
            or str(position) != manifest["boundary"]["final_position"]
            or index["final_cursor"]["transaction_ordinal"] != str(position)):
        raise ValueError("initial_mixed_coverage")
    return {"protocol": "oo.oracle.initial-mixed/v1", "source_id": manifest["source_id"],
            "config_digest": p.digest(config["admission_config"]),
            "typed_schema_sha256": context["typed_schema_sha256"],
            "buffer_index": config["buffer_index"], "buffer_context": index["context"],
            "bundle_sha256": context["bundle"]["sha256"], "batch_count": len(batches),
            "fill_batch_count": fill_count, "final_position": str(position),
            "batches": batches, "tables": tables}


def fill_stream_state(config, bootstrap, index, context, manifest, plan):
    resume = bootstrap.get("resume")
    mixed = plan["protocol"] == "oo.oracle.initial-mixed/v1"
    if bootstrap.get("previous") is not None or bootstrap["source_generation"] != 1:
        raise ValueError("initial_fill_parent")
    sequence, epoch, completed = 0, None, []
    if resume is not None:
        expected = {"protocol": FILL_RESUME, "engine": "oracle", "source_id": bootstrap["source_id"],
                    "task_id": bootstrap["lease_id"], "source_generation": 1,
                    "config_digest": plan["config_digest"], "schema_digest": plan["typed_schema_sha256"],
                    "buffer_index": config["buffer_index"], "buffer_context": index["context"],
                    "bundle_sha256": context["bundle"]["sha256"], "initial_fill": plan}
        if (any(resume.get(k) != v for k, v in expected.items())
                or resume.get("state") not in ("open", "awaiting_publish")
                or "last_ack" not in resume
                or type(resume.get("lease_generation")) is not int
                or not 1 <= resume["lease_generation"] <= bootstrap["lease_generation"]
                or type(resume.get("source_epoch")) is not int or resume["source_epoch"] < 1):
            raise ValueError("initial_fill_resume_binding")
        epoch = resume["source_epoch"]
        if resume["last_ack"] is not None:
            ack = document(resume["last_ack"])
            sequence = ack.get("receipt", {}).get("sequence")
            if (ack.get("protocol") != "oo.jobs.oracle-c1-baseline-ack/v1"
                    or ack.get("stage") != "typed_batch" or ack.get("durable") is not True
                    or type(sequence) is not int or not 1 <= sequence <= plan["batch_count"]):
                raise ValueError("initial_fill_resume_ack")
            receipt = ack["receipt"]
            exact = {"protocol": "oo.c1.typed-ack/v3", "source": bootstrap["source_id"],
                     "task": bootstrap["lease_id"], "generation": resume["lease_generation"],
                    "epoch": epoch, "sequence": sequence,
                    "position": plan["batches"][sequence-1]["position"] if mixed else "0",
                     "digest": plan["batches"][sequence-1]["digest"]}
            if any(receipt.get(k) != v for k, v in exact.items()):
                raise ValueError("initial_fill_resume_ack")
            if mixed and plan["batches"][sequence-1]["kind"] == "cdc":
                payload = next((document(ref) for ref in index["transactions"]
                                if document(ref)["batch"]["sequence"] == sequence), None)
                if payload is None:
                    raise ValueError("initial_mixed_ack_transaction")
                p.validate_buffer_ack(ack, payload["source_position"])
        completed = resume.get("completed_tables")
        if (resume.get("resume_sequence") != sequence or not isinstance(completed, list)
                or any(not isinstance(name, str) for name in completed)
                or completed != sorted(set(completed))
                or any(name not in plan["tables"] or plan["tables"][name]["last_sequence"] > sequence
                       for name in completed)):
            raise ValueError("initial_fill_checkpoint_or_complete")
        if mixed and sequence > plan["fill_batch_count"] and completed != sorted(plan["tables"]):
            raise ValueError("initial_mixed_incomplete_snapshot")
        if resume["state"] == "awaiting_publish" and (
                sequence != plan["batch_count"] or completed != sorted(plan["tables"])):
            raise ValueError("initial_fill_unsealed")
    return {"descriptor": None, "sequence": sequence, "source_epoch": epoch,
            "initial_fill": plan, "completed_tables": completed}


def stream_state(config, bootstrap, index, context, manifest):
    """Only an exact prior final ACK or this task's durable typed ACK permits skipping."""
    previous, resume = bootstrap.get("previous"), bootstrap.get("resume")
    plan = initial_fill_plan(config, index, context, manifest)
    if plan is not None:
        return fill_stream_state(config, bootstrap, index, context, manifest, plan)
    descriptor = None
    if config.get("rollover") is not None:
        validated = verify(config["buffer_index"], config["rollover"])
        descriptor = document(config["rollover"])
        if (previous != validated["previous"] or bootstrap["source_generation"] != descriptor["source_generation"]
                or p.digest(config["admission_config"]) != previous["config_digest"]):
            raise ValueError("rollover_bootstrap_previous")
        if not validated["transactions"]:
            raise ValueError("rollover_no_new_transactions")
        epoch = previous["source_epoch"]
    else:
        if previous is not None or bootstrap["source_generation"] != 1 or "rollover" in context:
            raise ValueError("rollover_parent_required")
        epoch = None
    sequence = 0
    if resume is not None:
        expected = {"protocol": RESUME, "engine": "oracle", "source_id": bootstrap["source_id"],
            "task_id": bootstrap["lease_id"], "source_generation": bootstrap["source_generation"],
            "config_digest": p.digest(config["admission_config"]), "buffer_index": config["buffer_index"],
            "buffer_context": index["context"], "bundle_sha256": context["bundle"]["sha256"]}
        if (any(resume.get(k) != v for k, v in expected.items()) or resume.get("state") not in ("open", "awaiting_publish")
                or "last_ack" not in resume
                or type(resume.get("lease_generation")) is not int
                or not 1 <= resume["lease_generation"] <= bootstrap["lease_generation"]
                or type(resume.get("source_epoch")) is not int or resume["source_epoch"] < 1
                or (epoch is not None and epoch != resume["source_epoch"])):
            raise ValueError("rollover_resume_binding")
        epoch = resume["source_epoch"]
        if resume.get("last_ack") is not None:
            ack = document(resume["last_ack"])
            if (ack.get("protocol") != "oo.jobs.oracle-c1-baseline-ack/v1"
                    or ack.get("stage") != "typed_batch"):
                raise ValueError("rollover_resume_ack_protocol")
            sequence = ack.get("receipt", {}).get("sequence")
            if type(sequence) is not int or not 1 <= sequence <= len(index["transactions"]):
                raise ValueError("rollover_resume_sequence")
            payload = document(index["transactions"][sequence-1])
            if payload["batch"]["sequence"] != sequence:
                raise ValueError("rollover_resume_sequence")
            p.validate_typed_ack(ack, payload["batch"], {**bootstrap, "lease_generation": resume["lease_generation"]}, epoch)
            p.validate_buffer_ack(ack, payload["source_position"])
        if resume["state"] == "awaiting_publish" and sequence != len(index["transactions"]):
            raise ValueError("rollover_resume_unsealed")
        # Mixed initial fill+CDC buffers are not the pure-fill recovery contract.
        if descriptor is None:
            raise ValueError("rollover_resume_delta_only")
    return {"descriptor": descriptor, "sequence": sequence, "source_epoch": epoch}

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--parent-index", type=Path, required=True)
    parser.add_argument("--successor-index", type=Path, required=True)
    parser.add_argument("--parent-ack", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    print(json.dumps(prepare(c.reference(args.parent_index), c.reference(args.successor_index),
                             c.reference(args.parent_ack), args.out)))

if __name__ == "__main__":
    try:
        main()
    except (ValueError, KeyError, OSError, sqlite3.Error) as error:
        print(json.dumps({"error": str(error) if isinstance(error, ValueError) else type(error).__name__,
                          "target_apply": False, "source_offset_write": False}), file=sys.stderr)
        sys.exit(2)
