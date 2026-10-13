#!/usr/bin/env python3
"""Live C4 window. Product account statements are reads. DBA mutates the fixture.

The Postgres database is this task's acceptance shadow. It is not the shared
Loader, not a Knowledge Pack, and not the Gateway.
"""

from __future__ import annotations

import decimal
import json
import os
import re
import subprocess
import time
from datetime import datetime, timezone
from decimal import Decimal
from pathlib import Path

import arrow_stream
import c4_capture as cap

ROOT = Path("/Users/redux/puppy-one/open-ontology")
OUT = Path(os.environ["OO_C4_OUT"])
CH = "oo-dl01w1-40-clickhouse"
PG = "oo-dl01w1-40-pg"
READER = "oo_dl01w1_40_reader"
DBA = "oo_dl01w1_40_dba"
READER_PASSWORD = "/etc/oo/reader.password"
DBA_PASSWORD = "/etc/oo/dba.password"
CUSTOMER = ROOT / (
    "evidence/4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/DL01/clickhouse-product/"
    "raw/run/product-run-11-http401/plan/customer.json"
)
FIXTURE = ROOT / "testing/dl01-driver-preflight/clickhouse/sql/00-fixture.sql"
SCHEMA = [
    {"name": "reading_id", "type": "UInt64"},
    {"name": "version", "type": "UInt64"},
    {"name": "amount", "type": "Decimal(76, 10)"},
    {"name": "note", "type": "Nullable(String)"},
    {"name": "code", "type": "LowCardinality(String)"},
    {"name": "code_or_null", "type": "LowCardinality(Nullable(String))"},
    {"name": "ts", "type": "DateTime64(9, 'UTC')"},
    {"name": "tags", "type": "Array(Int64)"},
    {"name": "labels", "type": "Array(Nullable(String))"},
    {"name": "qty", "type": "Int64"},
    {"name": "label", "type": "String"},
    {"name": "wide_u64", "type": "UInt64"},
]
FIELDS = ["reading_id", "ts", "amount", "note", "qty"]
WRITE_KINDS = {
    "Insert",
    "Alter",
    "Create",
    "Drop",
    "Rename",
    "Optimize",
    "Truncate",
    "KillQuery",
    "Grant",
    "Revoke",
    "Move",
    "Undrop",
    "System",
    "Delete",
    "Update",
}
AMOUNT = re.compile(r"^-?\d+(\.\d+)?$")
IDENT = re.compile(r"^[0-9]+$")
SEQ = 0


def next_id(prefix: str) -> str:
    global SEQ
    SEQ += 1
    return f"{prefix}{SEQ}"


def run(args: list[str], timeout: int = 90, data: bytes | None = None) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run(args, input=data, capture_output=True, timeout=timeout)


def ch(user: str, password_file: str, sql: str, query_id: str, partition: str = "", timeout: int = 90) -> subprocess.CompletedProcess[bytes]:
    script = (
        "export CLICKHOUSE_PASSWORD=\"$(tr -d '[:space:]' < \"$1\")\"; "
        'args=(clickhouse-client --multiquery --user "$2" --query_id "$4"); '
        'if [ -n "${5:-}" ]; then args+=(--param_partition "$5"); fi; '
        'args+=(--query "$3"); '
        '"${args[@]}"'
    )
    return run(
        ["docker", "exec", "-i", CH, "bash", "-lc", script, "bash", password_file, user, sql, query_id, partition],
        timeout=timeout,
    )


def psql(sql: str, timeout: int = 60) -> subprocess.CompletedProcess[bytes]:
    return run(
        ["docker", "exec", "-i", PG, "psql", "-U", "oo", "-d", "oo_shadow", "-v", "ON_ERROR_STOP=1", "-t", "-A"],
        timeout=timeout,
        data=sql.encode(),
    )


def text_of(proc: subprocess.CompletedProcess[bytes]) -> str:
    return proc.stdout.decode("utf-8", "replace").strip()


