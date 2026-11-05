import csv
import io
import json
import unittest
from types import SimpleNamespace
from unittest.mock import Mock

import audit


def mysql_row(kind, sql, user="fixture_reader[fixture_reader] @  [172.17.0.1]", stamp="2026-11-05 01:00:00.000001"):
    return "\t".join([stamp, "7", kind, sql.encode().hex(), user])


class AuditTests(unittest.TestCase):
    def test_postgres_audit_does_not_change_role_startup_settings(self):
        run = SimpleNamespace(engine="postgres", sql=Mock())
        audit.Audit(run).enable()
        sql = run.sql.call_args.args[0]
        self.assertIn("ALTER SYSTEM SET log_statement='all'", sql)
        self.assertIn("log_replication_commands=on", sql)
        self.assertIn("pg_reload_conf()", sql)
        self.assertNotIn("ALTER ROLE", sql)
        self.assertTrue(run.sql.call_args.kwargs["admin"])

    def test_read_policy(self):
        for sql in ("SELECT COUNT(*) FROM orders", "SHOW TABLES", "DESCRIBE orders",
                    "SELECT 'DELETE FROM orders;'", "START TRANSACTION READ ONLY",
                    "ALTER SESSION SET NLS_LANGUAGE='AMERICAN' TIME_ZONE='+00:00'",
                    "SELECT DECODE(USER, 'XS$NULL', XS_SYS_CONTEXT('XS$SESSION','USERNAME'), USER) FROM SYS.DUAL",
                    "SELECT * FROM v$logmnr_contents",
                    "BEGIN DBMS_LOGMNR.START_LOGMNR(STARTSCN=>:1, OPTIONS=>DBMS_LOGMNR.DICT_FROM_ONLINE_CATALOG); END;",
                    "SELECT * FROM pg_logical_slot_peek_changes('slot', NULL, NULL)"):
            self.assertTrue(audit.read_statement(sql), sql)
        for sql in ("DELETE FROM orders", "SELECT 1; DELETE FROM orders",
                    "WITH gone AS (DELETE FROM orders RETURNING *) SELECT * FROM gone",
                    "SELECT * INTO copy FROM orders", "SELECT nextval('s')",
                    "SELECT evil()", "SELECT schema.evil()", "SET log_statement=none",
                    "SELECT 1 /*! INTO OUTFILE '/tmp/x' */", "CALL write()",
                    "BEGIN DBMS_LOGMNR_D.BUILD; END;", "SELECT * FROM t FOR UPDATE",
                    "ALTER SESSION SET SQL_TRACE=TRUE", "SELECT $$evil$$",
                    "SELECT '--'; DELETE FROM t", "SELECT '/*'; DROP TABLE t",
                    "SELECT 'unterminated", "SELECT 1 /* unclosed",
                    "SELECT * FROM pg_logical_slot_get_changes('slot', NULL, NULL)",
                    "BEGIN DBMS_LOGMNR.START_LOGMNR(OPTIONS=>DBMS_LOGMNR.DICT_FROM_REDO_LOGS); END;"):
            self.assertFalse(audit.read_statement(sql), sql)

    def test_engine_rules_name_the_rule_miss(self):
        cases = [
            ("postgres", "snapshot", "START TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY", "txn_control"),
            ("postgres", "snapshot", "SET TRANSACTION SNAPSHOT '00000003-1'", "txn_control"),
            ("postgres", "snapshot", "DECLARE oo_chunk NO SCROLL CURSOR FOR SELECT id FROM public.t "
                                     "WHERE id >= CAST($1 AS bigint) ORDER BY id", "read_cursor"),
            ("postgres", "snapshot", "FETCH FORWARD 10000 FROM oo_chunk", "read_cursor"),
            ("postgres", "snapshot", "EXPLAIN (FORMAT JSON) SELECT * FROM public.t", "read_explain"),
            ("postgres", "snapshot", "SELECT pg_catalog.set_config('statement_timeout', $1, false), "
                                     "pg_catalog.set_config('default_transaction_read_only', 'on', false) LIMIT 1",
             "d12_session_state"),
            ("postgres", "snapshot", "SELECT pg_export_snapshot()", "txn_control"),
            ("mysql", "c1", "SET @master_binlog_checksum= @@global.binlog_checksum", "d12_session_state"),
            ("mysql", "c5", "SELECT COUNT(*),BIT_XOR(CRC32(CONCAT_WS('#',`id`,`payload`))) FROM `fixture`.`items`",
             "c5_digest"),
            ("oracle", "c5", "SELECT TO_CHAR(current_scn) FROM v$database", "catalog_read"),
        ]
        for engine, variant, sql, category in cases:
            self.assertEqual(audit.classify_sql(sql, engine, variant)[0], category, sql)
        rejections = [
            ("postgres", "EXPLAIN (ANALYZE) SELECT 1", "explain_analyze_executes"),
            ("postgres", "SELECT set_config('log_statement', 'none', false)", "unreviewed_setting:LOG_STATEMENT"),
            ("postgres", "BEGIN READ WRITE", "not_a_query"),
            ("postgres", "CREATE TEMP TABLE x(i int)", "write_keyword:CREATE"),
            ("mysql", "SET GLOBAL general_log=0", "server_scope_set"),
            ("mysql", "SET PERSIST sql_mode=''", "server_scope_set"),
            ("mysql", "SELECT 1 LOCK IN SHARE MODE", "write_keyword:LOCK"),
            ("mysql", "EXPLAIN ANALYZE SELECT 1", "explain_analyze_executes"),
            ("oracle", "SELECT f(1) FROM dual", "unreviewed_function:F"),
        ]
        for engine, sql, rule in rejections:
            self.assertEqual(audit.classify_sql(sql, engine), ("rejected", rule), sql)

    def test_replication_only_in_c1(self):
        record = {"kind": "replication", "sql": "START_REPLICATION SLOT \"oo\" LOGICAL 0/16B3748 (proto_version '1')"}
        self.assertEqual(audit.classify(record, "postgres", "c1")[0], "replication_protocol")
        self.assertEqual(audit.classify(record, "postgres", "c5"),
                         ("rejected", "replication_command_outside_c1"))
        self.assertEqual(audit.classify({"kind": "replication", "sql": "CREATE_REPLICATION_SLOT x LOGICAL pgoutput"},
                                        "postgres", "c1")[1], "unreviewed_replication_command:CREATE_REPLICATION_SLOT")
        dump = {"kind": "replication", "action": "Binlog Dump", "sql": ""}
        self.assertEqual(audit.classify(dump, "mysql", "c1")[0], "replication_protocol")
        self.assertEqual(audit.classify(dump, "mysql", "snapshot")[0], "rejected")

    def test_oracle_system_writes_need_privilege_proof_and_exact_objects(self):
        logon = {"kind": "statement", "action": "UPDATE", "object": "SYS.USER$", "sql": "update user$ set spare6=..."}
        self.assertEqual(audit.classify(logon, "oracle", "c5")[1], "system_dml_without_privilege_proof")
        self.assertEqual(audit.classify(logon, "oracle", "c5", True)[0], "d12_logon_bookkeeping")
        user_table = dict(logon, object="FIXTURE.ITEMS")
        self.assertEqual(audit.classify(user_table, "oracle", "c5", True), ("rejected", "write:UPDATE:FIXTURE.ITEMS"))
        miner = {"kind": "statement", "action": "DELETE", "object": "SYSTEM.LOGMNR_GT_TAB_INCLUDE$",
                 "sql": "DELETE FROM SYSTEM.LOGMNR_GT_TAB_INCLUDE$", "user": audit.ORACLE_MINER}
        self.assertEqual(audit.classify(miner, "oracle", "c1", True)[0], "d11a_logminer_object_dml")
        self.assertEqual(audit.classify(miner, "oracle", "c5", True)[0], "rejected")
        self.assertEqual(audit.classify(dict(miner, user=audit.READER), "oracle", "c1", True)[0], "rejected")
        commit = {"kind": "statement", "action": "COMMIT", "sql": ""}
        self.assertEqual(audit.classify(commit, "oracle", "c5")[0], "txn_control")
        self.assertEqual(audit.classify({"kind": "statement", "action": "SELECT", "sql": "SELECT 1 FROM dual",
                                         "truncated": True}, "oracle", "c5")[1], "truncated_sql_text")

    def test_mysql_records_and_commands(self):
        text = "\n".join([mysql_row("Connect", "fixture_reader@x on fixture"),
                          mysql_row("Query", "SELECT\n1"), mysql_row("Binlog Dump", ""),
                          mysql_row("Init DB", "fixture")])
        records = audit.mysql_records(text)
        self.assertEqual([r["kind"] for r in records], ["session", "statement", "replication", "mysql_command"])
        self.assertEqual(records[1]["sql"], "SELECT\n1")
        self.assertEqual(audit.classify(records[3], "mysql", "snapshot")[0], "d12_session_state")
        self.assertEqual(audit.classify(dict(records[3], action="Debug"), "mysql", "snapshot")[0], "rejected")
        for text in ("bad", "x\t1\tQuery\txyz\tu"):
            with self.assertRaises(ValueError):
                audit.mysql_records(text)

    def test_pg_identity_error_replication_and_csv(self):
        stream = io.StringIO()
        writer = csv.writer(stream)
        for user, message, error, statement in (
            ("postgres", "statement: INSERT INTO t VALUES(1)", "", ""),
            ("fixture_reader", "statement: SELECT\n1", "", ""),
            ("fixture_reader", "execute <unnamed>: SELECT 2", "", ""),
            ("fixture_reader", "received replication command: IDENTIFY_SYSTEM", "LOG", ""),
            ("fixture_reader", "denied", "ERROR", "DELETE FROM t"),
        ):
            row = [""] * 23
            row[0] = "2026-11-05 01:02:03.456 UTC"
            row[1], row[13], row[11], row[19] = user, message, error, statement
            writer.writerow(row)
        records = audit.postgres_records(stream.getvalue())
        self.assertEqual([r["sql"] for r in records], ["SELECT\n1", "SELECT 2", "IDENTIFY_SYSTEM", "DELETE FROM t"])
        self.assertEqual(records[2]["kind"], "replication")
        self.assertEqual(records[0]["time"], "2026-11-05 01:02:03.456")
        self.assertEqual(audit.classify(records[3], "postgres", "snapshot")[0], "rejected")
        with self.assertRaises(ValueError):
            audit.postgres_records("malformed")
        with self.assertRaises(ValueError):
            audit.postgres_records(stream.getvalue().replace(" UTC", " CET"))

    def test_oracle_rows_and_truncation(self):
        row = {"action": "SELECT", "user": "FIXTURE_READER", "time": "2026-11-05 01:00:00.000000",
               "owner": None, "object": None, "sql": "SELECT 1 FROM dual", "sql_length": "18"}
        records = audit.oracle_records(json.dumps(row) + "\n" + json.dumps(dict(row, action="LOGON", sql=None)))
        self.assertEqual([r["kind"] for r in records], ["statement", "session"])
        self.assertTrue(audit.oracle_records(json.dumps(dict(row, sql_length="4000")))[0]["truncated"])
        with self.assertRaises(ValueError):
            audit.oracle_records("SELECT|41")

    def test_windows_attribute_variants_and_harness(self):
        records = [{"time": "2026-01-01 00:00:01", "kind": "statement", "sql": "SELECT 1"},
                   {"time": "2026-01-01 00:00:05", "kind": "replication", "sql": "IDENTIFY_SYSTEM"},
                   {"time": "2026-01-01 00:00:09", "kind": "statement", "sql": "SELECT 2"}]
        windows = [{"seq": 1, "variant": "c1", "start": "2026-01-01 00:00:04", "end": "2026-01-01 00:00:06"}]
        summary = audit.summarize(records, "postgres", windows)
        self.assertEqual(summary["c1"]["statements"], 1)
        self.assertEqual(summary["harness"]["statements"], 2)
        self.assertEqual(summary["c1"]["rejected"], 0)
        # The same replication command outside a C1 window is rejected.
        summary = audit.summarize(records, "postgres", [dict(windows[0], variant="c5")])
        self.assertEqual(summary["c5"]["rejected"], 1)

    def test_markers_alone_do_not_prove_hook_activity(self):
        markers = ["oo_audit_start_x", "oo_audit_T1_start_x", "oo_audit_T1_end_x", "oo_audit_end_x"]
        text = "\n".join(mysql_row("Query", "SELECT '" + m + "'") for m in markers)
        responses = {"SELECT @@GLOBAL": (0, "1\tTABLE")}

        def sql(statement, **kwargs):
            for prefix, value in responses.items():
                if statement.startswith(prefix):
                    return value
            return 0, text
        run = SimpleNamespace(engine="mysql", sql=sql)
        collector = audit.Audit(run)
        collector.markers = markers
        result = collector.collect()
        self.assertFalse(result["passed"])
        self.assertEqual(result["suite_activity"], {"T1": 0})


if __name__ == "__main__":
    unittest.main()
