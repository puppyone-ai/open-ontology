import importlib.util
import io
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("worker", Path(__file__).parent / "worker/worker.py")
worker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(worker)


class WorkerTest(unittest.TestCase):
    def test_connection_injection_and_remote_plaintext(self):
        valid = dict(host="127.0.0.1", port=50000, database="FIXTURE",
                     user="reader", password="fixture")
        self.assertIn("DATABASE=FIXTURE;", worker.dsn(valid))
        for changed in (dict(database="X;SECURITY=NONE"), dict(host="remote.invalid"),
                        dict(tls_ca="ca;UID=admin"), dict(password="x;UID=admin")):
            with self.assertRaises(ValueError):
                worker.dsn({**valid, **changed})

    def test_control_and_read_only_limits(self):
        self.assertIsNone(worker.control(io.BytesIO(b"")))
        for raw in (b"{}", b'{"protocol":"other"}\n', b"x" * (1024 * 1024 + 1)):
            with self.assertRaises(ValueError):
                worker.control(io.BytesIO(raw))
        valid = dict(sql="SELECT 1 FROM SYSIBM.SYSDUMMY1", operation="query",
                     max_rows=1, max_bytes=1024, timeout_ms=1000)
        worker.validate(valid)
        for change in (dict(sql="DELETE FROM T"), dict(sql="SELECT 1;DROP TABLE T"),
                       dict(max_rows=0), dict(max_bytes=0), dict(timeout_ms=0)):
            with self.assertRaises(ValueError):
                worker.validate({**valid, **change})
