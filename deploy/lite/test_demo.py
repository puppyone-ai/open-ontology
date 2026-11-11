"""Offline contracts; these tests do not claim Gateway or Docker acceptance."""
import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import demo_runtime as demo

spec = importlib.util.spec_from_file_location("curate", demo.ASSETS / "curate.py")
curate = importlib.util.module_from_spec(spec)
spec.loader.exec_module(curate)


class Assets(unittest.TestCase):
    def test_shared_adjudication_hash_vector(self):
        vector = json.loads((Path(__file__).resolve().parents[2] /
                             "execution/v0.3/contracts/hash-vector.json").read_bytes())
        document = vector["document"]
        summary = curate.adjudication_summary(document["graph"], document["items"])
        self.assertEqual(curate.canonical(summary), vector["preimage"])
        self.assertEqual(curate.digest(summary), vector["expected_sha256"])

    def test_assets_bind_input_and_decisions_and_hide_denied_column(self):
        manifest = demo.verify_assets()
        self.assertEqual(manifest["model_calls"], 0)
        adjudication = json.loads((demo.ASSETS / "adjudication.json").read_bytes())
        self.assertEqual(len(adjudication["graph"]["entities"]), 10)
        self.assertEqual(len(adjudication["graph"]["edges"]), 10)
        for role in ("analyst", "ops"):
            pack = json.loads((demo.ASSETS / f"pack-{role}.json").read_bytes())
            self.assertNotIn("phone_synthetic", json.dumps(pack))
            self.assertEqual(pack["protocol"], "oo.builder.role-pack/v1")
            for file in pack["files"]:
                self.assertEqual(len(file["text"].encode()), file["bytes"])
                self.assertEqual(curate.hashlib.sha256(file["text"].encode()).hexdigest(), file["sha256"])

    def test_tamper_and_symlink_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in demo.verify_assets()["files"]:
                shutil.copyfile(demo.ASSETS / name, root / name)
            shutil.copyfile(demo.ASSETS / "manifest.json", root / "manifest.json")
            (root / "seed.sql").write_text("changed")
            with self.assertRaisesRegex(ValueError, "DEMO_ASSET_DIGEST"):
                demo.verify_assets(root)
            (root / "seed.sql").unlink()
            (root / "seed.sql").symlink_to(demo.ASSETS / "seed.sql")
            with self.assertRaisesRegex(ValueError, "DEMO_ASSET_DIGEST"):
                demo.verify_assets(root)

    def test_curated_inputs_recompile_without_model(self):
        compiler = Path(__file__).resolve().parents[2] / "target/debug/selfbuild-budget"
        if not compiler.is_file():
            self.skipTest("build selfbuild-budget to run real compiler verification")
        for role in ("analyst", "ops"):
            command = json.loads((demo.ASSETS / f"confirmed-{role}.json").read_bytes())
            result = subprocess.run([compiler], input=curate.canonical(
                dict(operation="reviewed", command=command)).encode(), capture_output=True, check=True)
            compiled = json.loads(result.stdout)
            self.assertEqual(compiled["producer"]["artifact"],
                             json.loads((demo.ASSETS / f"pack-{role}.json").read_bytes()))
            command["confirmation"]["summary_sha256"] = "0" * 64
            result = subprocess.run([compiler], input=curate.canonical(
                dict(operation="reviewed", command=command)).encode(), capture_output=True)
            self.assertNotEqual(result.returncode, 0)


class Lifecycle(unittest.TestCase):
    def test_ownership_requires_all_labels(self):
        owner = dict(id="a" * 32, project="oo-demo-" + "a" * 32)
        labels = {"oo.lane": "m", "oo.demo": owner["id"], "com.docker.compose.project": owner["project"]}
        for kind in ("container", "volume", "network"):
            for missing in [None, *labels]:
                actual = labels.copy()
                if missing:
                    del actual[missing]
                info = {"Config": {"Labels": actual}} if kind == "container" else {"Labels": actual}
                with patch.object(demo, "run", return_value=json.dumps([info]).encode()):
                    if missing:
                        with self.assertRaisesRegex(ValueError, "DEMO_OWNERSHIP"):
                            demo.check_resource(kind, "owned", owner)
                    else:
                        demo.check_resource(kind, "owned", owner)

    def test_down_refuses_entire_set_before_delete(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with patch.object(demo, "owner_at", return_value={}), \
                 patch.object(demo, "resources", side_effect=ValueError("DEMO_OWNERSHIP")), \
                 patch.object(demo, "run") as run:
                with self.assertRaises(ValueError):
                    demo.down(root)
                run.assert_not_called()

    def test_private_files_reject_symlink_and_permissive_mode(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "file"
            demo.private_write(path, b"synthetic")
            self.assertEqual(demo.private_read(path), b"synthetic")
            path.chmod(0o644)
            with self.assertRaises(ValueError):
                demo.private_read(path)
            link = Path(directory) / "link"
            link.symlink_to(path)
            with self.assertRaises(OSError):
                demo.private_read(link)

    def test_state_rejects_outside_and_symlinked_parent(self):
        with self.assertRaises(ValueError):
            demo.state_path(Path.cwd().parent)
        with tempfile.TemporaryDirectory(dir=Path.cwd() / "evidence") as directory:
            root = Path(directory)
            (root / "link").symlink_to(root, target_is_directory=True)
            with self.assertRaises(ValueError):
                demo.state_path(root / "link" / "state")

    def test_owner_record_cannot_be_copied_to_another_directory(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            owner = dict(protocol="oo.demo-owner/v1", lane="m", id="a" * 32,
                         project="oo-demo-" + "a" * 32, image="sha256:" + "b" * 64,
                         root_sha256="0" * 64)
            demo.private_write(root / "owner.json", json.dumps(owner).encode())
            with self.assertRaisesRegex(ValueError, "DEMO_OWNER"):
                demo.owner_at(root)


if __name__ == "__main__":
    unittest.main()
