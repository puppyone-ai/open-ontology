import io
import json
import unittest

from worker import MAX_CONTROL, PROTOCOL, LOGMINER_CALLS, control, error_status, validate_sql


class ControlTests(unittest.TestCase):
    def test_logminer_exact_calls_only(self):
        for sql, kinds in LOGMINER_CALLS.items():
            request = {"operation": "logminer", "sql": sql,
                       "binds": [{kind: "100" if kind == "number" else "/redo/log"}
                                 for kind in kinds]}
            self.assertEqual(validate_sql(request), "logminer")
            for bad in [sql + " DELETE FROM APP.T", sql.replace("DBMS_LOGMNR.", "OTHER."),
                        sql.replace("ONLINE_CATALOG", "REDO_LOGS")]:
                if bad != sql:
                    with self.assertRaises(ValueError):
                        validate_sql({**request, "sql": bad})
            with self.assertRaises(ValueError):
                validate_sql({**request, "operation": "query"})
        for sql in ["DELETE FROM SYS.LOGMNR_GT_XID_INCLUDE$", "COMMIT",
                    "BEGIN DBMS_LOGMNR_D.BUILD; END;", "CREATE TABLE T(ID NUMBER)",
                    "SELECT 1 FROM DUAL; DELETE FROM T"]:
            with self.assertRaises(ValueError):
                validate_sql({"operation": "logminer", "sql": sql, "binds": []})
        self.assertEqual(validate_sql({"sql": "SELECT * FROM V$LOGMNR_CONTENTS"}), "query")

    def test_nested_timeout_preserves_only_closed_codes(self):
        class Detail:
            code = 0
            full_code = "DPY-4011"
            message = "DPY-4011: private text\nDPI-1080: more private text"
        result = error_status(Detail())
        self.assertTrue(result["timeout"])
        self.assertEqual(result["driver_codes"], ["DPY-4011", "DPI-1080"])
        self.assertNotIn("private", json.dumps(result))
        Detail.message = "DPY-4011: private text"
        self.assertFalse(error_status(Detail())["timeout"])

    def test_versioned_control_and_eof(self):
        request = {"protocol": PROTOCOL, "sql": "SELECT 1 FROM dual"}
        self.assertEqual(control(io.BytesIO(json.dumps(request).encode() + b"\n")), request)
        self.assertIsNone(control(io.BytesIO()))

    def test_reject_bad_version_truncation_and_oversized_control(self):
        for raw in [b'{"protocol":"other"}\n', b'{"protocol":', b"x" * (MAX_CONTROL + 1)]:
            with self.assertRaises(ValueError):
                control(io.BytesIO(raw))


if __name__ == "__main__":
    unittest.main()
