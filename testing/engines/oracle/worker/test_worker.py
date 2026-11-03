import io
import json
import unittest

from worker import MAX_CONTROL, PROTOCOL, control, error_status


class ControlTests(unittest.TestCase):
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
