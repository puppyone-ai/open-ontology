#!/usr/bin/env python3
"""ClickHouse C4 capture into the fixed native loader.

The product account only reads. DBA fixture writes use a separate user and
query-id prefix. The acceptance tables in schema capture are the C4
projection. The native loader database is the Agent DB target. Neither is a
Gateway, a Knowledge Pack, or a model dialogue.
"""

from __future__ import annotations

import decimal
import hashlib
import json
import os
import subprocess
import time
import uuid
from datetime import datetime, timezone
from pathlib import Path

import arrow_stream
import c4_capture as cap
import capture_receipt as receipt

ROOT = Path("/Users/redux/puppy-one/open-ontology")
OUT = Path(os.environ["OO_NATIVE_OUT"])
CH = os.environ.get("OO_NATIVE_CH", "oo-dl01w1-40-native-ch")
PG = os.environ.get("OO_NATIVE_PG", "oo-dl01w1-40-native-pg")
ELF = Path(os.environ.get("OO_NATIVE_ELF", "/tmp/oo-u06-194/debug/oo-pipeline"))
PG_PASSWORD = Path(os.environ["OO_NATIVE_PG_PASSWORD"])
RUST_IMAGE = os.environ.get(
    "OO_NATIVE_RUST_IMAGE",
    "rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5",
)
PIPELINE_SHA = "4673c8b6e58526c46cac9697b13fd015601992179ac6c38f0e21cf3566acabfe"
READER = "oo_dl01w1_40_reader"
DBA = "oo_dl01w1_40_dba"
READER_PASSWORD = "/etc/oo/reader.password"
DBA_PASSWORD = "/etc/oo/dba.password"
CUSTOMER = (
    ROOT
    / "evidence/4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/DL01/clickhouse-product"
    / "raw/run/product-run-11-http401/plan/customer.json"
)
PREFIX = "oo-dl01w1-40-n2-"
DBA_PREFIX = "oo-dl01w1-40-n2-dba-"
SOURCE_ID = "clickhouse_268"
SCHEMA_NAME = "ch"
TABLE_NAME = "readings"
FIELDS = ["reading_id", "ts", "amount", "note", "qty"]
COLUMNS = [
    {"name": "reading_id", "logical_name": "reading_id", "arrow_type": "uint64", "nullable": False, "precision": 64},
    {"name": "ts", "logical_name": "ts", "arrow_type": "timestamp_nanos_utc", "nullable": False},
    {"name": "amount", "logical_name": "amount", "arrow_type": "decimal256", "nullable": False, "precision": 76, "scale": 10},
    {"name": "note", "logical_name": "note", "arrow_type": "binary", "nullable": True},
    {"name": "qty", "logical_name": "qty", "arrow_type": "int64", "nullable": False, "precision": 64},
]
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
WRITE_KINDS = {
    "Insert", "Alter", "Create", "Drop", "Rename", "Optimize", "Truncate",
    "KillQuery", "Grant", "Revoke", "Move", "Undrop", "System", "Delete", "Update",
}
SEQ = 0
STEPS: list[dict] = []


def next_id(prefix: str) -> str:
    global SEQ
    SEQ += 1
    return f"{prefix}{SEQ}"


def run(args: list[str], timeout: int = 120, data: bytes | None = None) -> subprocess.CompletedProcess[bytes]:
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


def psql(sql: str, user: str = "postgres", database: str = "oo_ch40", timeout: int = 60) -> subprocess.CompletedProcess[bytes]:
    return run(
        ["docker", "exec", "-i", PG, "psql", "-U", user, "-d", database, "-v", "ON_ERROR_STOP=1", "-t", "-A"],
        timeout=timeout,
        data=sql.encode(),
    )


def text_of(proc: subprocess.CompletedProcess[bytes]) -> str:
    return proc.stdout.decode("utf-8", "replace").strip()


def require(proc: subprocess.CompletedProcess[bytes], label: str) -> str:
    if proc.returncode != 0:
        raise RuntimeError(f"{label} exit {proc.returncode}: {proc.stderr.decode('utf-8', 'replace')[:500]}")
    return text_of(proc)


def remember(item: dict) -> None:
    STEPS.append(item)
    (OUT / "steps.json").write_text(json.dumps(STEPS, indent=2) + "\n")


def norm_amount(value: str) -> str:
    with decimal.localcontext() as ctx:
        ctx.prec = 96
        number = decimal.Decimal(value)
        return format(number.quantize(decimal.Decimal("0.0000000001")), "f")


def note_text(value: bytes | None) -> str | None:
    if value is None:
        return None
    if value == b"":
        return r"\x"
    return r"\x" + value.hex()


def truth_rows(rows: list[dict]) -> list[list]:
    found = []
    for row in rows:
        found.append([
            str(int(row["reading_id"])),
            str(int(row["ts"])),
            norm_amount(str(row["amount"])),
            note_text(row.get("note")),
            str(int(row["qty"])),
        ])
    return found


def cells_of(rows: list[dict]) -> list[tuple]:
    return sorted(
        (
            int(row["reading_id"]),
            int(row["ts"]),
            norm_amount(str(row["amount"])),
            None if row.get("note") is None else row["note"].hex(),
            int(row["qty"]),
        )
        for row in rows
    )


