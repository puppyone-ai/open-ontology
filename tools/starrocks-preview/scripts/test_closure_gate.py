#!/usr/bin/env python3
"""Host checks for scrub, full-cell compare, and customer admission."""

from __future__ import annotations

import json
from pathlib import Path

import closure_gate as gate


def test_scrub_keeps_a_decimal_coefficient() -> None:
    secret = b"999001"
    coefficient = b"22345678909990017890123456789"
    body = b'{"kind":"numeric","coefficient":"' + coefficient + b'","bearer":"999001"}'
    cleaned = gate.scrub_bytes(body, [secret])
    assert coefficient in cleaned
    assert b'"bearer":"[redacted]"' in cleaned
    assert b'"bearer":"999001"' not in cleaned


def test_compare_is_full_cells_not_only_counts() -> None:
    fields = [
        {"name": "account_id", "type": "int64"},
        {"name": "balance", "type": "decimal128(38,9)"},
        {"name": "opened", "type": "timestamp_second_naive"},
    ]
    decoded = [{
        "account_id": 300,
        "balance": "22345678901234567890123456789.123456789",
        "opened": "2026-05-02 03:04:05",
    }]
    got = [[
        {"kind": "int8", "value": "300"},
        gate.decimal_cell("22345678901234567890123456789.123456789"),
        {"kind": "timestamp", "value": "2026-05-02T03:04:05.000000"},
    ]]
    compared = gate.compare_rows("pk_accounts", decoded, fields, got)
    assert compared["equal"] is True
    assert compared["expected_rows"] == 1
    assert compared["cell_mismatches"] == []
    assert compared["expected"][0][1]["coefficient"].endswith("123456789")
    assert compared["temporal_equal"] is True
    assert compared["temporal"][0]["consumer_kind"] == "timestamp"
    short = [row[:2] for row in got]
    wrong = gate.compare_rows("pk_accounts", decoded, fields, short)
    assert wrong["equal"] is False
    assert wrong["got_rows"] == 1
    date_fields = [{"name": "bucket", "type": "date32"}]
    dated = gate.compare_rows(
        "dup_events",
        [{"bucket": "2026-05-06"}],
        date_fields,
        [[{"kind": "date", "value": "2026-05-06"}]],
    )
    assert dated["temporal"][0]["source_type"] == "date32"
    assert dated["temporal"][0]["equal"] is True
    naive_wrong = gate.compare_rows(
        "dup_events",
        [{"bucket": "2026-05-06"}],
        date_fields,
        [[{"kind": "varchar", "value": "2026-05-06"}]],
    )
    assert naive_wrong["temporal_equal"] is False
    null_seen = gate.compare_rows(
        "dup_events",
        [{"seen": None}],
        [{"name": "seen", "type": "timestamp_second_naive"}],
        [[{"kind": "null"}]],
    )
    assert null_seen["temporal_equal"] is True
    assert null_seen["temporal"][0]["rows"][0]["expected"]["kind"] == "null"


def test_unknown_object_is_refused_before_read() -> None:
    registry = {
        "bindings": [{
            "source_id": "sr_w45",
            "source_engine": "starrocks",
            "capture_receipt_sha256": "abc",
            "fields": [{"name": "account_id"}, {"name": "opened"}],
            "objects": [{
                "logical_schema": "oo_w45_sr",
                "logical_table": "pk_accounts",
                "row_count": 3,
                "arrow_sha256": "def",
            }],
        }]
    }
    request = {
        "protocol": gate.CUSTOMER_PROTOCOL,
        "secret_ref": "secret://oo-dl01w1-45/reader",
        "source_id": "sr_w45",
        "budget": {"unit": "Rows", "hard_limit": 1000},
        "objects": [
            {"schema": "oo_w45_sr", "name": "pk_accounts"},
            {"schema": "oo_w45_sr", "name": "secret_accounts", "fields": ["password"]},
            {"schema": "oo_w45_sr", "name": "pk_accounts", "fields": ["account_id", "password"]},
        ],
    }
    decision = gate.admit_customer(request, registry)
    assert decision["ok"] is True
    assert [item["name"] for item in decision["admitted"]] == ["pk_accounts"]
    assert decision["admitted"][0]["row_count"] == 3
    reasons = {item["reason"] for item in decision["refused"]}
    assert reasons == {"not_approved", "field_not_approved"}
    assert all(item["read_attempted"] is False for item in decision["refused"])
    opened = []

    def opener(name: str) -> None:
        opened.append(name)

    for item in decision["admitted"]:
        opener(item["name"])
    assert opened == ["pk_accounts"]
    missing = gate.resolve_secret(
        "secret://oo-dl01w1-45/other",
        {"secret://oo-dl01w1-45/reader": "/tmp/reader.password"},
        lambda _path: False,
        lambda _path: 0o600,
    )
    assert missing["ok"] is False
    assert missing["read_attempted"] is False