def require(proc: subprocess.CompletedProcess[bytes], label: str) -> str:
    if proc.returncode != 0:
        raise RuntimeError(
            f"{label} exit {proc.returncode}: {proc.stderr.decode('utf-8', 'replace')[:500]}"
        )
    return text_of(proc)


def norm_amount(value: str) -> str:
    # numeric(76,10) has more significant digits than Decimal's default precision.
    with decimal.localcontext() as ctx:
        ctx.prec = 96
        quant = Decimal(value.strip()).quantize(Decimal("0.0000000001"))
        return format(quant, "f")


def cells(rows: list[dict]) -> list[tuple]:
    body = []
    for row in rows:
        note = row.get("note")
        if isinstance(note, bytes):
            note_text = note.hex()
        else:
            note_text = None
        body.append(
            (
                str(row["reading_id"]),
                str(row["ts"]),
                norm_amount(str(row["amount"])),
                note_text,
                str(row["qty"]),
            )
        )
    return sorted(body)


class LiveSource:
    def __init__(self) -> None:
        self.statements: list[str] = []
        self.arrow_bytes = b""
        self.arrow_files: list[str] = []
        self.query_seq = 0

    def _read(self, sql: str, partition: str = "") -> str:
        self.statements.append(sql)
        proc = ch(READER, READER_PASSWORD, sql, next_id("oo-dl01w1-40-c4d-r"), partition)
        return require(proc, "reader")

    def _arrow(self, sql: str, partition: str = "") -> bytes:
        self.statements.append(sql)
        query_id = next_id("oo-dl01w1-40-c4d-a")
        proc = ch(READER, READER_PASSWORD, sql, query_id, partition)
        if proc.returncode != 0:
            raise RuntimeError(proc.stderr.decode("utf-8", "replace")[:500])
        label = partition or "all"
        directory = OUT / "arrows"
        directory.mkdir(parents=True, exist_ok=True)
        name = f"{query_id}-{label}.arrow"
        (directory / name).write_bytes(proc.stdout)
        self.arrow_files.append(name)
        self.arrow_bytes = proc.stdout
        return proc.stdout

    def inspect(self) -> dict:
        columns_sql = (
            "SELECT name, type FROM system.columns "
            "WHERE database = 'oo_dl01w40' AND table = 'readings' ORDER BY position FORMAT JSONEachRow"
        )
        parts_sql = (
            "SELECT partition, toString(sum(rows)) AS rows, toString(sum(bytes_on_disk)) AS bytes, "
            "toString(max(modification_time)) AS modified, arrayStringConcat(groupArray(name), ',') AS names "
            "FROM system.parts WHERE database = 'oo_dl01w40' AND table = 'readings' AND active "
            "GROUP BY partition ORDER BY partition FORMAT JSONEachRow"
        )
        pending_sql = (
            "SELECT toString(count()) FROM system.mutations "
            "WHERE database = 'oo_dl01w40' AND table = 'readings' AND is_done = 0 FORMAT TSV"
        )
        uuid_sql = (
            "SELECT toString(uuid) FROM system.tables "
            "WHERE database = 'oo_dl01w40' AND name = 'readings' FORMAT TSV"
        )
        columns = [json.loads(line) for line in self._read(columns_sql).splitlines() if line]
        parts = [json.loads(line) for line in self._read(parts_sql).splitlines() if line]
        pending = self._read(pending_sql)
        uuid = self._read(uuid_sql)
        return {
            "uuid": uuid,
            "fingerprint": cap.fingerprint(columns),
            "mutation_pending": pending not in {"0", ""},
            "mutation_is_done": 0 if pending not in {"0", ""} else 1,
            "partitions": [
                {
                    "id": str(item["partition"]),
                    "signature": "|".join(
                        [
                            str(item["partition"]),
                            item["rows"],
                            item["bytes"],
                            item["modified"],
                            ",".join(sorted(part for part in str(item["names"]).split(",") if part)),
                        ]
                    ),
                }
                for item in parts
            ],
        }

    def _rows(self, payload: bytes, partition: str | None) -> list[dict]:
        decoded = arrow_stream.rows_from_stream(payload)
        if partition is None:
            mapping = self._partition_map()
            for item in decoded:
                item["partition_id"] = mapping[str(item["reading_id"])]
        else:
            for item in decoded:
                item["partition_id"] = partition
        return decoded

    def _partition_map(self) -> dict[str, str]:
        sql = (
            "SELECT toString(reading_id), _partition_id FROM oo_dl01w40.readings FINAL "
            "ORDER BY reading_id FORMAT TSV"
        )
        body = self._read(sql)
        found = {}
        for line in body.splitlines():
            ident, part = line.split("\t")
            found[ident] = part
        return found

    def read_partition(self, partition: str) -> list[dict]:
        sql = (
            "SELECT reading_id, ts, amount, note, qty FROM oo_dl01w40.readings FINAL "
            "WHERE _partition_id = {partition:String} ORDER BY reading_id FORMAT ArrowStream"
        )
        return self._rows(self._arrow(sql, partition), partition)

    def read_all(self) -> list[dict]:
        sql = (
            "SELECT reading_id, ts, amount, note, qty FROM oo_dl01w40.readings FINAL "
            "ORDER BY reading_id FORMAT ArrowStream"
        )
        return self._rows(self._arrow(sql), None)

    def count(self) -> int:
        sql = "SELECT toString(count()) FROM oo_dl01w40.readings FINAL FORMAT TSV"
        return int(self._read(sql))


