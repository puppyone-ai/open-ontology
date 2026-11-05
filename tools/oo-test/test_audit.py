import csv
import io
import unittest
from types import SimpleNamespace
from unittest.mock import Mock

import audit


class AuditTests(unittest.TestCase):
    def test_postgres_audit_does_not_change_role_startup_settings(self):
        run = SimpleNamespace(engine="postgres", sql=Mock())
        audit.Audit(run).enable()
        sql = run.sql.call_args.args[0]
        self.assertIn("ALTER SYSTEM SET log_statement='all'", sql)
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
                    "SELECT * FROM pg_logical_slot_get_changes('slot', NULL, NULL)"):
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
                    "BEGIN DBMS_LOGMNR.START_LOGMNR(OPTIONS=>DBMS_LOGMNR.DICT_FROM_REDO_LOGS); END;"):
            self.assertFalse(audit.read_statement(sql), sql)

    def test_mysql_commands_and_multiline(self):
        sql = "SELECT\n1"
        text = "Connect\t" + "reader".encode().hex() + "\nQuery\t" + sql.encode().hex()
        self.assertEqual(audit.mysql_records(text), [sql])
        for text in ("bad", "Query\txyz", "Init DB\t666f6f"):
            with self.assertRaises(ValueError):
                audit.mysql_records(text)

    def test_pg_identity_error_and_csv(self):
        stream = io.StringIO()
        writer = csv.writer(stream)
        for user, message, error, statement in (
            ("postgres", "statement: INSERT INTO t VALUES(1)", "", ""),
            ("fixture_reader", "statement: SELECT\n1", "", ""),
            ("fixture_reader", "execute <unnamed>: SELECT 2", "", ""),
            ("fixture_reader", "denied", "ERROR", "DELETE FROM t"),
        ):
            row = [""] * 23
            row[1], row[13], row[11], row[19] = user, message, error, statement
            writer.writerow(row)
        self.assertEqual(audit.postgres_records(stream.getvalue()),
                         ["SELECT\n1", "SELECT 2", "DELETE FROM t"])
        with self.assertRaises(ValueError):
            audit.postgres_records("malformed")

    def test_oracle_missing_sql_is_not_a_pass(self):
        self.assertEqual(audit.oracle_records("LOGON|-\nSELECT|" + b"SELECT 1 FROM dual".hex()),
                         ["SELECT 1 FROM dual"])
        for text in ("SELECT|-", "CREATE TABLE|-", "SELECT|" + "41" * 4000):
            with self.assertRaises(ValueError):
                audit.oracle_records(text)

    def test_markers_alone_do_not_prove_hook_activity(self):
        markers = ["oo_audit_start_x", "oo_audit_T1_start_x", "oo_audit_T1_end_x", "oo_audit_end_x"]
        sql = ["SELECT '" + m + "'" for m in markers]
        text = "\n".join("Query\t" + s.encode().hex() for s in sql)
        run = SimpleNamespace(engine="mysql", sql=lambda *a, **k: (0, text))
        collector = audit.Audit(run)
        collector.markers = markers
        result = collector.collect()
        self.assertFalse(result["passed"])
        self.assertEqual(result["suite_activity"], {"T1": 0})


if __name__ == "__main__":
    unittest.main()
