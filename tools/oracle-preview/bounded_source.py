#!/usr/bin/env python3
"""Local Oracle snapshot/C5 staging. This does not publish or ACK a source offset."""
from __future__ import annotations

import argparse
import contextlib
import fcntl
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import time


def canonical(value):
    return json.dumps(value, ensure_ascii=True, sort_keys=True, separators=(",", ":")).encode()


def sha(path):
    h = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for data in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(data)
    return h.hexdigest()


def atomic(path, data):
    path = Path(path)
    temp = path.with_name(path.name + ".partial")
    with temp.open("wb") as stream:
        os.chmod(temp, 0o600)
        stream.write(data)
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(temp, path)
    fd = os.open(path.parent, os.O_RDONLY)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


@contextlib.contextmanager
def locked(root):
    root.mkdir(parents=True, mode=0o700, exist_ok=True)
    with (root / "writer.lock").open("a") as stream:
        fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
        yield


class Driver:
    def __init__(self, binary, config):
        self.binary = Path(binary).resolve(strict=True)
        self.digest = sha(self.binary)
        self.config = config

    def call(self, action, **fields):
        if sha(self.binary) != self.digest:
            raise ValueError("driver_changed")
        request = {"action": action, "config": self.config, **fields}
        proc = subprocess.run([str(self.binary)], input=canonical(request), capture_output=True, timeout=45)
        if proc.returncode:
            try:
                error = json.loads(proc.stderr)["error"]
            except (ValueError, KeyError):
                error = "driver_failed_redacted"
            raise ValueError(error)
        result = json.loads(proc.stdout)
        if "error" in result:
            raise ValueError("structured_driver_error")
        return result


def database(path, readonly=False):
    db = sqlite3.connect(f"file:{path}?mode=ro", uri=True) if readonly else sqlite3.connect(path)
    db.execute("PRAGMA cache_size=-4096")
    db.execute("PRAGMA temp_store=FILE")
    if not readonly:
        db.execute("PRAGMA journal_mode=WAL")
        db.execute("PRAGMA synchronous=FULL")
        db.executescript("""
            CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS parts (
                ordinal INTEGER PRIMARY KEY, table_id TEXT NOT NULL, filename TEXT UNIQUE NOT NULL,
                digest TEXT NOT NULL, lower_key TEXT NOT NULL, upper_key TEXT NOT NULL,
                row_count INTEGER NOT NULL, eof INTEGER NOT NULL, verified_cells INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS cells (
                table_id TEXT NOT NULL, key TEXT NOT NULL, row_json TEXT NOT NULL,
                row_hash TEXT NOT NULL, PRIMARY KEY(table_id,key));
        """)
    return db


def table_id(table):
    return canonical([table["owner"], table["table"]]).decode()


def check_part(part, table, scn, lower, rows, byte_limit):
    if (part.get("protocol") != "oo.oracle.bounded-chunk/v1"
            or part.get("table") != table or part.get("scn") != scn
            or part.get("lower_exclusive") != lower):
        raise ValueError("part_binding")
    values = part["rows"]
    if len(values) > rows or part["row_count"] != len(values) or part["eof"] != (len(values) < rows):
        raise ValueError("part_row_budget_or_eof")
    indices = [next(i for i, c in enumerate(table["columns"]) if c["name"] == key) for key in table["keys"]]
    if not indices:
        raise ValueError("no_key")
    for row in values:
        if len(row) != len(table["columns"]) or any(v is not None and not isinstance(v, str) for v in row):
            raise ValueError("part_typed_shape")
        if any(row[i] is None for i in indices):
            raise ValueError("null_key")
    upper = [values[-1][i] for i in indices] if values else lower
    if upper != part["upper_inclusive"] or (values and upper == lower):
        raise ValueError("part_cursor")
    if sum(len(canonical(row)) + 1 for row in values) > byte_limit * 6:
        # JSON escaping can expand UTF-8; the native byte ledger is checked too.
        raise ValueError("part_encoded_byte_budget")
    if part["logical_bytes"] > byte_limit or part["reader_mutations"] != 0:
        raise ValueError("part_source_contract")
    return indices