class PgShadow:
    def __init__(self, table: str) -> None:
        self.table = table
        self.generation = 0
        self.open = False
        self.sql: list[str] = []
        self.rolled_back = 0

    def begin(self) -> None:
        self.open = True
        self.sql = ["BEGIN;"]

    def _literal(self, row: dict) -> str:
        if not IDENT.fullmatch(str(row["reading_id"])) or not IDENT.fullmatch(str(row["ts"])):
            raise RuntimeError("identity is not a decimal integer")
        if not IDENT.fullmatch(str(row["qty"])):
            raise RuntimeError("qty is not an integer")
        if not AMOUNT.fullmatch(str(row["amount"])):
            raise RuntimeError("amount is not a decimal")
        if not re.fullmatch(r"^[0-9]{6}$", str(row["partition_id"])):
            raise RuntimeError("partition id is not a token")
        note = row.get("note")
        if note is None:
            note_sql = "NULL"
        elif isinstance(note, bytes):
            note_sql = f"decode('{note.hex()}','hex')"
        else:
            raise RuntimeError("note is not bytes")
        return (
            f"('{row['partition_id']}', {row['reading_id']}, {row['ts']}, "
            f"{row['amount']}, {note_sql}, {row['qty']})"
        )

    def replace_partition(self, partition: str, rows: list[dict]) -> None:
        if not re.fullmatch(r"^[0-9]{6}$", partition):
            raise RuntimeError("partition id is not a token")
        self.sql.append(f"DELETE FROM capture.{self.table} WHERE partition_id = '{partition}';")
        if rows:
            values = ", ".join(self._literal(row) for row in rows)
            self.sql.append(
                f"INSERT INTO capture.{self.table} "
                "(partition_id, reading_id, ts, amount, note, qty) VALUES " + values + ";"
            )

    def replace_all(self, rows: list[dict]) -> None:
        self.sql.append(f"DELETE FROM capture.{self.table};")
        if rows:
            values = ", ".join(self._literal(row) for row in rows)
            self.sql.append(
                f"INSERT INTO capture.{self.table} "
                "(partition_id, reading_id, ts, amount, note, qty) VALUES " + values + ";"
            )

    def upsert(self, rows: list[dict]) -> None:
        for row in rows:
            self.sql.append(f"DELETE FROM capture.{self.table} WHERE reading_id = {row['reading_id']};")
        if rows:
            values = ", ".join(self._literal(row) for row in rows)
            self.sql.append(
                f"INSERT INTO capture.{self.table} "
                "(partition_id, reading_id, ts, amount, note, qty) VALUES " + values + ";"
            )

    def commit(self) -> int:
        self.sql.append("COMMIT;")
        proc = psql("\n".join(self.sql))
        self.open = False
        if proc.returncode != 0:
            raise RuntimeError(proc.stderr.decode("utf-8", "replace")[:500])
        self.generation += 1
        return self.generation

    def rollback(self) -> None:
        if self.open:
            proc = psql("ROLLBACK;")
            if proc.returncode != 0:
                raise RuntimeError(proc.stderr.decode("utf-8", "replace")[:400])
        self.open = False
        self.sql = []
        self.rolled_back += 1

    def rows(self) -> list[dict]:
        proc = psql(
            "COPY (SELECT partition_id, reading_id::text, ts::text, amount::text, "
            f"CASE WHEN note IS NULL THEN NULL ELSE encode(note, 'hex') END, qty::text "
            f"FROM capture.{self.table} ORDER BY reading_id) TO STDOUT"
        )
        body = require(proc, "shadow copy")
        found = []
        for line in body.splitlines():
            part, ident, ts, amount, note, qty = line.split("\t")
            found.append(
                {
                    "partition_id": part,
                    "reading_id": int(ident),
                    "ts": int(ts),
                    "amount": norm_amount(amount),
                    "note": None if note == r"\N" else bytes.fromhex(note),
                    "qty": int(qty),
                }
            )
        return found