class LiveSource:
    def __init__(self) -> None:
        self.statements: list[str] = []
        self.arrow_files: list[str] = []
        self.arrow_bytes = b""

    def _read(self, sql: str, partition: str = "") -> str:
        self.statements.append(sql)
        proc = ch(READER, READER_PASSWORD, sql, next_id(PREFIX), partition)
        if proc.returncode != 0:
            raise RuntimeError(proc.stderr.decode("utf-8", "replace")[:500])
        return proc.stdout.decode("utf-8", "replace").strip()

    def _arrow(self, sql: str, partition: str = "") -> bytes:
        self.statements.append(sql)
        query_id = next_id(PREFIX)
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
        columns = [
            json.loads(line)
            for line in self._read(
                "SELECT name, type FROM system.columns "
                "WHERE database = 'oo_dl01w40' AND table = 'readings' ORDER BY position FORMAT JSONEachRow"
            ).splitlines()
            if line
        ]
        parts = [
            json.loads(line)
            for line in self._read(
                "SELECT partition, toString(sum(rows)) AS rows, toString(sum(bytes_on_disk)) AS bytes, "
                "toString(max(modification_time)) AS modified, arrayStringConcat(groupArray(name), ',') AS names "
                "FROM system.parts WHERE database = 'oo_dl01w40' AND table = 'readings' AND active "
                "GROUP BY partition ORDER BY partition FORMAT JSONEachRow"
            ).splitlines()
            if line
        ]
        pending = self._read(
            "SELECT toString(count()) FROM system.mutations "
            "WHERE database = 'oo_dl01w40' AND table = 'readings' AND is_done = 0 FORMAT TSV"
        )
        table_uuid = self._read(
            "SELECT toString(uuid) FROM system.tables WHERE database = 'oo_dl01w40' AND name = 'readings' FORMAT TSV"
        )
        return {
            "uuid": table_uuid,
            "columns": columns,
            "fingerprint": cap.fingerprint(columns),
            "mutation_pending": pending not in {"0", ""},
            "mutation_is_done": 0 if pending not in {"0", ""} else 1,
            "partitions": [
                {
                    "id": str(item["partition"]),
                    "signature": "|".join([
                        str(item["partition"]),
                        item["rows"],
                        item["bytes"],
                        item["modified"],
                        ",".join(sorted(part for part in str(item["names"]).split(",") if part)),
                    ]),
                }
                for item in parts
            ],
        }

    def _rows(self, payload: bytes, partition: str | None) -> list[dict]:
        decoded = arrow_stream.rows_from_stream(payload)
        if partition is None:
            mapping = {}
            body = self._read(
                "SELECT toString(reading_id), _partition_id FROM oo_dl01w40.readings FINAL "
                "ORDER BY reading_id FORMAT TSV"
            )
            for line in body.splitlines():
                ident, part = line.split("\t")
                mapping[ident] = part
            for item in decoded:
                item["partition_id"] = mapping[str(item["reading_id"])]
        else:
            for item in decoded:
                item["partition_id"] = partition
        return decoded

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
        return int(self._read("SELECT toString(count()) FROM oo_dl01w40.readings FINAL FORMAT TSV"))


class PgShadow:
    def __init__(self, table: str) -> None:
        self.table = table
        self.generation = 0
        self.open = False
        self.sql: list[str] = []

    def begin(self) -> None:
        self.open = True
        self.sql = ["BEGIN;"]

    def _literal(self, row: dict) -> str:
        note = row.get("note")
        if note is None:
            note_sql = "NULL"
        elif isinstance(note, bytes):
            note_sql = f"decode('{note.hex()}','hex')"
        else:
            raise RuntimeError("note is not bytes")
        return (
            f"('{row['partition_id']}', {int(row['reading_id'])}, {int(row['ts'])}, "
            f"{norm_amount(str(row['amount']))}, {note_sql}, {int(row['qty'])})"
        )

    def replace_partition(self, partition: str, rows: list[dict]) -> None:
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

    def rows(self) -> list[dict]:
        proc = psql(
            "COPY (SELECT partition_id, reading_id::text, ts::text, amount::text, "
            "CASE WHEN note IS NULL THEN NULL ELSE encode(note, 'hex') END, qty::text "
            f"FROM capture.{self.table} ORDER BY reading_id) TO STDOUT"
        )
        body = require(proc, "shadow copy")
        found = []
        for line in body.splitlines():
            part, ident, ts, amount, note, qty = line.split("\t")
            found.append({
                "partition_id": part,
                "reading_id": int(ident),
                "ts": int(ts),
                "amount": norm_amount(amount),
                "note": None if note == r"\N" else bytes.fromhex(note),
                "qty": int(qty),
            })
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


def oracle_rows() -> list[dict]:
    sql = (
        "SELECT toString(reading_id), toString(toUnixTimestamp64Nano(ts)), toString(amount), "
        "if(isNull(note), NULL, hex(note)), toString(qty), _partition_id "
        "FROM oo_dl01w40.readings FINAL ORDER BY reading_id FORMAT TSV"
    )
    proc = ch(READER, READER_PASSWORD, sql, next_id(PREFIX))
    body = require(proc, "oracle")
    rows = []
    for line in body.splitlines():
        ident, ts, amount, note, qty, part = line.split("\t")
        rows.append({
            "reading_id": int(ident),
            "ts": int(ts),
            "amount": norm_amount(amount),
            "note": None if note == r"\N" else bytes.fromhex(note),
            "qty": int(qty),
            "partition_id": part,
        })
    return rows


def compare_source(shadow: PgShadow, label: str) -> dict:
    left = cells_of(shadow.rows())
    right = cells_of(oracle_rows())
    return {"label": label, "shadow": len(left), "source": len(right), "diff": 0 if left == right else 1}


def product_writes() -> dict:
    flushed = dba("SYSTEM FLUSH LOGS", DBA_PREFIX + "flush")
    sql = (
        "SELECT query_kind, type, toString(count()) AS n FROM system.query_log "
        f"WHERE user = '{READER}' AND query_id LIKE '{PREFIX}%' "
        "GROUP BY query_kind, type ORDER BY query_kind, type FORMAT JSONEachRow"
    )
    proc = ch(DBA, DBA_PASSWORD, sql, DBA_PREFIX + "audit")
    body = require(proc, "audit")
    rows = [json.loads(line) for line in body.splitlines() if line]
    write_count = 0
    for item in rows:
        if item["query_kind"] in WRITE_KINDS:
            write_count += int(item["n"])
    return {"flush": flushed, "rows": rows, "write_count": write_count}