def exclusions_for(inventory, approvals):
    known = {f'{t["owner"]}.{t["table"]}': t for t in inventory["tables"]}
    if any(name not in known or not isinstance(reason, str) or not reason.strip() for name, reason in approvals.items()):
        raise ValueError("invalid_exclusion_approval")
    rejected = {name: table["issues"] for name, table in known.items() if table["issues"] and name not in approvals}
    if rejected:
        raise ValueError("inventory_rejected:" + canonical(rejected).decode())
    return [t for name, t in known.items() if name not in approvals]


def capture(driver, root, *, chunk_rows=256, chunk_bytes=2*1024*1024, exclusions=None,
            verify=True, stop_after=None, failpoint=None, deadline_seconds=1200,
            budget_rows=10_000_000, budget_bytes=1024*1024*1024, snapshot_scn=None):
    root = Path(root).resolve()
    approvals = exclusions or {}
    if (not 1 <= chunk_rows <= 4096 or not 1024 <= chunk_bytes <= 16*1024*1024
            or budget_rows < 1 or budget_bytes < 1024 or deadline_seconds < 1):
        raise ValueError("budget")
    start = time.monotonic()
    with locked(root), contextlib.closing(database(root / "journal.sqlite")) as db:
        inventory = driver.call("inventory")
        selected = exclusions_for(inventory, approvals)
        if not selected:
            raise ValueError("empty_scope")
        existing = db.execute("SELECT value FROM meta WHERE key='binding'").fetchone()
        prior = json.loads(existing[0]) if existing else None
        if snapshot_scn is not None and (
                not isinstance(snapshot_scn, str) or not snapshot_scn.isascii()
                or not snapshot_scn.isdigit() or str(int(snapshot_scn)) != snapshot_scn
                or not 0 < int(snapshot_scn) <= int(inventory["scn"])
                or (prior is not None and prior["scn"] != snapshot_scn)):
            raise ValueError("snapshot_scn_invalid_or_changed")
        binding = {
            "protocol": "oo.oracle.chunk-stage/v1", "source_id": inventory["source_id"],
            "identity": inventory["identity"], "inventory": inventory["tables"],
            "scope": inventory["scope"], "exclusions": approvals, "driver_sha256": driver.digest,
            "chunk_rows": chunk_rows, "chunk_bytes": chunk_bytes, "typed_verify": verify,
            "read_budget": {"rows": budget_rows, "logical_bytes": budget_bytes, "seconds": deadline_seconds,
                            "scope": "row data including independent verification; not wire bytes/catalog traffic"},
            "scn": prior["scn"] if prior else snapshot_scn or inventory["scn"],
        }
        if prior is not None and prior != binding:
            raise ValueError("resume_binding_drift")
        if prior is None:
            with db:
                db.execute("INSERT INTO meta VALUES ('binding',?)", (canonical(binding).decode(),))
                db.execute("INSERT INTO meta VALUES ('budget',?)", (canonical({"rows": 0, "logical_bytes": 0, "seconds": 0.0, "attempts": 0}).decode(),))
        # Never trust a saved ACK without rehashing its sealed payload.
        for filename, expected in db.execute("SELECT filename,digest FROM parts ORDER BY ordinal"):
            if sha(root / filename) != expected:
                raise ValueError("sealed_part_digest_mismatch")
        added = 0
        for table in selected:
            tid = table_id(table)
            previous = db.execute("SELECT upper_key,eof FROM parts WHERE table_id=? ORDER BY ordinal DESC LIMIT 1", (tid,)).fetchone()
            if previous and previous[1]:
                continue
            lower = json.loads(previous[0]) if previous else None
            while True:
                if time.monotonic() - start > deadline_seconds:
                    raise TimeoutError("snapshot_deadline: resume same SCN or explicitly rebaseline")
                ordinal = db.execute("SELECT COALESCE(MAX(ordinal),-1)+1 FROM parts").fetchone()[0]
                ledger = json.loads(db.execute("SELECT value FROM meta WHERE key='budget'").fetchone()[0])
                factor = 2 if verify else 1
                reservation = {"rows": chunk_rows*factor, "logical_bytes": chunk_bytes*factor, "seconds": 45*factor}
                limits = {"rows": budget_rows, "logical_bytes": budget_bytes, "seconds": deadline_seconds}
                if any(ledger[k] + reservation[k] > limits[k] for k in reservation):
                    raise ValueError("durable_read_budget_exhausted")
                # Crash/timeout reservations remain charged; a resume never resets them.
                reserved = {k: ledger[k] + reservation[k] for k in reservation}
                reserved["attempts"] = ledger["attempts"] + 1
                with db:
                    db.execute("UPDATE meta SET value=? WHERE key='budget'", (canonical(reserved).decode(),))
                read_started = time.monotonic()
                part = driver.call("chunk", table=table, scn=binding["scn"], lower=lower,
                                   chunk_rows=chunk_rows, chunk_bytes=chunk_bytes)
                indices = check_part(part, table, binding["scn"], lower, chunk_rows, chunk_bytes)
                filename = f"part-{ordinal:08d}.json"
                path = root / filename
                data = canonical(part) + b"\n"
                if path.exists() and path.read_bytes() != data:
                    raise ValueError("orphan_part_drift")
                if not path.exists():
                    atomic(path, data)
                if failpoint == "after_seal":
                    raise RuntimeError("injected_after_seal_before_ack")
                expected = sha(path)
                checked = driver.call("verify", table=table, part=str(path), sha256=expected,
                                      chunk_rows=chunk_rows) if verify else {"typed_cells": 0, "differences": 0}
                if checked["differences"] != 0 or (verify and checked["typed_cells"] != len(part["rows"])*len(table["columns"])):
                    raise ValueError("typed_reconciliation_failed")
                # Staging rows and this part's durable ACK commit together. No source/C1 ACK.
                with db:
                    for row in part["rows"]:
                        key = canonical([row[i] for i in indices]).decode()
                        encoded = canonical(row)
                        db.execute("INSERT INTO cells VALUES (?,?,?,?)", (tid, key, encoded.decode(), hashlib.sha256(encoded).hexdigest()))
                    db.execute("INSERT INTO parts VALUES (?,?,?,?,?,?,?,?,?)", (
                        ordinal, tid, filename, expected, canonical(lower).decode(),
                        canonical(part["upper_inclusive"]).decode(), len(part["rows"]),
                        int(part["eof"]), checked["typed_cells"]))
                    ledger["rows"] += len(part["rows"])*factor
                    ledger["logical_bytes"] += part["logical_bytes"]*factor
                    ledger["seconds"] += time.monotonic() - read_started
                    ledger["attempts"] += 1
                    db.execute("UPDATE meta SET value=? WHERE key='budget'", (canonical(ledger).decode(),))
                added += 1
                if stop_after is not None and added >= stop_after:
                    return {"complete": False, "new_parts": added, "source_ack": False, "scn": binding["scn"]}
                lower = part["upper_inclusive"]
                if part["eof"]:
                    break
        final_inventory = driver.call("inventory")
        if final_inventory["identity"] != binding["identity"] or final_inventory["tables"] != binding["inventory"]:
            raise ValueError("schema_drift_before_seal")
        parts = [dict(zip(("ordinal", "table_id", "file", "sha256", "lower", "upper", "rows", "eof", "typed_cells"), row))
                 for row in db.execute("SELECT * FROM parts ORDER BY ordinal")]
        content = hashlib.sha256()
        for tid, key, row_hash in db.execute("SELECT table_id,key,row_hash FROM cells ORDER BY table_id,key"):
            content.update(canonical([tid, key, row_hash]) + b"\n")
        db.execute("PRAGMA wal_checkpoint(TRUNCATE)")
        manifest = {
            "protocol": "oo.oracle.sealed-snapshot/v1", "binding": binding, "parts": parts,
            "journal_sha256": sha(root / "journal.sqlite"), "content_sha256": content.hexdigest(),
            "rows": sum(p["rows"] for p in parts), "typed_cells": sum(p["typed_cells"] for p in parts),
            "complete": True, "scope_complete": not approvals, "published": False,
            "source_ack": False, "c1": "not_run", "capture": "SCN-pinned snapshot/C5 only",
            "read_budget_used": json.loads(db.execute("SELECT value FROM meta WHERE key='budget'").fetchone()[0]),
            "delete_quality": "DELETES_LAGGED until complete-generation reconciliation",
            "storage": "column-typed exact text/null cells; NUMBER never float, RAW hex; TZ region normalized to numeric offset; no target coercion authorized",
        }
        atomic(root / "manifest.json", canonical(manifest) + b"\n")
        return manifest