def dba(sql: str, query_id: str, timeout: int = 90) -> dict:
    try:
        proc = ch(DBA, DBA_PASSWORD, sql, query_id, timeout=timeout)
    except subprocess.TimeoutExpired:
        return {"query_id": query_id, "exit": 124, "stdout": "", "stderr": "timeout"}
    return {
        "query_id": query_id,
        "exit": proc.returncode,
        "stdout": proc.stdout.decode("utf-8", "replace")[:400],
        "stderr": proc.stderr.decode("utf-8", "replace")[:400],
    }


def wait_ready() -> None:
    deadline = time.time() + 90
    while time.time() < deadline:
        ping = ch(DBA, DBA_PASSWORD, "SELECT 1 FORMAT TSV", next_id("oo-dl01w1-40-c4d-dba-ping"))
        pg = run(["docker", "exec", PG, "pg_isready", "-U", "oo", "-d", "oo_shadow"], timeout=10)
        if ping.returncode == 0 and pg.returncode == 0:
            return
        time.sleep(2)
    raise RuntimeError("clickhouse or postgres did not become ready")


def prepare_schema() -> None:
    sql = """
CREATE SCHEMA IF NOT EXISTS capture;
CREATE TABLE IF NOT EXISTS capture.readings (
  partition_id text NOT NULL,
  reading_id numeric(20,0) NOT NULL,
  ts numeric(20,0) NOT NULL,
  amount numeric(76,10) NOT NULL,
  note bytea,
  qty int8 NOT NULL,
  PRIMARY KEY (reading_id)
);
CREATE TABLE IF NOT EXISTS capture.readings_c5 (LIKE capture.readings INCLUDING ALL);
TRUNCATE capture.readings, capture.readings_c5;
"""
    require(psql(sql), "schema")


def oracle_rows() -> list[dict]:
    sql = (
        "SELECT toString(reading_id), toString(toUnixTimestamp64Nano(ts)), toString(amount), "
        "if(isNull(note), NULL, hex(note)), toString(qty), _partition_id "
        "FROM oo_dl01w40.readings FINAL ORDER BY reading_id FORMAT TSV"
    )
    proc = ch(READER, READER_PASSWORD, sql, next_id("oo-dl01w1-40-c4d-oracle"))
    body = require(proc, "oracle")
    rows = []
    for line in body.splitlines():
        ident, ts, amount, note, qty, part = line.split("\t")
        rows.append(
            {
                "reading_id": int(ident),
                "ts": int(ts),
                "amount": norm_amount(amount),
                "note": None if note == r"\N" else bytes.fromhex(note),
                "qty": int(qty),
                "partition_id": part,
            }
        )
    return rows


