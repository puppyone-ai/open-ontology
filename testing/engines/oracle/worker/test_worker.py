import io
import json
import unittest
import time

from worker import MAX_CONTROL, PROTOCOL, LOGMINER_CALLS, LobBudget, control, error_status, read_lob, validate_sql


class ControlTests(unittest.TestCase):
    def test_probe_only_accepts_a_read(self):
        self.assertEqual(validate_sql({"operation": "probe", "sql": "SELECT C FROM APP.T"}),
                         "probe")
        for sql in ("UPDATE APP.T SET C=NULL", "SELECT C FROM APP.T; DELETE FROM APP.T",
                    "BEGIN DBMS_LOGMNR.END_LOGMNR; END;"):
            with self.assertRaises(ValueError):
                validate_sql({"operation": "probe", "sql": sql})

    def test_lob_bounds_and_utf16_offsets(self):
        class Lob:
            def __init__(self, value):
                self.value = value
                self.binary = isinstance(value, bytes)
                self.data = value if self.binary else value.encode("utf-16-le")
            def size(self):
                return len(self.data) // (1 if self.binary else 2)
            def read(self, offset, amount):
                unit = 1 if self.binary else 2
                data = self.data[(offset-1)*unit:(offset-1+amount)*unit]
                return data if self.binary else data.decode("utf-16-le")
        for value in ("\U0001f600" * 9000, "\u4e2d" * 9000, "", bytes(range(256)) * 100):
            binary = isinstance(value, bytes)
            expected = value.hex() if binary else value
            limit = len(expected.encode())
            deadline = time.monotonic() + 10
            self.assertEqual(read_lob(Lob(value), binary, limit, deadline), expected)
            if limit:
                with self.assertRaises(LobBudget):
                    read_lob(Lob(value), binary, limit-1, deadline)
                with self.assertRaises(TimeoutError):
                    read_lob(Lob(value), binary, limit, 0)

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
