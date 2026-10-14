import contextlib
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest


SCRIPT = Path(__file__).parent / "linux" / "native_transaction_adapter.py"
SPEC = importlib.util.spec_from_file_location("native_transaction_adapter", SCRIPT)
adapter = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(adapter)


def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value))


class NativeTransactionAdapterTest(unittest.TestCase):
    def test_one_table_transaction_uses_only_its_admitted_seal(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            table = ("customer_east", "delivery_notes")
            arrow = b"admitted Arrow page"
            arrow_sha = hashlib.sha256(arrow).hexdigest()
            tombstones = []
            tombstones_sha = hashlib.sha256(b"[]").hexdigest()
            position = (int("0", 16) << 32) | int("4085240", 16)
            scope = {
                "source_id": "oo_dl01w1_46", "xid": 42, "end_lsn": "0/4085240",
                "complete": True, "tables": [{
                    "logical_schema": table[0], "logical_table": table[1],
                    "source_generation": 4, "parent_source_generation": 3,
                    "arrow_sha256": arrow_sha, "tombstones_sha256": tombstones_sha,
                }],
            }
            source = root / "native-admit" / "customer_east.delivery_notes" / "g4"
            source.parent.mkdir(parents=True)
            Path(str(source) + ".arrow").write_bytes(arrow)
            write(Path(str(source) + ".capture.json"), {
                "transaction_scope": scope, "cross_table_consistency": "T",
                "source_generation": 4, "logical_schema": table[0], "logical_table": table[1],
            })
            request = {
                "consistency_grade": "T", "cross_table_consistent": True,
                "logical_schema": table[0], "logical_table": table[1],
                "source_generation": 4, "parent_source_generation": 3,
                "source_id": "oo_dl01w1_46", "snapshot": "snapshot-1",
                "input_digest": arrow_sha, "applied_pos": str(position),
                "tombstones": tombstones, "columns": [], "target": {},
            }
            write(Path(str(source) + ".request.json"), request)
            seal = root / "sealed-current" / "customer_east.delivery_notes" / "4"
            write(seal / "decision.json", {"admitted": True, "blocks": []})
            write(seal / "registry.json", {"bindings": [{
                "source_generation": 4, "source_id": "oo_dl01w1_46",
                "snapshot": "snapshot-1", "input_digest": arrow_sha,
                "transaction_scope": scope, "fields": [], "schema_digest": "schema",
                "type_digest": "types", "config_digest": "config",
                "capture_receipt_sha256": "capture", "admission_receipt_path": "receipt",
                "admission_receipt_sha256": "admission", "run_id": "run",
            }]})

            adapter.target_watermark = lambda: (3, position - 1)
            output = root / "producer.json"
            argv = ["native_transaction_adapter", "--root", str(root), "--binding",
                    "3dd3e1e9-4085-4b00-bc44-aed278a3cf1f", "--pin", "1", "--out", str(output)]
            previous = sys.argv
            try:
                sys.argv = argv
                with contextlib.redirect_stdout(io.StringIO()):
                    adapter.main()
            finally:
                sys.argv = previous

            body = json.loads(output.read_text())
            self.assertEqual(len(body["entries"]), 1)
            self.assertEqual(body["entries"][0]["logical_table"], "delivery_notes")
            self.assertEqual(body["transaction_scope"], scope)

    def test_unapproved_transaction_table_is_refused(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            write(root / "native-admit" / "public.other" / "g1.capture.json", {
                "transaction_scope": {"end_lsn": "0/1", "xid": 1, "complete": True,
                                      "tables": [{"logical_schema": "public", "logical_table": "other",
                                                  "source_generation": 1}]},
            })
            previous = sys.argv
            try:
                sys.argv = ["native_transaction_adapter", "--root", str(root), "--binding",
                            "3dd3e1e9-4085-4b00-bc44-aed278a3cf1f", "--pin", "1",
                            "--out", str(root / "producer.json")]
                with self.assertRaisesRegex(ValueError, "transaction table set"):
                    adapter.main()
            finally:
                sys.argv = previous


if __name__ == "__main__":
    unittest.main()