def product_writes() -> dict:
    dba("SYSTEM FLUSH LOGS", "oo-dl01w1-40-c4d-dba-flush")
    sql = (
        "SELECT query_kind, type, toString(count()) AS n FROM system.query_log "
        "WHERE user = 'oo_dl01w1_40_reader' AND query_id LIKE 'oo-dl01w1-40-c4d-%' "
        "GROUP BY query_kind, type ORDER BY query_kind, type FORMAT JSONEachRow"
    )
    proc = ch(DBA, DBA_PASSWORD, sql, "oo-dl01w1-40-c4d-dba-audit")
    body = require(proc, "audit")
    rows = [json.loads(line) for line in body.splitlines() if line]
    write_count = 0
    for item in rows:
        if item["query_kind"] in WRITE_KINDS:
            write_count += int(item["n"])
    return {"rows": rows, "write_count": write_count}


def schedules() -> list[dict]:
    found = []
    for every in ("3s", "7s"):
        start = time.monotonic()
        cap.observe_period(every, time.sleep, time.monotonic)
        found.append({"every": every, "slept_seconds": time.monotonic() - start})
    return found


def base_capture(every: str) -> dict:
    return {
        "connector": "clickhouse",
        "sync": "partition_refresh",
        "every": every,
        "consistency": "per_table",
        "credential": "secret://oo-dl01w1-40/reader",
        "locale": "en-US",
        "budget": {"unit": "Rows", "hard_limit": 100000},
        "window": None,
        "capabilities": ["C4", "C5", "Catalog", "BatchFull", "SnapshotToken"],
        "license": "not_required",
        "cdc": None,
        "delete_detection": {"mode": "partition_replace"},
        "schema_columns": SCHEMA,
        "untrusted_partition_metadata": False,
        "object": {"database": "oo_dl01w40", "table": "readings"},
        "fields": FIELDS,
    }


def compare(shadow: PgShadow, label: str) -> dict:
    left = cells(shadow.rows())
    right = cells(oracle_rows())
    return {
        "label": label,
        "shadow_count": len(left),
        "source_count": len(right),
        "diff": 0 if left == right else 1,
        "shadow": left,
        "source": right,
    }