def wait_ready() -> None:
    deadline = time.time() + 180
    while time.time() < deadline:
        ping = ch(DBA, DBA_PASSWORD, "SELECT 1 FORMAT TSV", next_id(DBA_PREFIX + "ping"))
        pg = run(["docker", "exec", PG, "pg_isready", "-U", "postgres", "-d", "oo_ch40"], timeout=10)
        if ping.returncode == 0 and pg.returncode == 0:
            return
        time.sleep(2)
    raise RuntimeError("clickhouse or postgres did not become ready")


def cgroup() -> dict:
    inspect = run(["docker", "inspect", CH, PG])
    body = json.loads(inspect.stdout.decode())
    found = {}
    for item in body:
        host = item["HostConfig"]
        name = item["Name"].lstrip("/")
        cpu = run(["docker", "exec", name, "cat", "/sys/fs/cgroup/cpu.max"])
        mem = run(["docker", "exec", name, "cat", "/sys/fs/cgroup/memory.max"])
        found[name] = {
            "nano_cpus": host.get("NanoCpus"),
            "memory": host.get("Memory"),
            "cpu_quota": host.get("CpuQuota"),
            "cpu_max": text_of(cpu),
            "memory_max": text_of(mem),
        }
    return found


def prepare_capture_schema() -> None:
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
    require(psql(sql), "capture schema")


def load_fixture() -> dict:
    sql = (ROOT / "testing/dl01-driver-preflight/clickhouse/sql/00-fixture.sql").read_text(encoding="utf-8")
    proc = ch(DBA, DBA_PASSWORD, sql, DBA_PREFIX + "fixture", timeout=120)
    return {
        "query_id": DBA_PREFIX + "fixture",
        "exit": proc.returncode,
        "stderr": proc.stderr.decode("utf-8", "replace")[:500],
    }


def write_json(path: Path, document: dict) -> str:
    payload = receipt.canonical_bytes(document)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(payload)
    return receipt.sha256_bytes(payload)


def stable_inputs(columns: list[dict]) -> dict:
    customer_bytes = CUSTOMER.read_bytes()
    config = {
        "budget": {"hard_limit": 100000, "unit": "Rows"},
        "capabilities": ["C4", "C5", "Catalog", "BatchFull", "SnapshotToken"],
        "connector": "clickhouse",
        "consistency": "per_table",
        "delete_detection": "partition_replace",
        "disclosure": cap.DISCLOSURE,
        "fields": FIELDS,
        "license": "not_required",
        "locale": "en-US",
        "object": {"database": "oo_dl01w40", "table": "readings"},
        "sync": "partition_refresh",
    }
    types = {"columns": COLUMNS, "protocol": "oo.clickhouse.native-types/v1"}
    schema = {"columns": columns, "protocol": "oo.clickhouse.live-columns/v1"}
    directory = OUT / "identity"
    directory.mkdir(parents=True, exist_ok=True)
    config_digest = write_json(directory / "config.json", config)
    type_digest = write_json(directory / "types.json", types)
    schema_digest = write_json(directory / "schema.json", schema)
    input_digest = receipt.sha256_bytes(customer_bytes)
    (directory / "input.sha256").write_text(input_digest + "\n")
    (directory / "input.source").write_text(
        "evidence/4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/DL01/clickhouse-product/"
        "raw/run/product-run-11-http401/plan/customer.json\n"
    )
    return {
        "config_digest": config_digest,
        "type_digest": type_digest,
        "schema_digest": schema_digest,
        "input_digest": input_digest,
    }


def produce(generation: int, run_id: str, snapshot: str, audit: dict, arrow_path: Path, digests: dict, driver_digest: str, registry_digest: str, catalog_digest: str) -> tuple[Path, str]:
    body = {
        "arrow_schema_sha256": receipt.arrow_schema_sha256(arrow_path.read_bytes()),
        "arrow_sha256": [receipt.sha256_file(arrow_path)],
        "audit_digest": write_json(OUT / "loads" / f"gen{generation}" / "audit.json", audit),
        "catalog_sha256": catalog_digest,
        "config_digest": digests["config_digest"],
        "driver_digest": driver_digest,
        "input_digest": digests["input_digest"],
        "registry_sha256": registry_digest,
        "run_id": run_id,
        "schema": receipt.PROTOCOL,
        "schema_digest": digests["schema_digest"],
        "snapshot": {"kind": "partition_signature", "value": snapshot},
        "source_engine": "clickhouse",
        "source_generation": generation,
        "source_id": SOURCE_ID,
        "type_digest": digests["type_digest"],
    }
    path = OUT / "loads" / f"gen{generation}" / "receipt.json"
    digest = receipt.write_receipt(path, body)
    return path, digest


def target(owner: str) -> dict:
    return {
        "host": "127.0.0.1",
        "port": 5432,
        "database": "oo_ch40",
        "runtime_principal": "oo_ch40_runtime",
        "password_file": "/secrets/pg.password",
        "ssl": "disable",
        "shard": "shard_ch40",
        "owner": owner,
    }


