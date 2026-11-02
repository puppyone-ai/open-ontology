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



def inputs(root, position):
    config = root / "source-config.json"
    state = root / "worker-state.json"
    write(config, {"source_id": "unit_pg_current", "capture": {"slot": "unit_owned_slot"},
                   "approved_objects": [{"schema": "warehouse", "table": "parcels"}]})
    write(state, {"protocol": "oo.jobs.native-state/v1", "applied": True,
                  "binding": "3dd3e1e9-4085-4b00-bc44-aed278a3cf1f", "source_id": "unit_pg_current",
                  "method": "C1", "pin": 8, "source_generation": 7, "applied_pos": str(position)})
    return ["--source-config", str(config), "--prior-state", str(state),
            "--prior-state-sha256", hashlib.sha256(state.read_bytes()).hexdigest()]

class NativeTransactionAdapterTest(unittest.TestCase):
    def test_one_table_transaction_uses_only_its_admitted_seal(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            table = ("warehouse", "parcels")
            arrow = b"admitted Arrow page"
            arrow_sha = hashlib.sha256(arrow).hexdigest()
            tombstones = []
            tombstones_sha = hashlib.sha256(b"[]").hexdigest()
            position = (int("0", 16) << 32) | int("4085240", 16)
            scope = {
                "source_id": "unit_pg_current", "slot": "unit_owned_slot", "xid": 42, "end_lsn": "0/4085240",
                "complete": True, "tables": [{
                    "logical_schema": table[0], "logical_table": table[1],
                    "source_generation": 8, "parent_source_generation": 7,
                    "arrow_sha256": arrow_sha, "tombstones_sha256": tombstones_sha,
                }],
            }
            source = root / "native-admit" / "warehouse.parcels" / "g8"
            source.parent.mkdir(parents=True)
            Path(str(source) + ".arrow").write_bytes(arrow)
            write(Path(str(source) + ".capture.json"), {
                "transaction_scope": scope, "cross_table_consistency": "T",
                "source_generation": 8, "logical_schema": table[0], "logical_table": table[1],
            })
            request = {
                "consistency_grade": "T", "cross_table_consistent": True,
                "logical_schema": table[0], "logical_table": table[1],
                "source_generation": 8, "parent_source_generation": 7,
                "source_id": "unit_pg_current", "snapshot": "snapshot-1",
                "input_digest": arrow_sha, "applied_pos": str(position),
                "tombstones": tombstones, "columns": [], "target": {},
            }
            write(Path(str(source) + ".request.json"), request)
            seal = root / "sealed-current" / "warehouse.parcels" / "8"
            write(seal / "decision.json", {"admitted": True, "blocks": []})
            write(seal / "registry.json", {"bindings": [{
                "source_generation": 8, "source_id": "unit_pg_current",
                "snapshot": "snapshot-1", "input_digest": arrow_sha,
                "transaction_scope": scope, "fields": [], "schema_digest": "schema",
                "type_digest": "types", "config_digest": "config",
                "capture_receipt_sha256": "capture", "admission_receipt_path": "receipt",
                "admission_receipt_sha256": "admission", "run_id": "run",
            }]})

            output = root / "producer.json"
            argv = ["native_transaction_adapter", "--root", str(root), "--binding",
                    "3dd3e1e9-4085-4b00-bc44-aed278a3cf1f", "--pin", "9", "--out", str(output)] + inputs(root, position - 1)
            previous = sys.argv
            try:
                sys.argv = argv
                with contextlib.redirect_stdout(io.StringIO()):
                    adapter.main()
            finally:
                sys.argv = previous

            body = json.loads(output.read_text())
            self.assertEqual(len(body["entries"]), 1)
            self.assertEqual(body["entries"][0]["logical_table"], "parcels")
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
                            "3dd3e1e9-4085-4b00-bc44-aed278a3cf1f", "--pin", "9",
                            "--out", str(root / "producer.json")] + inputs(root, 0)
                with self.assertRaisesRegex(ValueError, "transaction table set"):
                    adapter.main()
            finally:
                sys.argv = previous


    def test_next_scope_never_skips_an_unacknowledged_commit(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for index in (1, 2, 3):
                write(root / "native-admit" / "warehouse.parcels" / f"g{index}.capture.json",
                      {"transaction_scope": {"end_lsn": f"0/{index:X}", "xid": index, "complete": True}})
            self.assertEqual(adapter.next_scope(root, 1)["xid"], 2)

    def test_current_parts_never_become_synthetic_native_receipts(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            write(root / "manifest.json", {"protocol": "oo.postgres-preview.capture-set/v1"})
            with self.assertRaisesRegex(ValueError, "refusing synthetic"):
                adapter.next_scope(root, 0)

if __name__ == "__main__":
    unittest.main()