def main() -> int:
    OUT.mkdir(parents=True, exist_ok=True)
    customer = json.loads(CUSTOMER.read_text())
    secret = os.environ["OO_C4_SECRET"]
    plan = cap.gate(customer, base_capture("3s"), secret, Path(secret).is_file())
    (OUT / "plan.json").write_text(json.dumps(plan, indent=2) + "\n")
    if not plan["admitted"]:
        (OUT / "result.json").write_text(json.dumps({"published": False, "plan": plan}, indent=2) + "\n")
        return 2
    periods = schedules()
    (OUT / "schedules.json").write_text(json.dumps(periods, indent=2) + "\n")
    wait_ready()
    dba("SYSTEM START MERGES", "oo-dl01w1-40-c4d-dba-start-all", timeout=30)
    dropped = dba("DROP DATABASE IF EXISTS oo_dl01w40 SYNC", "oo-dl01w1-40-c4d-dba-drop", timeout=120)
    (OUT / "dba-drop.json").write_text(json.dumps(dropped, indent=2) + "\n")
    fixture = dba(FIXTURE.read_text(), "oo-dl01w1-40-c4d-dba-fixture")
    (OUT / "dba-fixture.json").write_text(json.dumps(fixture, indent=2) + "\n")
    if fixture["exit"] != 0:
        return 3
    prepare_schema()
    moment = datetime.now(timezone.utc)
    source = LiveSource()
    shadow = PgShadow("readings")
    steps = []

    def remember(item: dict) -> None:
        steps.append(item)
        (OUT / "steps.json").write_text(json.dumps(steps, indent=2, default=str) + "\n")
    first = cap.cycle(plan, source, shadow, None, moment)
    first_public = {key: value for key, value in first.items() if key != "rows"}
    first_public["arrow_files"] = list(source.arrow_files)
    first_public["cell_diff"] = compare(shadow, "baseline")
    first_public["row_count"] = len(first.get("rows") or [])
    remember(first_public)
    if not first.get("published") or first_public["cell_diff"]["diff"] != 0:
        (OUT / "steps.json").write_text(json.dumps(steps, indent=2) + "\n")
        return 4
    unchanged = cap.cycle(plan, source, shadow, first["checkpoint"], moment)
    remember({"reason": unchanged.get("reason"), "replaces": unchanged.get("replaces"), "generation": shadow.generation})
    c5_plan = cap.gate(
        customer,
        base_capture("7s") | {"sync": "full", "delete_detection": {"mode": "reconcile"}, "untrusted_partition_metadata": True},
        secret,
        True,
    )
    c5_shadow = PgShadow("readings_c5")
    c5_source = LiveSource()
    c5 = cap.cycle(c5_plan, c5_source, c5_shadow, None, moment)
    c5_view = compare(c5_shadow, "c5")
    remember(
        {
            "method": c5.get("method"),
            "published": c5.get("published"),
            "cell_diff": c5_view,
            "arrow_files": list(c5_source.arrow_files),
        }
    )
    inserted = dba(
        "INSERT INTO oo_dl01w40.readings "
        "(reading_id, version, amount, note, code, code_or_null, ts, tags, labels, qty, label, wide_u64) "
        "VALUES (104, 1, CAST('4.0000000000' AS Decimal(76, 10)), 'new', 'n', 'n', "
        "toDateTime64('2024-02-15 00:00:00', 9, 'UTC'), CAST([] AS Array(Int64)), CAST([] AS Array(Nullable(String))), 4, 'new', toUInt64(4))",
        "oo-dl01w1-40-c4d-dba-insert",
    )
    insert_checkpoint = first["checkpoint"] if unchanged.get("reason") == "unchanged" else unchanged.get("checkpoint")
    after_insert = cap.cycle(plan, source, shadow, insert_checkpoint, moment)
    remember(
        {
            "dba_insert": inserted,
            "published": after_insert.get("published"),
            "cell_diff": compare(shadow, "insert"),
            "count": len(after_insert.get("rows") or []),
            "arrow_files": list(source.arrow_files),
        }
    )
    updated = dba(
        "INSERT INTO oo_dl01w40.readings "
        "(reading_id, version, amount, note, code, code_or_null, ts, tags, labels, qty, label, wide_u64) "
        "VALUES (101, 3, CAST('8.0000000000' AS Decimal(76, 10)), 'upd', 'u', 'u', "
        "toDateTime64('2024-03-15 12:34:56.123456789', 9, 'UTC'), [1], ['u'], 8, 'upd', toUInt64(8))",
        "oo-dl01w1-40-c4d-dba-update",
    )
    checkpoint = after_insert.get("checkpoint")
    after_update = cap.cycle(plan, source, shadow, checkpoint, moment)
    remember(
        {
            "dba_update": updated,
            "published": after_update.get("published"),
            "cell_diff": compare(shadow, "update"),
            "arrow_files": list(source.arrow_files),
        }
    )
    dba("SYSTEM START MERGES oo_dl01w40.readings", "oo-dl01w1-40-c4d-dba-start-before-delete")
    inflight = dba(
        "DELETE FROM oo_dl01w40.readings WHERE reading_id = 102 "
        "SETTINGS lightweight_deletes_sync = 0, mutations_sync = 0",
        "oo-dl01w1-40-c4d-dba-delete-async",
        timeout=45,
    )
    samples = []
    pending_result = None
    for _ in range(15):
        probe = LiveSource()
        meta = probe.inspect()
        samples.append({"pending": meta["mutation_pending"], "is_done": meta["mutation_is_done"]})
        if meta["mutation_pending"] and pending_result is None:
            pending_result = cap.cycle(plan, probe, shadow, after_update.get("checkpoint"), moment)
            break
        if not meta["mutation_pending"]:
            break
        time.sleep(1)
    if pending_result is None and samples and samples[-1]["pending"]:
        dba("SYSTEM START MERGES oo_dl01w40.readings", "oo-dl01w1-40-c4d-dba-start-merges")
    caught = cap.cycle(plan, source, shadow, after_update.get("checkpoint"), moment)
    if not caught.get("published"):
        dba("SYSTEM START MERGES oo_dl01w40.readings", "oo-dl01w1-40-c4d-dba-start-merges-2")
        for _ in range(20):
            meta = LiveSource().inspect()
            samples.append({"pending": meta["mutation_pending"], "is_done": meta["mutation_is_done"]})
            if not meta["mutation_pending"]:
                break
            time.sleep(1)
        caught = cap.cycle(plan, source, shadow, after_update.get("checkpoint"), moment)
    dba("SYSTEM STOP MERGES oo_dl01w40.readings", "oo-dl01w1-40-c4d-dba-stop-merges")
    settled = cap.cycle(plan, source, shadow, caught.get("checkpoint"), moment)
    settle_point = settled.get("checkpoint") or caught.get("checkpoint")
    replay = cap.cycle(plan, source, shadow, settle_point, moment)
    identities = sorted(int(item[0]) for item in cells(shadow.rows()))
    remember(
        {
            "dba_delete": inflight,
            "mutation_samples": samples,
            "pending_cycle": None if pending_result is None else pending_result.get("reason"),
            "caught": caught.get("published"),
            "cell_diff": compare(shadow, "delete"),
            "replay_reason": replay.get("reason"),
            "identities": identities,
            "deleted_absent": 102 not in identities,
            "arrow_files": list(source.arrow_files),
        }
    )

    def cancel() -> None:
        raise cap.Cancelled()

    source.signatures_bump = True
    # Force a change by a DBA insert of a higher version that we then interrupt before commit.
    dba(
        "INSERT INTO oo_dl01w40.readings "
        "(reading_id, version, amount, note, code, code_or_null, ts, tags, labels, qty, label, wide_u64) "
        "VALUES (104, 2, CAST('5.0000000000' AS Decimal(76, 10)), 'x', 'n', 'n', "
        "toDateTime64('2024-02-15 00:00:00', 9, 'UTC'), CAST([] AS Array(Int64)), CAST([] AS Array(Nullable(String))), 5, 'x', toUInt64(5))",
        "oo-dl01w1-40-c4d-dba-interrupt-src",
    )
    before_rows = cells(shadow.rows())
    interrupted = cap.cycle(plan, source, shadow, settle_point, moment, before_commit=cancel)
    after_rows = cells(shadow.rows())
    killer = subprocess.Popen(
        [
            "docker", "exec", PG, "psql", "-U", "oo", "-d", "oo_shadow", "-c",
            "BEGIN; DELETE FROM capture.readings WHERE partition_id = '202401'; SELECT pg_sleep(30);",
        ],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
    )
    visible = "0"
    activity = ""
    # The observation query contains the letters pg_sleep, so it matches itself.
    # Count only other backends.
    visible_sql = (
        "SELECT count(*) FROM pg_stat_activity "
        "WHERE pid <> pg_backend_pid() AND state = 'active' AND position('pg_sleep' in query) > 0"
    )
    activity_sql = (
        "SELECT pid::text || ' ' || state || ' ' || left(query, 160) "
        "FROM pg_stat_activity WHERE state = 'active' AND position('pg_sleep' in query) > 0 ORDER BY pid"
    )
    deadline = time.time() + 8
    while time.time() < deadline:
        active = psql(visible_sql)
        visible = text_of(active).strip()
        activity = text_of(psql(activity_sql)).strip()
        if visible.isdigit() and int(visible) >= 1:
            break
        time.sleep(0.4)
    killer.kill()
    killer.wait(timeout=10)
    killed_rows = cells(shadow.rows())
    remember(
        {
            "interrupted": interrupted.get("reason"),
            "rolled_back": interrupted.get("rolled_back"),
            "shadow_unchanged": before_rows == after_rows == killed_rows,
            "kill_exit": killer.returncode,
            "sleep_visible": visible,
            "sleep_activity": activity,
        }
    )
    added = dba(
        "ALTER TABLE oo_dl01w40.readings ADD COLUMN IF NOT EXISTS extra_c4 String DEFAULT ''",
        "oo-dl01w1-40-c4d-dba-add-column",
    )
    schema_samples: list[dict] = []
    observed_fingerprint = ""
    for _ in range(20):
        meta = LiveSource().inspect()
        observed_fingerprint = str(meta["fingerprint"])
        schema_samples.append({"pending": meta["mutation_pending"], "is_done": meta["mutation_is_done"]})
        if not meta["mutation_pending"]:
            break
        time.sleep(1)
    if schema_samples and schema_samples[-1]["pending"]:
        dba("SYSTEM START MERGES oo_dl01w40.readings", "oo-dl01w1-40-c4d-dba-start-merges-schema")
        for _ in range(20):
            meta = LiveSource().inspect()
            observed_fingerprint = str(meta["fingerprint"])
            schema_samples.append({"pending": meta["mutation_pending"], "is_done": meta["mutation_is_done"]})
            if not meta["mutation_pending"]:
                break
            time.sleep(1)
    paused = cap.cycle(plan, LiveSource(), shadow, caught.get("checkpoint"), moment)
    fingerprint_differs = observed_fingerprint != plan["fingerprint"]
    remember(
        {
            "add_column": added,
            "mutation_samples": schema_samples,
            "reason": paused.get("reason"),
            "published": paused.get("published"),
            "fingerprint_differs": fingerprint_differs,
        }
    )
    writes = product_writes()
    (OUT / "product-writes.json").write_text(json.dumps(writes, indent=2) + "\n")
    (OUT / "steps.json").write_text(json.dumps(steps, indent=2, default=str) + "\n")
    ok = (
        writes["write_count"] == 0
        and steps[0]["cell_diff"]["diff"] == 0
        and steps[1]["reason"] == "unchanged"
        and steps[2]["cell_diff"]["diff"] == 0
        and steps[3]["cell_diff"]["diff"] == 0
        and steps[3]["count"] == 4
        and steps[4]["cell_diff"]["diff"] == 0
        and steps[5]["deleted_absent"]
        and steps[5]["cell_diff"]["diff"] == 0
        and steps[5]["replay_reason"] == "unchanged"
        and steps[6]["rolled_back"] is True
        and steps[6]["shadow_unchanged"] is True
        and steps[6]["sleep_visible"].isdigit()
        and int(steps[6]["sleep_visible"]) >= 1
        and steps[7]["published"] is False
        and steps[7]["fingerprint_differs"] is True
        and steps[7]["reason"] in {"schema_changed", "mutation_pending"}
        and abs(periods[0]["slept_seconds"] - 3) < 2
        and abs(periods[1]["slept_seconds"] - 7) < 2
        and abs(periods[0]["slept_seconds"] - periods[1]["slept_seconds"]) > 2
    )
    (OUT / "result.json").write_text(json.dumps({"ok": ok, "write_count": writes["write_count"]}, indent=2) + "\n")
    return 0 if ok else 5


if __name__ == "__main__":
    try:
        code = main()
    except Exception as exc:
        OUT.mkdir(parents=True, exist_ok=True)
        (OUT / "error.txt").write_text(f"{type(exc).__name__}: {exc}\n")
        raise
    raise SystemExit(code)
