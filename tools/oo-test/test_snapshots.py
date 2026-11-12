from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import runner
import snapshots


class SnapshotTests(unittest.TestCase):
    def test_key_covers_scale_tables_platform_and_generator(self):
        args = ("postgres", "17", 2000, 3, "linux/arm64")
        key, manifest = snapshots.identity(*args)
        self.assertIn("@sha256:", manifest["image"])
        for changed in (("postgres", "17", 2001, 3, "linux/arm64"),
                        ("postgres", "17", 2000, 4, "linux/arm64"),
                        ("postgres", "17", 2000, 3, "linux/amd64")):
            self.assertNotEqual(key, snapshots.identity(*changed)[0])
        original = Path.read_bytes
        def changed(path):
            data = original(path)
            return data + b"\n" if path.name == "generate.py" else data
        with patch.object(Path, "read_bytes", changed):
            self.assertNotEqual(key, snapshots.identity(*args)[0])

    def test_every_requested_version_has_an_immutable_identity(self):
        for engine, versions in (
            ("postgres", ("13", "14", "15", "16", "17", "18")),
            ("mysql", ("5.7", "8.0", "8.4", "9")),
            ("oracle", ("11", "19", "21", "23")),
        ):
            for version in versions:
                item = runner.resolve(engine, version)
                for platform in item["platforms"]:
                    key, value = snapshots.identity(engine, version, 2000, 3, platform)
                    self.assertEqual(len(key), 64)
                    self.assertIn("@sha256:", value["image"])

    def test_symlink_tree_refused(self):
        with tempfile.TemporaryDirectory(dir=runner.ROOT / "evidence") as directory:
            root = Path(directory)
            (root / "source").mkdir()
            (root / "source/link").symlink_to(root)
            with self.assertRaisesRegex(ValueError, "external"):
                snapshots.clone_tree(root / "source", root / "clone")
            self.assertFalse((root / "clone").exists())

    def test_only_the_mysql_runtime_socket_is_removed(self):
        with tempfile.TemporaryDirectory(dir=runner.ROOT / "evidence") as directory:
            root = Path(directory)
            socket = root / "mysql.sock"
            socket.symlink_to("/var/run/mysqld/mysqld.sock")
            snapshots.remove_runtime_socket(root, "postgres")
            self.assertTrue(socket.is_symlink())
            snapshots.remove_runtime_socket(root, "mysql")
            self.assertFalse(socket.is_symlink())
            socket.symlink_to(root / "tablespace")
            snapshots.remove_runtime_socket(root, "mysql")
            self.assertTrue(socket.is_symlink())

    def test_clone_failure_is_not_a_full_copy_fallback(self):
        with tempfile.TemporaryDirectory(dir=runner.ROOT / "evidence") as directory:
            root = Path(directory)
            (root / "source").mkdir()
            (root / "source/data").write_bytes(b"fixture")
            with patch.object(snapshots, "clone_file", side_effect=OSError("no COW")):
                with self.assertRaises(OSError):
                    snapshots.clone_tree(root / "source", root / "clone")
            self.assertFalse((root / "clone/data").exists())

    def test_oracle_slot_and_memory_fail_closed(self):
        with patch.object(snapshots.subprocess, "check_output",
                          return_value="gvenzl/oracle-xe:11.2.0.2\n"):
            with self.assertRaisesRegex(RuntimeError, "oracle_slot"):
                snapshots.capacity("oracle")
        with patch.object(snapshots.subprocess, "check_output",
                          side_effect=["postgres:17\n", "39GiB / 94GiB\n"]):
            with self.assertRaisesRegex(RuntimeError, "memory_budget"):
                snapshots.capacity("mysql")
        with patch.object(snapshots.subprocess, "check_output",
                          side_effect=["", "invalid\n"]):
            with self.assertRaisesRegex(RuntimeError, "unrecognized"):
                snapshots.capacity("postgres")
        with patch.object(snapshots.subprocess, "check_output",
                          return_value="sha256:opaque|oracle|ta\n"):
            with self.assertRaisesRegex(RuntimeError, "oracle_slot"):
                snapshots.capacity("oracle")
        with patch.object(snapshots.subprocess, "check_output",
                          return_value="sha256:a|mysql|ta\nsha256:b|postgres|ta\n"):
            with self.assertRaisesRegex(RuntimeError, "non_oracle_slots"):
                snapshots.capacity("mysql")

    def test_volume_creation_is_suppressed(self):
        run = object.__new__(snapshots.SnapshotRun)
        run.created_volume = True
        self.assertEqual(run.command(["docker", "volume", "create", "test"]), (0, ""))
        self.assertFalse(run.created_volume)

    def test_restore_uses_content_id_bind_and_memory_limit(self):
        run = object.__new__(snapshots.SnapshotRun)
        run.name, run.volume, run.engine = "test", "test-data", "postgres"
        run.data = Path("data")
        run.snapshot_image = "sha256:fixture"
        run.manifest = {"image": "postgres@sha256:base"}
        command = ["docker", "create", "--name", "test", "--mount",
                   "type=volume,src=test-data,dst=/var/lib/postgresql/data",
                   run.manifest["image"]]
        with patch.object(snapshots.t1.LaneRun, "command", return_value=(0, "")) as call:
            run.command(command)
        actual = call.call_args.args[0]
        self.assertIn("type=bind,src=data,dst=/var/lib/postgresql/data", actual)
        self.assertEqual(actual[-1], "sha256:fixture")
        self.assertEqual(actual[2:4], ["--memory", "1g"])
        self.assertIn("type=volume,src=test-data,dst=/var/lib/postgresql/data", command)


if __name__ == "__main__":
    unittest.main()
