#!/usr/bin/env python3
"""Durable Debezium transport spool and committed-transaction boundary adapter.

No redo parser. HTTP success ACKs only durable capture, never target publication.
"""
import argparse
import hashlib
from http.server import BaseHTTPRequestHandler, HTTPServer
import json
from pathlib import Path
import sqlite3


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":")).encode()


def connect(path):
    db = sqlite3.connect(path)
    db.execute("PRAGMA journal_mode=WAL")
    db.execute("PRAGMA synchronous=FULL")
    db.execute("PRAGMA cache_size=-4096")
    db.executescript("""
      CREATE TABLE IF NOT EXISTS transport(seq INTEGER PRIMARY KEY, digest TEXT UNIQUE NOT NULL, body BLOB NOT NULL);
      CREATE TABLE IF NOT EXISTS events(tx TEXT, ord INTEGER, body BLOB NOT NULL, PRIMARY KEY(tx,ord));
      CREATE TABLE IF NOT EXISTS boundaries(tx TEXT PRIMARY KEY, expected INTEGER, body BLOB NOT NULL);
      CREATE TABLE IF NOT EXISTS snapshots(seq INTEGER PRIMARY KEY, body BLOB NOT NULL);
      CREATE TABLE IF NOT EXISTS schemas(table_id TEXT PRIMARY KEY, digest TEXT NOT NULL);
    """)
    return db


def payload(record):
    if not isinstance(record, dict):
        raise ValueError("invalid_debezium_envelope")
    return record.get("payload", record)


def ingest(db, record):
    value = payload(record)
    if value is None:
        return "tombstone"
    if value.get("status") == "BEGIN":
        return "begin"
    if value.get("status") == "END":
        tx = value["id"]
        count = value["event_count"]
        if type(count) is not int or count < 0:
            raise ValueError("transaction_count")
        if count == 0:
            return "empty_transaction_no_target_checkpoint"
        encoded = canonical(value)
        prior = db.execute("SELECT expected FROM boundaries WHERE tx=?", (tx,)).fetchone()
        if prior and prior[0] != count:
            raise ValueError("transaction_end_conflict")
        db.execute("INSERT OR IGNORE INTO boundaries VALUES (?,?,?)", (tx, count, encoded))
        return "end"
    if value.get("op") in ("r", "c", "u", "d"):
        schema = record.get("schema")
        if not isinstance(schema, dict):
            raise ValueError("typed_schema_required")
        source = value.get("source") or {}
        table = canonical([source.get(k) for k in ("db", "schema", "table")]).decode()
        digest = hashlib.sha256(canonical(schema)).hexdigest()
        prior = db.execute("SELECT digest FROM schemas WHERE table_id=?", (table,)).fetchone()
        if prior and prior[0] != digest:
            raise ValueError("schema_drift_rebaseline_required")
        db.execute("INSERT OR IGNORE INTO schemas VALUES (?,?)", (table, digest))
    if value.get("op") == "r":
        if value["source"].get("connector") != "oracle":
            raise ValueError("not_oracle")
        db.execute("INSERT INTO snapshots(body) VALUES (?)", (canonical(record),))
        return "snapshot_not_c1"
    if value.get("op") not in ("c", "u", "d"):
        raise ValueError("schema_or_unknown_event: rebaseline/review required")
    source = value["source"]
    if source.get("connector") != "oracle" or source.get("snapshot") not in ("false", False):
        raise ValueError("not_live_oracle_log")
    if not str(source.get("scn", "")).isdigit() or not str(source.get("commit_scn", "")).isdigit():
        raise ValueError("missing_log_commit_position")
    transaction = value.get("transaction") or {}
    tx, order = transaction.get("id"), transaction.get("total_order")
    if not isinstance(tx, str) or not tx or type(order) is not int or order < 1:
        raise ValueError("transaction_metadata_required")
    # Transport timestamps can change on replay; the logical event identity cannot.
    stable = {k: value.get(k) for k in ("op", "before", "after", "transaction")}
    stable["source"] = {k: source.get(k) for k in ("connector", "db", "schema", "table", "scn", "commit_scn", "txId", "rs_id", "ssn")}
    stable["schema"] = record.get("schema")
    encoded = canonical(stable)
    prior = db.execute("SELECT body FROM events WHERE tx=? AND ord=?", (tx, order)).fetchone()
    if prior and prior[0] != encoded:
        raise ValueError("replay_conflict")
    db.execute("INSERT OR IGNORE INTO events VALUES (?,?,?)", (tx, order, encoded))
    return "log"