def test_failed_write_is_counted() -> None:
    clean = gate.audit_events([
        {"class": "handshake"},
        {"class": "read", "sql": "SELECT 1"},
        {"class": "cleanup"},
    ])
    assert clean["write_attempts"] == 0
    assert clean["ok"] is True
    failed = gate.audit_events([
        {"class": "handshake"},
        {"class": "write", "sql": "INSERT INTO pk_accounts VALUES (1)", "ok": False, "error": "denied"},
        {"class": "cleanup"},
    ])
    assert failed["write_attempts"] == 1
    assert failed["failed_write_attempts"] == 1
    assert failed["ok"] is False
    hidden = gate.audit_events([
        {"class": "handshake"},
        {"class": "read", "sql": "DROP TABLE pk_accounts", "error": "denied"},
        {"class": "cleanup"},
    ])
    assert hidden["write_attempts"] == 1
    assert hidden["ok"] is False
    assert gate.fe_write_attempts("User=oo_w45_reader|Stmt=SELECT 1\n") == 0
    assert gate.fe_write_attempts("User=oo_w45_reader|Stmt=INSERT INTO t VALUES (1)\n") == 1
    assert gate.fe_write_attempts("User=root|Stmt=GRANT SELECT TO USER 'oo_w45_reader'@'%'\n") == 0


def test_same_pack_requires_one_identity() -> None:
    query = {
        "protocol": "oo.operator-preview.result/v1",
        "type_path": "native_contract",
        "returned_rows": 3,
        "import": {"import_hash": "abc", "source_id": "sr_w45"},
    }
    pack = {
        "pack_digest": "digest",
        "model": "called",
        "model_input_sha256": "model",
        "identity_hash": "abc",
        "source_id": "sr_w45",
        "source_engine": "starrocks",
    }
    typed = gate.pack_identity(pack, query)
    again = gate.pack_identity(pack, query)
    assert gate.same_pack(typed, again) is True
    other = dict(again)
    other["query_import_hash"] = "different"
    assert gate.same_pack(typed, other) is False


def test_customer_file_matches_the_registry() -> None:
    root = Path(__file__).resolve().parents[3]
    customer = json.loads((root / "evidence/2c023948-0862-4dae-ae8f-6fc30241db62/DL01/starrocks-product/customer-input.json").read_text())
    registry = json.loads((root / "evidence/2c023948-0862-4dae-ae8f-6fc30241db62/DL01/starrocks-product/raw/native-bind/registry.json").read_text())
    decision = gate.admit_customer(customer, registry)
    assert decision["ok"] is True
    counts = {item["name"]: item["row_count"] for item in decision["admitted"]}
    assert counts == {
        "pk_accounts": 3,
        "agg_metrics": 2,
        "dup_events": 6,
        "uniq_dim": 2,
        "dup_partition_sums": 2,
    }
    assert any(item["reason"] == "not_approved" for item in decision["refused"])
    assert any(item["reason"] == "field_not_approved" for item in decision["refused"])


if __name__ == "__main__":
    test_scrub_keeps_a_decimal_coefficient()
    test_compare_is_full_cells_not_only_counts()
    test_unknown_object_is_refused_before_read()
    test_failed_write_is_counted()
    test_same_pack_requires_one_identity()
    test_customer_file_matches_the_registry()
    print("closure_gate tests ok")