def load_sealed(root, expected_digest):
    root = Path(root).resolve()
    if sha(root / "manifest.json") != expected_digest:
        raise ValueError("manifest_digest")
    manifest = json.loads((root / "manifest.json").read_text())
    if manifest.get("protocol") != "oo.oracle.sealed-snapshot/v1" or not manifest["complete"]:
        raise ValueError("snapshot_incomplete")
    if sha(root / "journal.sqlite") != manifest["journal_sha256"]:
        raise ValueError("journal_digest")
    for part in manifest["parts"]:
        name = part["file"]
        if Path(name).name != name or sha(root / name) != part["sha256"]:
            raise ValueError("sealed_part_digest_mismatch")
    return manifest


def c5_diff(parent, current, out, parent_digest, current_digest):
    parent, current, out = Path(parent).resolve(), Path(current).resolve(), Path(out).resolve()
    old, new = load_sealed(parent, parent_digest), load_sealed(current, current_digest)
    for field in ("source_id", "identity", "inventory", "exclusions", "scope"):
        if old["binding"][field] != new["binding"][field]:
            raise ValueError("c5_binding_or_schema_drift")
    if int(new["binding"]["scn"]) < int(old["binding"]["scn"]):
        raise ValueError("c5_scn_regression")
    if out.exists():
        raise ValueError("diff_output_exists")
    out.mkdir(parents=True, mode=0o700)
    counts = {"insert": 0, "update": 0, "delete": 0}
    with contextlib.closing(database(current / "journal.sqlite", readonly=True)) as db:
        db.execute("ATTACH DATABASE ? AS old", (f"file:{parent / 'journal.sqlite'}?mode=ro",))
        sql = """SELECT 'insert',n.table_id,n.key,NULL,n.row_json FROM cells n LEFT JOIN old.cells o
                 ON n.table_id=o.table_id AND n.key=o.key WHERE o.key IS NULL
                 UNION ALL SELECT 'update',n.table_id,n.key,o.row_json,n.row_json FROM cells n JOIN old.cells o
                 ON n.table_id=o.table_id AND n.key=o.key WHERE n.row_hash<>o.row_hash
                 UNION ALL SELECT 'delete',o.table_id,o.key,o.row_json,NULL FROM old.cells o LEFT JOIN cells n
                 ON n.table_id=o.table_id AND n.key=o.key WHERE n.key IS NULL"""
        with (out / "changes.jsonl").open("xb") as stream:
            os.chmod(out / "changes.jsonl", 0o600)
            for op, table, key, before, after in db.execute(sql):
                stream.write(canonical({"op": op, "table": json.loads(table), "key": json.loads(key),
                                        "before": json.loads(before) if before else None,
                                        "after": json.loads(after) if after else None}) + b"\n")
                counts[op] += 1
            stream.flush()
            os.fsync(stream.fileno())
    receipt = {"protocol": "oo.oracle.c5-diff/v1", "method": "C5", "c1": "not_run", "counts": counts,
               "parent_manifest": parent_digest, "current_manifest": current_digest,
               "changes_sha256": sha(out / "changes.jsonl"), "empty_diff": not any(counts.values()),
               "apply": False, "ack": False, "full_generation_reconcile": True,
               "window": "all admitted keys, not a recent-time window"}
    atomic(out / "receipt.json", canonical(receipt) + b"\n")
    return receipt


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--reader", required=True)
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--chunk-rows", type=int, default=256)
    parser.add_argument("--chunk-bytes", type=int, default=2*1024*1024)
    parser.add_argument("--exclusions", type=Path)
    parser.add_argument("--stop-after", type=int)
    args = parser.parse_args()
    config = json.loads(args.config.read_text())
    result = capture(Driver(args.reader, config), args.out, chunk_rows=args.chunk_rows,
                     chunk_bytes=args.chunk_bytes, stop_after=args.stop_after,
                     exclusions=json.loads(args.exclusions.read_text()) if args.exclusions else {})
    print(json.dumps({k: v for k, v in result.items() if k not in ("binding", "parts")}))


if __name__ == "__main__":
    main()