def receive(db, raw):
    record = json.loads(raw)
    records = record if isinstance(record, list) else [record]
    digest = hashlib.sha256(raw).hexdigest()
    with db:
        if db.execute("SELECT 1 FROM transport WHERE digest=?", (digest,)).fetchone():
            return
        for item in records:
            ingest(db, item)
        db.execute("INSERT INTO transport(digest,body) VALUES (?,?)", (digest, raw))


def committed(db):
    """Emit only complete transactions; an END alone never advances a checkpoint."""
    for tx, expected in db.execute("SELECT tx,expected FROM boundaries ORDER BY rowid"):
        count, lo, hi = db.execute("SELECT COUNT(*),MIN(ord),MAX(ord) FROM events WHERE tx=?", (tx,)).fetchone()
        if count != expected or (expected and (lo != 1 or hi != expected)):
            continue
        positions = set()
        for (raw,) in db.execute("SELECT body FROM events WHERE tx=? ORDER BY ord", (tx,)):
            positions.add(json.loads(raw)["source"]["commit_scn"])
        if len(positions) != 1 and expected:
            raise ValueError("transaction_commit_scn_conflict")
        yield {"transaction_id": tx, "event_count": count,
               "commit_scn": next(iter(positions)) if positions else None,
               "checkpoint": "Debezium offset store belongs to capture; target ACK required separately"}


def summary(db):
    transactions = list(committed(db))
    counts = {"c": 0, "u": 0, "d": 0}
    for transaction in transactions:
        for (raw,) in db.execute("SELECT body FROM events WHERE tx=?", (transaction["transaction_id"],)):
            counts[json.loads(raw)["op"]] += 1
    result = {
        "protocol": "oo.oracle.debezium-c1-spool/v1", "method": "C1",
        "route": "Debezium Oracle LogMiner", "transactions": transactions, "committed_counts": counts,
        "snapshot_rows_not_c1": db.execute("SELECT COUNT(*) FROM snapshots").fetchone()[0],
        "captured_log_rows": db.execute("SELECT COUNT(*) FROM events").fetchone()[0],
        "transport_messages": db.execute("SELECT COUNT(*) FROM transport").fetchone()[0],
        "incomplete_transactions": db.execute("SELECT COUNT(DISTINCT tx) FROM events").fetchone()[0] - len(transactions),
        "target_apply": False, "target_ack": False, "published": False, "c5": "separate",
    }
    return result


def receipt(path):
    db = connect(path)
    try:
        return summary(db)
    finally:
        db.close()


def serve(path, port):
    db = connect(path)

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_GET(self):
            if self.path == "/receipt":
                body = canonical(summary(db))
            elif self.path == "/backup":
                target = sqlite3.connect(str(path) + ".export")
                db.backup(target)
                target.close()
                body = b"backed-up\n"
            else:
                body = b"durable-capture-only\n"
            self.send_response(200)
            self.end_headers()
            self.wfile.write(body)

        def do_POST(self):
            try:
                length = int(self.headers.get("Content-Length", "0"))
                if not 0 < length <= 16*1024*1024:
                    raise ValueError("request_budget")
                raw = self.rfile.read(length)
                receive(db, raw)
                code = 200
            except (ValueError, KeyError, sqlite3.Error):
                code = 422
            self.send_response(code)
            self.end_headers()
            self.wfile.write(b"captured\n" if code == 200 else b"capture_refused\n")

    # Acceptance runs this only inside a new internal Docker network, no host port.
    HTTPServer(("0.0.0.0", port), Handler).serve_forever()


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("action", choices=["serve", "receipt"])
    parser.add_argument("--db", type=Path, required=True)
    parser.add_argument("--port", type=int, default=8080)
    args = parser.parse_args()
    if args.action == "serve":
        serve(args.db, args.port)
    else:
        print(json.dumps(receipt(args.db), indent=2))