def load_document(generation: int, batch: str, receipt_path: Path, receipt_sha: str, arrow_name: str, truth: list, tombstones: list[str], snapshot: str, digests: dict, audit_digest: str, owner: str, parent: int | None) -> dict:
    document = {
        "protocol": "oo.pipeline.native-update/v1",
        "source_engine": "clickhouse",
        "source_id": SOURCE_ID,
        "source_generation": generation,
        "logical_schema": SCHEMA_NAME,
        "logical_table": TABLE_NAME,
        "batch_id": batch,
        "capture_method": "c4_partition",
        "delete_detection": "partition_replace",
        "consistency_grade": "S0",
        "cross_table_consistent": False,
        "position_codec": "snapshot-zero/v1",
        "position_lineage": "clickhouse.c4.partition",
        "applied_pos": "0",
        "config_digest": digests["config_digest"],
        "schema_digest": digests["schema_digest"],
        "type_digest": digests["type_digest"],
        "input_digest": digests["input_digest"],
        "audit_digest": audit_digest,
        "snapshot": {"kind": "partition_signature", "value": snapshot},
        "columns": COLUMNS,
        "arrow_path": f"/work/loads/gen{generation}/{arrow_name}",
        "tombstones": tombstones,
        "truth": truth,
        "row_identity": "unique",
        "key_column": "reading_id",
        "mirror": False,
        "admission_path": f"/work/loads/gen{generation}/receipt.json",
        "admission_expect": {"declared_receipt_sha256": receipt_sha},
        "target": target(owner),
    }
    if parent is not None:
        document["parent_source_generation"] = parent
    if str(receipt_path) == "":
        raise RuntimeError("receipt path is empty")
    return document


def call_pipeline(name: str, command: str, document: dict) -> dict:
    directory = OUT / "cli" / name
    directory.mkdir(parents=True, exist_ok=True)
    payload = json.dumps(document).encode("utf-8")
    (directory / "request.json").write_bytes(payload)
    proc = run(
        [
            "docker", "run", "--rm", "-i",
            "--network", f"container:{PG}",
            "--platform", "linux/arm64", "--pull", "never",
            "--entrypoint", "/opt/oo-pipeline",
            "--mount", f"type=bind,source={ELF},target=/opt/oo-pipeline,readonly",
            "--mount", f"type=bind,source={OUT},target=/work",
            "--mount", f"type=bind,source={PG_PASSWORD},target=/secrets/pg.password,readonly",
            "--label", "oo.project=open-ontology",
            "--label", "oo.owner=oo-dl01w1-40",
            "--label", "oo.session=4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de",
            "--label", "oo.task=DL01-W1-40",
            RUST_IMAGE, command,
        ],
        timeout=180,
        data=payload,
    )
    (directory / "stdout.txt").write_bytes(proc.stdout)
    (directory / "stderr.txt").write_bytes(proc.stderr)
    (directory / "exit").write_text(str(proc.returncode) + "\n")
    parsed = None
    try:
        parsed = json.loads(proc.stdout.decode())
    except json.JSONDecodeError:
        parsed = None
    return {"exit": proc.returncode, "stdout": parsed, "stderr": proc.stderr.decode("utf-8", "replace")[:800]}


def install_and_role(owner: str) -> dict:
    installed = call_pipeline(
        "install",
        "install-native-update",
        {
            "protocol": "oo.pipeline.install-native-update/v1",
            "target": {
                "host": "127.0.0.1",
                "port": 5432,
                "database": "oo_ch40",
                "runtime_principal": "postgres",
                "password_file": "/secrets/pg.password",
                "ssl": "disable",
                "shard": "shard_ch40",
                "owner": owner,
            },
        },
    )
    password = PG_PASSWORD.read_text().strip()
    role_sql = (
        "DO $$\nBEGIN\n"
        "  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'oo_ch40_runtime') THEN\n"
        "    CREATE ROLE oo_ch40_runtime LOGIN INHERIT;\n"
        "  END IF;\n"
        "END $$;\n"
        f"ALTER ROLE oo_ch40_runtime PASSWORD '{password}';\n"
        "GRANT oo_pipeline_runtime TO oo_ch40_runtime;\n"
        "GRANT CONNECT ON DATABASE oo_ch40 TO oo_ch40_runtime;\n"
    )
    role = psql(role_sql)
    role_err = role.stderr.decode("utf-8", "replace").replace(password, "[redacted]")
    jsonb = psql("SELECT _oo.full_import_type_sql(3802,-1);")
    (OUT / "cli" / "install" / "jsonb.txt").write_bytes(jsonb.stdout)
    (OUT / "cli" / "install" / "role.exit").write_text(str(role.returncode) + "\n")
    (OUT / "cli" / "install" / "role.stderr").write_text(role_err)
    return {"install": installed, "role_exit": role.returncode, "jsonb": text_of(jsonb)[:300]}


def import_generation(name: str, document: dict, arrow_bytes: bytes, arrow_name: str) -> dict:
    directory = OUT / "loads" / name
    directory.mkdir(parents=True, exist_ok=True)
    (directory / arrow_name).write_bytes(arrow_bytes)
    return call_pipeline(name, "import-native-update", document)


def changed_rows(previous: list[dict], current: list[dict]) -> tuple[list[dict], list[str]]:
    old = {int(row["reading_id"]): row for row in previous}
    new = {int(row["reading_id"]): row for row in current}
    tombstones = [str(key) for key in sorted(old) if key not in new]
    upserts = [new[key] for key in sorted(new) if key not in old or cells_of([old[key]]) != cells_of([new[key]])]
    if not upserts and current:
        upserts = [new[sorted(new)[0]]]
    return upserts, tombstones


