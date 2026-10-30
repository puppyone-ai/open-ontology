import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import current_cycle

ROOT = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("current_runtime", ROOT / "linux/native_transaction_producer_runtime.py")
runtime = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(runtime)


class CurrentCliTest(unittest.TestCase):
    def test_help_is_offline_from_unrelated_directory(self):
        commands = [
            ["connection_cycle.py"], ["current_cycle.py"], ["whole.py"], ["whole.py", "source"],
            ["producer_v2.py"], ["linux/native_transaction_adapter.py"],
            ["linux/native_transaction_producer_runtime.py"],
        ]
        with tempfile.TemporaryDirectory() as folder:
            for script, *args in commands:
                with self.subTest(script=script, args=args):
                    result = subprocess.run([sys.executable, str(ROOT / script), *args, "--help"],
                        cwd=folder, env={"PATH": "/nonexistent", "HOME": folder},
                        capture_output=True, timeout=5)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertIn(b"usage:", result.stdout.lower())

    def manifest(self, root):
        part = root / "wal.part-00000000.ndjson"
        part.write_bytes(b"source bytes\n")
        path = root / "manifest.json"
        path.write_text(json.dumps({
            "protocol": "oo.postgres-preview.capture-set/v1", "kind": "transactions",
            "complete": True, "slot_advanced": False, "target_applied": False, "worker_ack": False,
            "parts": [{"path": part.name, "bytes": part.stat().st_size,
                       "sha256": current_cycle.digest(part)}],
        }))
        return {"path": str(path), "sha256": current_cycle.digest(path)}

    def test_parts_are_verified_not_only_manifest(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            artifact = self.manifest(root)
            current_cycle.verify_manifest(artifact)
            (root / "wal.part-00000000.ndjson").write_bytes(b"changed")
            with self.assertRaisesRegex(ValueError, "part digest"):
                current_cycle.verify_manifest(artifact)

    def test_manifest_cannot_claim_target_ack(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            artifact = self.manifest(root)
            path = Path(artifact["path"])
            body = json.loads(path.read_bytes())
            body["worker_ack"] = True
            path.write_text(json.dumps(body))
            artifact["sha256"] = current_cycle.digest(path)
            with self.assertRaisesRegex(ValueError, "source-only"):
                current_cycle.verify_manifest(artifact)

    def test_part_cannot_escape_output(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            artifact = self.manifest(root)
            path = Path(artifact["path"])
            body = json.loads(path.read_bytes())
            body["parts"][0]["path"] = "../outside"
            path.write_text(json.dumps(body))
            artifact["sha256"] = current_cycle.digest(path)
            with self.assertRaisesRegex(ValueError, "outside"):
                current_cycle.verify_manifest(artifact)

    def test_failed_capture_never_advances_progress(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            # Existing stdlib process exits nonzero without running a source tool.
            with self.assertRaisesRegex(RuntimeError, "capture failed"):
                current_cycle.run_capture(Path(sys.executable), {"statement_ms": 1000},
                                          root / "attempt", "unknown-command")
            self.assertTrue((root / "attempt/exit.json").is_file())
            self.assertFalse((root / "attempt/capture-progress.json").exists())
            self.assertFalse((root / "attempt/capture/manifest.json").exists())


class RuntimeContextTest(unittest.TestCase):
    def setUp(self):
        self.context = {"protocol": "oo.jobs.native-producer/v1", "method": "C1", "cycle": "pull",
                        "binding": "new-owned-binding", "source_id": "new_pg", "pin": 29,
                        "acknowledged_source_generation": 17}
        self.output = {"protocol": "oo.jobs.native-transaction-producer/v1", "capture_method": "c1_cdc",
                       "binding": "new-owned-binding", "source_id": "new_pg", "pin": 29,
                       "source_generation": 18, "transaction_scope": {"complete": True}, "entries": [{}]}

    def emit(self, context, output=None, digest=None):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "producer.json"
            data = json.dumps(output or self.output).encode()
            path.write_bytes(data)
            return runtime.emit(context, path, digest or hashlib.sha256(data).hexdigest())

    def test_current_non_fixture_pin_and_generation(self):
        self.assertEqual(json.loads(self.emit(self.context)), self.output)

    def test_wrong_worker_contexts_refuse(self):
        for key, wrong in (("binding", "other"), ("source_id", "other"), ("pin", 1),
                           ("method", "C5"), ("cycle", "push"), ("acknowledged_source_generation", 18)):
            with self.subTest(key=key), self.assertRaises(ValueError):
                self.emit(dict(self.context, **{key: wrong}))

    def test_unpinned_or_unsealed_input_refuses(self):
        with self.assertRaisesRegex(ValueError, "changed"):
            self.emit(self.context, digest="0" * 64)
        with self.assertRaises(ValueError):
            self.emit(self.context, output={"protocol": "oo.postgres-preview.capture-set/v1"})


if __name__ == "__main__":
    unittest.main()
