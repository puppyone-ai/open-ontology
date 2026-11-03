import io
import json
import unittest

from worker import MAX_CONTROL, PROTOCOL, control


class ControlTests(unittest.TestCase):
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
