"""Unit tests for the StarRocks oracle helpers.

These tests are labeled unit tests. They do not start StarRocks and they are
not capability acceptance.
"""

from __future__ import annotations

import json
import sys
import unittest
from decimal import Decimal
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(Path(__file__).resolve().parent))

import oracle_lib  # noqa: E402


class OracleUnitTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.oracle = json.loads((ROOT / "oracle" / "expected.json").read_text(encoding="utf-8"))
        cls.load_sql = (ROOT / "sql" / "dba-load.sql").read_text(encoding="utf-8")

    def test_admission_bounds(self) -> None:
        self.assertEqual(oracle_lib.decide_admission(0, 0), "admit")
        self.assertEqual(oracle_lib.decide_admission(9, 0), "admit")
        self.assertEqual(oracle_lib.decide_admission(10, 0), "pending-capacity-global-rust")
        self.assertEqual(oracle_lib.decide_admission(0, 1), "pending-capacity-own-rust")

    def test_product_identity_rejects_mysql_and_doris(self) -> None:
        ok = oracle_lib.product_identity("3.5.21-aaaa", "StarRocks version 3.5.21", "5.1.0")
        self.assertTrue(ok["ok"])
        self.assertIn("not a StarRocks product version", ok["protocol_note"])
        # Observed on starrocks/allin1-ubuntu 3.5.21: the word StarRocks is not
        # in current_version() or @@version_comment. @@version is 8.0.33.
        observed = oracle_lib.product_identity(
            "3.5.21-85fccf8",
            "3.5.21-85fccf8",
            "8.0.33",
            ["be_tablets", "tables_config"],
        )
        self.assertTrue(observed["ok"])
        self.assertNotEqual(observed["product_version"], observed["protocol_version"])
        build_without_catalog = oracle_lib.product_identity("3.5.21-85fccf8", "3.5.21-85fccf8", "8.0.33", [])
        self.assertFalse(build_without_catalog["ok"])
        mysql = oracle_lib.product_identity(
            "8.0.36",
            "MySQL Community Server (GPL)",
            "8.0.36",
            ["be_tablets", "tables_config"],
        )
        self.assertFalse(mysql["ok"])
        doris = oracle_lib.product_identity(
            "2.1.7",
            "Doris version 2.1.7",
            "5.7.99",
            ["be_tablets", "tables_config"],
        )
        self.assertFalse(doris["ok"])
        protocol_only = oracle_lib.product_identity("8.0.33", "8.0.33", "8.0.33", ["tables"])
        self.assertFalse(protocol_only["ok"])

    def test_handwritten_partition_sums_match_rows(self) -> None:
        dup = self.oracle["dup_events"]
        summary = oracle_lib.partition_summary(dup["visible_initial"])
        self.assertEqual(summary, dup["partition_sums_initial"])
        after = oracle_lib.visible_after_append(dup["visible_initial"], dup["append"])
        self.assertEqual(oracle_lib.partition_summary(after), dup["partition_sums_after_append"])

    def test_null_zero_empty_are_distinct(self) -> None:
        dup = self.oracle["dup_events"]
        rows = oracle_lib.multiset(dup["visible_initial"], dup["columns"])
        null_amount = [row for row in rows if row[0] == 2][0]
        zero_amount = [row for row in rows if row[0] == 5][0]
        null_note = [row for row in rows if row[0] == 3][0]
        self.assertIsNone(null_amount[2])
        self.assertEqual(null_amount[3], "")
        self.assertEqual(zero_amount[2], Decimal("0.0000"))
        self.assertIsNone(null_note[3])
        self.assertNotEqual(null_amount, zero_amount)

    def test_primary_and_unique_keep_last_and_duplicate_keeps_both(self) -> None:
        dup_rows = oracle_lib.multiset(
            self.oracle["dup_events"]["visible_initial"],
            self.oracle["dup_events"]["columns"],
        )
        alpha = [row for row in dup_rows if row[0] == 1]
        self.assertEqual(len(alpha), 2)
        pk = self.oracle["pk_accounts"]
        visible = oracle_lib.multiset(pk["visible_initial"], pk["columns"])
        hidden = oracle_lib.multiset(pk["not_visible"], pk["columns"])
        self.assertEqual(len(visible), 3)
        self.assertFalse(set(hidden) & set(visible))
        uniq = self.oracle["uniq_dim"]
        uvis = oracle_lib.multiset(uniq["visible"], uniq["columns"])
        uhid = oracle_lib.multiset(uniq["not_visible"], uniq["columns"])
        self.assertEqual(len(uvis), 2)
        self.assertFalse(set(uhid) & set(uvis))

    def test_aggregate_visible_is_the_sum(self) -> None:
        agg = self.oracle["agg_metrics"]
        visible = oracle_lib.multiset(agg["visible"], agg["columns"])
        self.assertEqual(visible[0][2], 5)
        self.assertEqual(visible[0][3], Decimal("3.7500"))
        inputs = oracle_lib.multiset(agg["inputs"], agg["columns"])
        self.assertNotEqual(inputs, visible)

    def test_large_decimal_is_not_a_float_literal(self) -> None:
        balance = "12345678901234567890123456789.123456789"
        self.assertEqual(Decimal(balance), Decimal(balance))
        as_float = format(float(balance), "f")
        self.assertNotEqual(Decimal(as_float), Decimal(balance))
        encoded = "100|" + balance + "|replaced"
        columns = self.oracle["pk_accounts"]["columns"]
        parsed = oracle_lib.parse_encoded_row(encoded, columns)
        match = oracle_lib.rows_match([encoded], [self.oracle["pk_accounts"]["visible_initial"][0]], columns)
        self.assertTrue(match["ok"])
        self.assertEqual(parsed["balance"], balance)

    def test_date_display_uses_the_calendar_day(self) -> None:
        columns = self.oracle["dup_events"]["columns"]
        line = "1|2026-01-15 00:00:00|10.2500|alpha"
        parsed = oracle_lib.parse_encoded_row(line, columns)
        self.assertEqual(oracle_lib.norm_value(parsed["bucket"], columns[1]), "2026-01-15")

    def test_sql_literals_cover_the_oracle_and_have_no_passwords(self) -> None:
        needed = [
            "10.2500",
            "99999999999999.9999",
            "-0.0001",
            "12345678901234567890123456789.123456789",
            "0.100000000",
            "1.5000",
            "2.2500",
            "3.7500",
            "边界",
            "零",
            "DUPLICATE KEY",
            "PRIMARY KEY",
            "AGGREGATE KEY",
            "UNIQUE KEY",
            "p202601",
            "p202602",
        ]
        for item in needed:
            self.assertIn(item, self.load_sql if item != "3.7500" else json.dumps(self.oracle))
        self.assertNotIn("IDENTIFIED BY", self.load_sql.upper())
        self.assertNotIn("PASSWORD", self.load_sql.upper())
        reader = (ROOT / "sql" / "reader-positive.sql").read_text(encoding="utf-8").upper()
        for forbidden in ("INSERT", "UPDATE", "DELETE", "CREATE", "DROP", "ALTER"):
            self.assertNotIn(forbidden, reader)

    def test_marked_results_and_audit_classifier(self) -> None:
        text = "@@result dup_events\n1|2026-01-15|10.2500|alpha\n@@result pk_accounts\n101|\\N|\\N\n"
        marked = oracle_lib.split_marked_results(text)
        self.assertEqual(marked["pk_accounts"], [r"101|\N|\N"])
        lines = [
            "2026 [query] |User=oo_reader|State=EOF|Stmt=select /* marker */ 1",
            "2026 [query] |User=oo_reader|State=ERR|Stmt=insert into pk_accounts values (1)",
            "2026 [query] |User=oo_dba|State=EOF|Stmt=insert into dup_events values (1)",
            "2026 [query] |User=oo_dba|State=EOF|Stmt=KILL QUERY 9",
        ]
        read_only = oracle_lib.reader_audit(lines[:1])
        self.assertTrue(read_only["read_window_ok"])
        with_write = oracle_lib.reader_audit(lines)
        self.assertFalse(with_write["read_window_ok"])
        self.assertEqual(with_write["classes"].get("write"), 1)
        self.assertEqual(oracle_lib.statement_class("KILL QUERY 9"), "control")
        self.assertEqual(oracle_lib.statement_class("/* c */ SELECT sleep(30)"), "read")
        kill_line = "2026 [query] |User=oo_reader|State=OK|Stmt=KILL QUERY 9|Digest=abc|PlanCpuCost=0"
        select_line = "2026 [query] |User=oo_reader|State=EOF|Stmt=SELECT 1|Digest="
        self.assertEqual(oracle_lib.extract_audit_stmt(kill_line), "KILL QUERY 9")
        operational = oracle_lib.reader_audit([select_line, kill_line])
        self.assertTrue(operational["read_window_ok"])
        self.assertEqual(operational["classes"].get("control"), 1)
        self.assertEqual(operational["classes"].get("write", 0), 0)
        self.assertEqual(operational["control_statements"], ["KILL QUERY 9"])
        self.assertEqual(operational["write_statements"], [])

    def test_grant_text_accepts_create_table_underscore(self) -> None:
        shown = "GRANT ALTER, DROP, CREATE_TABLE ON DATABASE oo_dl01_sr TO USER 'oo_dba'@'%'"
        self.assertTrue(oracle_lib.grant_visible(shown, "CREATE TABLE"))
        self.assertFalse(oracle_lib.grant_visible("GRANT SELECT ON ALL TABLES IN DATABASE oo_dl01_sr", "CREATE TABLE"))
        tables = "GRANT DELETE, DROP, INSERT, SELECT, ALTER ON ALL TABLES IN DATABASE oo_dl01_sr"
        self.assertTrue(oracle_lib.grant_visible(tables, "INSERT"))

    def test_redact_removes_secret_and_identified_by(self) -> None:
        secret = "fixture-secret-value"
        raw = f"ALTER USER 'root' IDENTIFIED BY '{secret}'; SET PASSWORD = PASSWORD('{secret}')"
        cleaned = oracle_lib.redact(raw, [secret])
        self.assertNotIn(secret, cleaned)
        self.assertIn("***", cleaned)


if __name__ == "__main__":
    unittest.main()