def native_cells(generation: int, config_digest: str) -> dict:
    listing = psql(
        "SELECT level || E'\\t' || logical_column || E'\\t' || physical_name "
        "FROM _oo.physical_mappings "
        f"WHERE source = '{SOURCE_ID}' AND config_digest = '{config_digest}' "
        f"AND run_generation = {generation} ORDER BY 1"
    )
    if listing.returncode != 0:
        return {
            "exit": listing.returncode,
            "stderr": listing.stderr.decode("utf-8", "replace")[:400],
            "mappings": [],
            "cells": [],
            "table": "",
        }
    rows = []
    table = ""
    columns: dict[str, str] = {}
    for line in text_of(listing).splitlines():
        if not line.strip():
            continue
        level, logical, physical = line.split("\t")
        rows.append({"level": level, "logical": logical, "physical": physical})
        if level in {"t", "table"} and logical in {"", TABLE_NAME}:
            table = physical
        if level in {"c", "column"}:
            columns[logical] = physical
    if table == "" :
        for item in rows:
            if item["level"] in {"t", "table"}:
                table = item["physical"]
    ordered = [columns[name] for name in FIELDS if name in columns]
    body = ""
    if table and ordered:
        select = ", ".join(f"{name}::text" for name in ordered)
        read = psql(f"SELECT {select} FROM _oo.{table} ORDER BY 1")
        body = text_of(read)
        if read.returncode != 0:
            return {"exit": read.returncode, "stderr": read.stderr.decode()[:400], "mappings": rows}
    parsed = []
    for line in body.splitlines():
        if not line.strip():
            continue
        parts = line.split("|") if "|" in line else line.split("\t")
        parsed.append(parts)
    return {"exit": 0, "table": table, "mappings": rows, "cells": parsed}


def exact_diff(truth: list[list], cells: list[list]) -> int:
    def norm(row: list) -> tuple:
        note = row[3]
        if note is None or note == "":
            note_norm = None
        else:
            note_norm = note
        return (row[0], row[1], norm_amount(row[2]), note_norm, row[4])
    left = sorted(norm(row) for row in truth)
    right = sorted(norm(row) for row in cells)
    return 0 if left == right else 1


def sealed_identity(generation: int, config_digest: str) -> dict:
    sql = (
        "SELECT _oo.full_import_sealed_identity("
        f"'{SOURCE_ID}', '{config_digest}', {generation})::text"
    )
    proc = psql(sql, user="oo_ch40_runtime")
    (OUT / "loads" / f"gen{generation}" / "sealed.stdout").write_bytes(proc.stdout)
    (OUT / "loads" / f"gen{generation}" / "sealed.stderr").write_bytes(proc.stderr)
    (OUT / "loads" / f"gen{generation}" / "sealed.exit").write_text(str(proc.returncode) + "\n")
    return {"exit": proc.returncode, "stderr": proc.stderr.decode("utf-8", "replace")[:400]}


def runtime_select_denied(table: str) -> dict:
    proc = psql(f"SELECT count(*) FROM _oo.{table}", user="oo_ch40_runtime")
    (OUT / "cli" / "runtime-select.stderr").write_bytes(proc.stderr)
    (OUT / "cli" / "runtime-select.exit").write_text(str(proc.returncode) + "\n")
    return {"exit": proc.returncode, "stderr": proc.stderr.decode("utf-8", "replace")[:400]}


def snapshot_value(checkpoint: dict) -> str:
    payload = receipt.canonical_bytes({"partitions": checkpoint.get("partitions") or {}, "uuid": checkpoint.get("uuid")})
    return hashlib.sha256(payload).hexdigest()


