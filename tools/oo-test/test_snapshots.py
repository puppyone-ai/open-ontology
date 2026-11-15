import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch, Mock
from types import SimpleNamespace

import runner
import snapshots


class SnapshotTests(unittest.TestCase):
    def test_cold_restart_waits_for_sql_readiness(self):
        run = object.__new__(snapshots.SnapshotRun)
        run.engine = "postgres"
        run.args = SimpleNamespace(health_timeout=10)
        run.sql = Mock(side_effect=[(2, ""), (0, "1")])
        with patch.object(snapshots.time, "sleep") as sleep:
            run.wait_ready()
        self.assertEqual(run.sql.call_count, 2)
        sleep.assert_called_once_with(1)
        run.engine = "oracle"
        run.sql = Mock(return_value=(1, ""))
        with patch.object(snapshots.time, "monotonic", side_effect=[0, 20]):
            with self.assertRaisesRegex(RuntimeError, "restart_health_timeout"):
                run.wait_ready()

    def test_oracle_wait_polls_at_two_minutes_and_keeps_pending_evidence(self):
        with tempfile.TemporaryDirectory(dir=runner.ROOT / "evidence") as directory:
            receipt = Path(directory) / "polls.json"
            with patch.object(snapshots, "capacity",
                              side_effect=[RuntimeError("oracle_slot_occupied"), {}]), \
                 patch.object(snapshots.time, "monotonic", side_effect=[0, 1]), \
                 patch.object(snapshots.time, "sleep") as sleep:
                self.assertTrue(snapshots.wait_for_oracle(300, receipt))
                sleep.assert_called_once_with(120)
            with patch.object(snapshots, "capacity",
                              side_effect=RuntimeError("oracle_slot_occupied")), \
                 patch.object(snapshots.time, "monotonic", side_effect=[0, 1]), \
                 patch.object(snapshots.time, "sleep") as sleep:
                self.assertFalse(snapshots.wait_for_oracle(1, receipt))
                sleep.assert_not_called()
            with patch.object(snapshots, "capacity",
                              side_effect=RuntimeError("docker_memory_budget_exhausted")):
                with self.assertRaisesRegex(RuntimeError, "memory"):
                    snapshots.wait_for_oracle(300, receipt)

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

    def test_unreadable_datadir_socket_goes_through_the_helper(self):
        calls = []
        denied = PermissionError(13, "Permission denied", "mysql.sock")
        with patch.object(snapshots.Path, "is_symlink", side_effect=denied), \
             patch.object(snapshots, "helper",
                          side_effect=lambda *a: calls.append(a) or (0, "")):
            self.assertEqual(snapshots.remove_runtime_socket(Path("/x/d"), "mysql", "img"),
                             "root-helper")
            with self.assertRaises(PermissionError):
                snapshots.remove_runtime_socket(Path("/x/d"), "mysql")
        self.assertEqual(calls[0][1], [(Path("/x/d"), "/d", "rw")])
        # Only the entrypoint's own link is removed, never another symlink.
        self.assertIn('"$(readlink /d/mysql.sock)" = /var/run/mysqld/mysqld.sock', calls[0][2])
        with patch.object(snapshots.Path, "is_symlink", side_effect=denied), \
             patch.object(snapshots, "helper", return_value=(1, "")):
            with self.assertRaisesRegex(RuntimeError, "socket_failed"):
                snapshots.remove_runtime_socket(Path("/x/d"), "mysql", "img")

    def test_clone_failure_is_not_a_full_copy_fallback(self):
        with tempfile.TemporaryDirectory(dir=runner.ROOT / "evidence") as directory:
            root = Path(directory)
            (root / "source").mkdir()
            (root / "source/data").write_bytes(b"fixture")
            cow = "clonefile" if snapshots.sys.platform == "darwin" else "reflink"
            with patch.object(snapshots, "clone_method", return_value=cow), \
                 patch.object(snapshots, "clone_file", side_effect=OSError("no COW")):
                with self.assertRaises(OSError):
                    snapshots.clone_tree(root / "source", root / "clone")
            self.assertFalse((root / "clone/data").exists())

    def test_reflink_unsupported_falls_back_to_a_recorded_plain_copy(self):
        with tempfile.TemporaryDirectory(dir=runner.ROOT / "evidence") as directory:
            root = Path(directory)
            (root / "source/sub").mkdir(parents=True)
            (root / "source/sub/data").write_bytes(b"fixture" * 1000)
            (root / "source/sub/data").chmod(0o640)
            unsupported = OSError(snapshots.errno.EOPNOTSUPP, "Operation not supported")
            with patch.object(snapshots.fcntl, "ioctl", side_effect=unsupported), \
                 patch.object(snapshots.sys, "platform", "linux"):
                self.assertEqual(snapshots.clone_method(root, root), "copy")
                seconds, method = snapshots.clone_tree(root / "source", root / "clone")
            self.assertEqual(method, "copy")
            self.assertGreaterEqual(seconds, 0)
            copied = root / "clone/sub/data"
            self.assertEqual(copied.read_bytes(), b"fixture" * 1000)
            self.assertEqual(copied.stat().st_mode & 0o777, 0o640)
            self.assertEqual(sorted(p.name for p in root.iterdir()), ["clone", "source"])
            # Only "cannot share extents" selects the copy; other faults raise.
            with patch.object(snapshots.fcntl, "ioctl", side_effect=OSError(snapshots.errno.EIO, "io")), \
                 patch.object(snapshots.sys, "platform", "linux"):
                with self.assertRaises(OSError):
                    snapshots.clone_method(root, root)
            self.assertEqual(sorted(p.name for p in root.iterdir()), ["clone", "source"])

    @unittest.skipUnless(sys.platform == "darwin", "APFS clonefile")
    def test_macos_keeps_clonefile(self):
        with tempfile.TemporaryDirectory(dir=runner.ROOT / "evidence") as directory:
            root = Path(directory)
            (root / "source").mkdir()
            (root / "source/data").write_bytes(b"fixture")
            self.assertEqual(snapshots.clone_method(root, root), "clonefile")
            self.assertEqual(snapshots.clone_tree(root / "source", root / "clone")[1], "clonefile")

    def test_container_owned_tree_is_removed_by_a_root_helper(self):
        with tempfile.TemporaryDirectory(dir=runner.ROOT / "evidence") as directory:
            data = Path(directory) / ".snapshot-data"
            (data / "mysql").mkdir(parents=True)
            (data / "mysql/x.frm").write_bytes(b"frm")
            calls = []
            def helper(image, mounts, script):
                calls.append((image, mounts, script))
                (data / "mysql/x.frm").unlink()  # rmtree itself is patched here
                (data / "mysql").rmdir()
                return 0, ""
            denied = PermissionError(13, "Permission denied", "x.frm")
            with patch.object(snapshots.shutil, "rmtree", side_effect=denied), \
                 patch.object(snapshots, "helper", side_effect=helper):
                self.assertEqual(snapshots.remove_tree(data, "mysql@sha256:a"), "root-helper")
            self.assertFalse(data.exists())
            self.assertEqual(calls, [("mysql@sha256:a", [(data, "/t", "rw")],
                                      "find /t -mindepth 1 -delete")])
            # Without an image the permission error is not swallowed; a failed
            # helper is an error, never files silently left behind.
            data.mkdir()
            with patch.object(snapshots.shutil, "rmtree", side_effect=denied):
                with self.assertRaises(PermissionError):
                    snapshots.remove_tree(data)
                with patch.object(snapshots, "helper", return_value=(1, "")):
                    with self.assertRaisesRegex(RuntimeError, "helper_remove_failed"):
                        snapshots.remove_tree(data, "mysql@sha256:a")
            self.assertEqual(snapshots.remove_tree(Path(directory) / "absent"), "absent")

    def test_root_helper_is_short_lived_owned_and_offline(self):
        with patch.object(snapshots.t1, "LANE", "ta"), \
             patch.object(snapshots.subprocess, "run",
                          return_value=SimpleNamespace(returncode=0, stdout="", stderr="")) as run:
            snapshots.helper("img@sha256:a", [(Path("/x/src"), "/src", "ro"),
                                              (Path("/x/dst"), "/dst", "rw")], "true")
        command = run.call_args.args[0]
        for part in (["--rm"], ["--network", "none"], ["--user", "0"],
                     ["--label", "oo.lane=ta"], ["--entrypoint", "sh"]):
            self.assertTrue(any(command[i:i + len(part)] == part for i in range(len(command))))
        self.assertIn("type=bind,src=/x/src,dst=/src,readonly", command)
        self.assertIn("type=bind,src=/x/dst,dst=/dst", command)
        self.assertEqual(command[-3:], ["img@sha256:a", "-c", "true"])
        self.assertNotIn("sudo", command)

    def test_container_owned_clone_goes_through_the_helper(self):
        with tempfile.TemporaryDirectory(dir=runner.ROOT / "evidence") as directory:
            root = Path(directory)
            (root / "source").mkdir()
            for method, flag in (("reflink", "--reflink=always "), ("copy", "")):
                calls = []
                with patch.object(snapshots, "host_owned", return_value=False), \
                     patch.object(snapshots, "clone_method", return_value=method), \
                     patch.object(snapshots, "helper",
                                  side_effect=lambda *a: calls.append(a) or (0, "")):
                    _, recorded = snapshots.clone_tree(root / "source", root / method, "img")
                self.assertEqual(recorded, method + "+root-helper")
                self.assertTrue(calls[0][2].endswith(f"cp -a {flag}/src/. /dst/"))
                self.assertIn((root / "source", "/src", "ro"), calls[0][1])
            # A symlink seen inside the container is refused and the partial clone removed.
            with patch.object(snapshots, "host_owned", return_value=False), \
                 patch.object(snapshots, "clone_method", return_value="copy"), \
                 patch.object(snapshots, "helper", return_value=(3, "oo_symlink\n")):
                with self.assertRaisesRegex(ValueError, "external"):
                    snapshots.clone_tree(root / "source", root / "linked", "img")
            self.assertFalse((root / "linked").exists())
            with patch.object(snapshots, "host_owned", return_value=False):
                with self.assertRaisesRegex(RuntimeError, "not_host_owned"):
                    snapshots.clone_tree(root / "source", root / "noimage")

    def test_clone_method_is_recorded_in_the_timing_receipt(self):
        import json, timing
        with tempfile.TemporaryDirectory(dir=runner.ROOT / "evidence") as directory:
            run = object.__new__(snapshots.SnapshotRun)
            run.path = Path(directory)
            run.receipt = {"run_id": "test"}
            run.timing = timing.Timing(run, clock=lambda: 0.0)
            run.record_clone("restore_clone", 1.5, "copy+root-helper")
            saved = json.loads((run.path / "timing.json").read_text())
            self.assertEqual(saved["attributes"]["restore_clone"],
                             {"seconds": 1.5, "method": "copy+root-helper"})
            self.assertEqual(saved["format"], "oo-test-timing/v1")
            receipt = json.loads((run.path / "resource-receipt.json").read_text())
            self.assertEqual(receipt["snapshot"]["restore_clone"]["method"], "copy+root-helper")
            self.assertEqual(receipt["timing"]["attributes"]["restore_clone"]["seconds"], 1.5)

    def test_cleanup_reports_a_tree_it_could_not_remove(self):
        with tempfile.TemporaryDirectory(dir=runner.ROOT / "evidence") as directory:
            run = object.__new__(snapshots.SnapshotRun)
            run.path = Path(directory)
            run.data = run.path / ".snapshot-data"
            run.data.mkdir()
            run.snapshot_image, run.manifest = None, {"image": "img@sha256:a"}
            run.receipt = {}
            with patch.object(snapshots.t1.LaneRun, "cleanup", return_value=[]), \
                 patch.object(snapshots, "remove_tree",
                              side_effect=RuntimeError("snapshot_helper_remove_failed")) as remove:
                self.assertEqual(run.cleanup(), [".snapshot-data"])
            remove.assert_called_once_with(run.data, "img@sha256:a")
            self.assertEqual(run.receipt["snapshot"]["data_removal"], "failed:RuntimeError")
            with patch.object(snapshots.t1.LaneRun, "cleanup", return_value=[]), \
                 patch.object(snapshots, "remove_tree", return_value="root-helper"):
                self.assertEqual(run.cleanup(), [])
            self.assertEqual(run.receipt["snapshot"]["data_removal"], "root-helper")

    def test_host_caps_default_and_explicit(self):
        names = ("OO_TEST_MAX_ORACLE", "OO_TEST_MAX_OTHER_CONTAINERS", "OO_TEST_DOCKER_CAP_GB")
        clean = {k: v for k, v in os.environ.items() if k not in names}
        with patch.dict(os.environ, clean, clear=True):
            self.assertEqual(snapshots.host_caps(), {"oracle": 1, "other": 2, "memory_gb": 40})
        roomy = dict(clean, OO_TEST_MAX_ORACLE="2", OO_TEST_MAX_OTHER_CONTAINERS="16",
                     OO_TEST_DOCKER_CAP_GB="70")
        with patch.dict(os.environ, roomy, clear=True):
            self.assertEqual(snapshots.host_caps(), {"oracle": 2, "other": 16, "memory_gb": 70})
            with patch.object(snapshots.subprocess, "check_output",
                              side_effect=["gvenzl/oracle-free:23|oracle|ta\n", "8GiB / 94GiB\n"]):
                snapshots.capacity("oracle")
            with patch.object(snapshots.subprocess, "check_output",
                              side_effect=["sha256:a|mysql|ta\nsha256:b|postgres|ta\n",
                                           "60GiB / 94GiB\n"]):
                snapshots.capacity("mysql")
            with patch.object(snapshots.subprocess, "check_output",
                              side_effect=["", "69GiB / 94GiB\n"]):
                with self.assertRaisesRegex(RuntimeError, "memory_budget"):
                    snapshots.capacity("postgres")
        with patch.dict(os.environ, dict(clean, OO_TEST_MAX_ORACLE="0"), clear=True):
            with self.assertRaisesRegex(ValueError, "invalid_host_cap"):
                snapshots.host_caps()

    def test_oracle_slot_and_memory_fail_closed(self):
        names = ("OO_TEST_MAX_ORACLE", "OO_TEST_MAX_OTHER_CONTAINERS", "OO_TEST_DOCKER_CAP_GB")
        with patch.dict(os.environ, {k: v for k, v in os.environ.items() if k not in names},
                        clear=True):
            self._fail_closed()

    def _fail_closed(self):
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