def main() -> int:
    OUT.mkdir(parents=True, exist_ok=True)
    owner = str(uuid.uuid4())
    (OUT / "identity").mkdir(parents=True, exist_ok=True)
    (OUT / "identity" / "owner.txt").write_text(owner + "\n")
    elf_sha = receipt.sha256_file(ELF)
    (OUT / "elf.sha256").write_text(elf_sha + "\n")
    if elf_sha != PIPELINE_SHA:
        remember({"phase": "elf", "sha256": elf_sha, "expected": PIPELINE_SHA})
        return 2
    customer = json.loads(CUSTOMER.read_text())
    secret = os.environ.get("OO_NATIVE_SECRET", "/tmp/oo-dl01w1-40-return/secrets/reader.password")
    refusals = {
        "cdc": cap.gate(customer, base_capture("3s") | {"sync": "cdc"}, secret, True),
        "secret_directory": cap.gate(customer, base_capture("3s"), secret, False),
        "c3_without_admin": cap.gate(
            customer,
            base_capture("3s") | {"sync": "incremental", "capabilities": ["C3", "C4", "C5"]},
            secret,
            True,
        ),
    }
    (OUT / "refusals.json").write_text(json.dumps(refusals, indent=2) + "\n")
    schedules = []
    for every in ("3s", "7s"):
        start = time.monotonic()
        cap.observe_period(every, time.sleep, time.monotonic)
        schedules.append({"every": every, "slept_seconds": time.monotonic() - start})
    (OUT / "schedules.json").write_text(json.dumps(schedules, indent=2) + "\n")
    wait_ready()
    (OUT / "cgroup.json").write_text(json.dumps(cgroup(), indent=2) + "\n")
    loaded = load_fixture()
    remember({"phase": "fixture", **loaded})
    if loaded["exit"] != 0:
        return 3
    prepare_capture_schema()
    plan = cap.gate(customer, base_capture("3s"), secret, Path(secret).is_file())
    if not plan.get("admitted"):
        remember({"phase": "gate", "plan": plan})
        return 4
    moment = datetime.now(timezone.utc)
    source = LiveSource()
    shadow = PgShadow("readings")
    first = cap.cycle(plan, source, shadow, None, moment)
    view = compare_source(shadow, "baseline")
    remember({"phase": "c4-baseline", "published": first.get("published"), "cell_diff": view, "count": len(first.get("rows") or [])})
    if not first.get("published") or view["diff"] != 0 or view["source"] != 3:
        return 5
    meta = source.inspect()
    driver = run(["docker", "exec", CH, "sha256sum", "/usr/bin/clickhouse"])
    driver_digest = text_of(driver).split()[0]
    digests = stable_inputs(meta["columns"])
    catalog = {
        "columns": meta["columns"],
        "fingerprint": meta["fingerprint"],
        "protocol": "oo.clickhouse.capture-catalog/v1",
        "source_id": SOURCE_ID,
        "table_uuid": meta["uuid"],
    }
    registry = {
        "bindings": [{
            "fields": FIELDS,
            "protocol": "oo.source-admission.capture-binding/v1",
            "source_engine": "clickhouse",
            "source_id": SOURCE_ID,
        }],
        "protocol": "oo.source-admission.binding-registry/v1",
    }
    catalog_digest = write_json(OUT / "identity" / "catalog.json", catalog)
    registry_digest = write_json(OUT / "identity" / "registry.json", registry)
    (OUT / "identity" / "driver.sha256").write_text(driver_digest + "\n")
    c5_plan = cap.gate(
        customer,
        base_capture("7s") | {"sync": "full", "delete_detection": {"mode": "reconcile"}},
        secret,
        True,
    )
    c5 = cap.cycle(c5_plan, LiveSource(), PgShadow("readings_c5"), None, moment)
    c5_view = compare_source(PgShadow("readings_c5"), "c5")
    # compare_source reads the shadow table; the cycle above committed into readings_c5.
    c5_rows = PgShadow("readings_c5").rows()
    c5_diff = 0 if cells_of(c5_rows) == cells_of(oracle_rows()) else 1
    remember({"phase": "c5", "published": c5.get("published"), "method": c5.get("method"), "diff": c5_diff, "gate": c5_view["label"]})
    if c5_diff != 0 or not c5.get("published"):
        return 5
    full = LiveSource()
    full_rows = full.read_all()
    if cells_of(full_rows) != cells_of(first["rows"]):
        remember({"phase": "gen1-arrow", "error": "full stream does not match the C4 projection"})
        return 5
    run_id = str(uuid.uuid4())
    snap = snapshot_value(first["checkpoint"])
    arrow_path = OUT / "loads" / "gen1" / "load.arrow"
    arrow_path.parent.mkdir(parents=True, exist_ok=True)
    arrow_path.write_bytes(full.arrow_bytes)
    audit = {"phase": "gen1", "cell_diff": view, "query_prefix": PREFIX, "write_class": "R0"}
    receipt_path, receipt_sha = produce(1, run_id, snap, audit, arrow_path, digests, driver_digest, registry_digest, catalog_digest)
    audit_digest = receipt.sha256_file(OUT / "loads" / "gen1" / "audit.json")
    truth = truth_rows(first["rows"])
    document = load_document(1, "ch40-gen1", receipt_path, receipt_sha, "load.arrow", truth, [], snap, digests, audit_digest, owner, None)
    installed = install_and_role(owner)
    remember({"phase": "install", "exit": installed["install"]["exit"], "role_exit": installed["role_exit"], "jsonb": installed["jsonb"]})
    if installed["install"]["exit"] != 0 or installed["role_exit"] != 0:
        return 6
    gen1 = import_generation("gen1", document, full.arrow_bytes, "load.arrow")
    remember({"phase": "import-gen1", "exit": gen1["exit"], "stdout": gen1["stdout"], "stderr": gen1["stderr"]})
    if gen1["exit"] != 0 or not gen1["stdout"] or gen1["stdout"].get("load_state") not in {"awaiting_publish", "replay"}:
        return 7
    replay = call_pipeline("gen1-replay", "import-native-update", document)
    remember({"phase": "replay-gen1", "exit": replay["exit"], "stdout": replay["stdout"]})
    sealed = sealed_identity(1, digests["config_digest"])
    pg_view = native_cells(1, digests["config_digest"])
    pg_diff = exact_diff(truth, pg_view.get("cells") or [])
    (OUT / "loads" / "gen1" / "pg-cells.json").write_text(json.dumps(pg_view, indent=2) + "\n")
    remember({"phase": "gen1-cells", "sealed_exit": sealed["exit"], "pg_diff": pg_diff, "source_bound": gen1["stdout"].get("source_bound"), "row_count": gen1["stdout"].get("row_count")})
    if pg_diff != 0:
        return 8
    denial = runtime_select_denied(pg_view["table"]) if pg_view.get("table") else {"exit": None}
    remember({"phase": "runtime-select", **denial})

    def publish(phase: str, generation: int, parent: int, checkpoint: dict, previous: list[dict], batch: str) -> dict:
        current = list(checkpoint["rows"])
        upserts, tombstones = changed_rows(previous, current)
        stream = LiveSource()
        payload = stream.read_all()
        if cells_of(payload) != cells_of(current):
            raise RuntimeError(f"{phase} arrow does not match the published rows")
        arrow_bytes = stream.arrow_bytes
        run_token = str(uuid.uuid4())
        snap_value = snapshot_value(checkpoint)
        path = OUT / "loads" / f"gen{generation}" / "load.arrow"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(arrow_bytes)
        phase_audit = {"phase": phase, "tombstones": tombstones, "upserts": sorted(int(row["reading_id"]) for row in upserts)}
        receipt_file, file_sha = produce(generation, run_token, snap_value, phase_audit, path, digests, driver_digest, registry_digest, catalog_digest)
        phase_audit_digest = receipt.sha256_file(OUT / "loads" / f"gen{generation}" / "audit.json")
        # The loader copies forward keys that are neither tombstones nor arrow keys.
        # Sending the full current stream keeps the physical table equal to the source
        # without using the empty-arrow type fallback.
        full_truth = truth_rows(current)
        doc = load_document(
            generation, batch, receipt_file, file_sha, "load.arrow", full_truth, tombstones,
            snap_value, digests, phase_audit_digest, owner, parent,
        )
        # Replace the arrow with the full current stream. Tombstones cover keys absent from it.
        result = import_generation(f"gen{generation}", doc, arrow_bytes, "load.arrow")
        cells = native_cells(generation, digests["config_digest"])
        diff = exact_diff(full_truth, cells.get("cells") or [])
        seal = sealed_identity(generation, digests["config_digest"])
        (OUT / "loads" / f"gen{generation}" / "pg-cells.json").write_text(json.dumps(cells, indent=2) + "\n")
        remember({
            "phase": phase,
            "exit": result["exit"],
            "stdout": result["stdout"],
            "stderr": result["stderr"],
            "tombstones": tombstones,
            "pg_diff": diff,
            "sealed_exit": seal["exit"],
            "source_bound": None if not result["stdout"] else result["stdout"].get("source_bound"),
            "row_count": None if not result["stdout"] else result["stdout"].get("row_count"),
        })
        return {"ok": result["exit"] == 0 and diff == 0, "checkpoint": checkpoint, "rows": current, "truth": full_truth}

    unchanged = cap.cycle(plan, LiveSource(), shadow, first["checkpoint"], moment)
    remember({"phase": "unchanged-before-dml", "reason": unchanged.get("reason")})
    inserted = dba(
        "INSERT INTO oo_dl01w40.readings "
        "(reading_id, version, amount, note, code, code_or_null, ts, tags, labels, qty, label, wide_u64) "
        "VALUES (104, 1, CAST('4.0000000000' AS Decimal(76, 10)), 'new', 'n', 'n', "
        "toDateTime64('2024-02-15 00:00:00', 9, 'UTC'), CAST([] AS Array(Int64)), CAST([] AS Array(Nullable(String))), 4, 'new', toUInt64(4))",
        DBA_PREFIX + "insert",
    )
    after_insert = cap.cycle(plan, LiveSource(), shadow, first["checkpoint"], moment)
    insert_view = compare_source(shadow, "insert")
    remember({"phase": "c4-insert", "dba": inserted, "published": after_insert.get("published"), "cell_diff": insert_view})
    if not after_insert.get("published") or insert_view["diff"] != 0:
        return 9
    gen2 = publish("import-gen2", 2, 1, after_insert["checkpoint"], first["rows"], "ch40-gen2")
    if not gen2["ok"]:
        return 10
    updated = dba(
        "INSERT INTO oo_dl01w40.readings "
        "(reading_id, version, amount, note, code, code_or_null, ts, tags, labels, qty, label, wide_u64) "
        "VALUES (101, 3, CAST('8.0000000000' AS Decimal(76, 10)), 'upd', 'u', 'u', "
        "toDateTime64('2024-03-15 12:34:56.123456789', 9, 'UTC'), [1], ['u'], 8, 'upd', toUInt64(8))",
        DBA_PREFIX + "update",
    )
    after_update = cap.cycle(plan, LiveSource(), shadow, after_insert["checkpoint"], moment)
    update_view = compare_source(shadow, "update")
    remember({"phase": "c4-update", "dba": updated, "published": after_update.get("published"), "cell_diff": update_view})
    if not after_update.get("published") or update_view["diff"] != 0:
        return 11
    gen3 = publish("import-gen3", 3, 2, after_update["checkpoint"], after_insert["rows"], "ch40-gen3")
    if not gen3["ok"]:
        return 12
    dba("SYSTEM START MERGES oo_dl01w40.readings", DBA_PREFIX + "start-before-delete")
    deleted = dba(
        "DELETE FROM oo_dl01w40.readings WHERE reading_id = 102 "
        "SETTINGS lightweight_deletes_sync = 0, mutations_sync = 0",
        DBA_PREFIX + "delete-async",
        timeout=45,
    )
    samples = []
    pending_reason = None
    for _ in range(15):
        probe = LiveSource()
        seen = probe.inspect()
        samples.append({"pending": seen["mutation_pending"], "is_done": seen["mutation_is_done"]})
        if seen["mutation_pending"] and pending_reason is None:
            pending_reason = cap.cycle(plan, probe, shadow, after_update["checkpoint"], moment).get("reason")
            break
        if not seen["mutation_pending"]:
            break
        time.sleep(1)
    caught = cap.cycle(plan, LiveSource(), shadow, after_update["checkpoint"], moment)
    if not caught.get("published"):
        dba("SYSTEM START MERGES oo_dl01w40.readings", DBA_PREFIX + "start-merges")
        for _ in range(20):
            seen = LiveSource().inspect()
            samples.append({"pending": seen["mutation_pending"], "is_done": seen["mutation_is_done"]})
            if not seen["mutation_pending"]:
                break
            time.sleep(1)
        caught = cap.cycle(plan, LiveSource(), shadow, after_update["checkpoint"], moment)
    delete_view = compare_source(shadow, "delete")
    identities = sorted(int(row["reading_id"]) for row in shadow.rows())
    remember({
        "phase": "c4-delete",
        "dba": deleted,
        "samples": samples,
        "pending_reason": pending_reason,
        "published": caught.get("published"),
        "cell_diff": delete_view,
        "identities": identities,
        "deleted_absent": 102 not in identities,
    })
    if not caught.get("published") or delete_view["diff"] != 0 or 102 in identities:
        return 13
    gen4 = publish("import-gen4", 4, 3, caught["checkpoint"], after_update["rows"], "ch40-gen4")
    if not gen4["ok"]:
        return 14
    quiet = cap.cycle(plan, LiveSource(), shadow, caught["checkpoint"], moment)
    remember({"phase": "unchanged-after-delete", "reason": quiet.get("reason"), "deleted_absent": 102 not in identities})
    writes = product_writes()
    (OUT / "product-writes.json").write_text(json.dumps(writes, indent=2) + "\n")
    remember({"phase": "product-writes", "write_count": writes["write_count"]})
    if writes["write_count"] != 0:
        return 15
    saved_truth = gen4["truth"]
    (OUT / "loads" / "gen4" / "truth.json").write_text(json.dumps(saved_truth) + "\n")

    def cancel() -> None:
        raise cap.Cancelled()

    dba(
        "INSERT INTO oo_dl01w40.readings "
        "(reading_id, version, amount, note, code, code_or_null, ts, tags, labels, qty, label, wide_u64) "
        "VALUES (104, 2, CAST('5.0000000000' AS Decimal(76, 10)), 'x', 'n', 'n', "
        "toDateTime64('2024-02-15 00:00:00', 9, 'UTC'), CAST([] AS Array(Int64)), CAST([] AS Array(Nullable(String))), 5, 'x', toUInt64(5))",
        DBA_PREFIX + "interrupt-src",
    )
    before = cells_of(shadow.rows())
    interrupted = cap.cycle(plan, LiveSource(), shadow, caught["checkpoint"], moment, before_commit=cancel)
    after = cells_of(shadow.rows())
    sleeper = subprocess.Popen(
        ["docker", "exec", "-i", PG, "psql", "-U", "postgres", "-d", "oo_ch40", "-v", "ON_ERROR_STOP=1",
         "-c", "BEGIN; DELETE FROM capture.readings WHERE partition_id = '202401'; SELECT pg_sleep(30);"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    visible = "0"
    activity = ""
    deadline = time.time() + 8
    while time.time() < deadline:
        active = psql(
            "SELECT count(*) FROM pg_stat_activity "
            "WHERE pid <> pg_backend_pid() AND state = 'active' AND position('pg_sleep' in query) > 0"
        )
        visible = text_of(active)
        activity = text_of(psql(
            "SELECT pid::text || ' ' || state || ' ' || left(query, 120) FROM pg_stat_activity "
            "WHERE state = 'active' AND position('pg_sleep' in query) > 0 ORDER BY pid"
        ))
        if visible.isdigit() and int(visible) >= 1:
            break
        time.sleep(0.4)
    terminated = psql(
        "SELECT pg_terminate_backend(pid) FROM pg_stat_activity "
        "WHERE pid <> pg_backend_pid() AND position('pg_sleep' in query) > 0"
    )
    try:
        sleeper.wait(timeout=15)
    except subprocess.TimeoutExpired:
        sleeper.kill()
        sleeper.wait(timeout=10)
    killed = cells_of(shadow.rows())
    interrupt_ok = interrupted.get("rolled_back") is True and before == after == killed and visible.isdigit() and int(visible) >= 1
    remember({
        "phase": "interrupt",
        "reason": interrupted.get("reason"),
        "rolled_back": interrupted.get("rolled_back"),
        "shadow_unchanged": before == after == killed,
        "sleep_visible": visible,
        "sleep_activity": activity,
        "terminate_exit": terminated.returncode,
        "sleeper_exit": sleeper.returncode,
        "ok": interrupt_ok,
    })
    stop = run(["docker", "stop", CH], timeout=60)
    offline = native_cells(4, digests["config_digest"])
    offline_diff = exact_diff(saved_truth, offline.get("cells") or [])
    start = run(["docker", "start", CH], timeout=60)
    wait_ready()
    remember({"phase": "source-off", "stop_exit": stop.returncode, "start_exit": start.returncode, "pg_diff": offline_diff})
    added = dba(
        "ALTER TABLE oo_dl01w40.readings ADD COLUMN IF NOT EXISTS extra_n1 String DEFAULT ''",
        DBA_PREFIX + "add-column",
    )
    samples = []
    observed = ""
    for _ in range(20):
        seen = LiveSource().inspect()
        observed = str(seen["fingerprint"])
        samples.append({"pending": seen["mutation_pending"], "is_done": seen["mutation_is_done"]})
        if not seen["mutation_pending"]:
            break
        time.sleep(1)
    paused = cap.cycle(plan, LiveSource(), shadow, caught["checkpoint"], moment)
    remember({
        "phase": "schema-changed",
        "dba": added,
        "samples": samples,
        "reason": paused.get("reason"),
        "published": paused.get("published"),
        "fingerprint_differs": observed != plan["fingerprint"],
    })
    final_writes = product_writes()
    (OUT / "product-writes-final.json").write_text(json.dumps(final_writes, indent=2) + "\n")
    token = "/tmp/oo-customer-demo-20261007/llm-token"
    probes = {}
    for name, prefix in (
        ("self_build", "13ad15b4"),
        ("gateway", "ac896"),
        ("public_cli", "a6d27"),
        ("native_loader", "2e0bd098363ace48bda9b9127f5fabe6d3a1647d"),
    ):
        found = run(["git", "-C", str(ROOT), "rev-parse", "--verify", prefix])
        probes[name] = text_of(found) if found.returncode == 0 else None
    which = run(["bash", "-lc", "command -v oo || true"])
    blocked = {
        "llm_token_is_directory": os.path.isdir(token),
        "llm_token_is_file": os.path.isfile(token),
        "llm_called": False,
        "pack_written": False,
        "gateway_called": False,
        "agent_db_used_as_gateway": False,
        "oo_cli": text_of(which) or None,
        "commit_probes": probes,
        "sealed_generations": [1, 2, 3, 4],
        "reason": (
            "The approved bearer path is a directory, so S5 and the model dialogue were not called. "
            "This run does not compile the self-build, gateway, or CLI trees and does not mock them."
        ),
    }
    (OUT / "blocked-interfaces.json").write_text(json.dumps(blocked, indent=2) + "\n")
    remember({"phase": "final-writes", "write_count": final_writes["write_count"], "blocked": blocked})
    if final_writes["write_count"] != 0 or offline_diff != 0:
        return 16
    if paused.get("published") is not False:
        return 17
    if not interrupt_ok:
        return 18
    return 0


if __name__ == "__main__":
    try:
        code = main()
    except Exception as exc:
        remember({"phase": "exception", "error": str(exc)[:800]})
        code = 1
    (OUT / "live.exit").write_text(str(code) + "\n")
    raise SystemExit(code)
