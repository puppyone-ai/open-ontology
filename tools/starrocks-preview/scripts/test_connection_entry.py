"""Owner-private run directory acceptance. Import does not connect."""
import json
import os
import stat
import tempfile
import unittest
from pathlib import Path

import connection_whole


def mode(path):
    return stat.S_IMODE(path.stat().st_mode)


class OpenRunDir(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def manifest(self, state="running", digest=None, extra=None):
        run = self.root / "run"
        run.mkdir(mode=0o700)
        os.chmod(run, 0o700)
        driver = run / "driver-config.json"
        driver.write_text('{"host":"127.0.0.1"}\n')
        os.chmod(driver, 0o600)
        body = {
            "protocol": "oo.source-onboarding.run/v1",
            "state": state,
            "config_sha256": connection_whole.sha(driver) if digest is None else digest,
            "identity": {"pins": []},
        }
        (run / "journal.json").write_text(json.dumps(body))
        (run / "lock").write_text("")
        if extra:
            (run / extra).write_text("x")
        return run, driver

    def test_missing_path_is_created_private(self):
        path = self.root / "new"
        got = connection_whole.open_run_dir(path, path / "driver-config.json")
        self.assertEqual(got, path)
        self.assertEqual(mode(path), 0o700)
        self.assertEqual(os.listdir(path), [])

    def test_empty_owner_dir_is_accepted(self):
        path = self.root / "empty"
        path.mkdir(mode=0o700)
        os.chmod(path, 0o700)
        self.assertEqual(connection_whole.open_run_dir(path, path / "unused"), path)
        self.assertEqual(os.listdir(path), [])

    def test_matching_manifest_is_not_rewritten(self):
        run, driver = self.manifest()
        (run / "0.stdout").write_text("keep")
        (run / "0.stderr").write_text("keep-err")
        before = {name: (run / name).read_bytes() for name in os.listdir(run)}
        connection_whole.open_run_dir(run, driver)
        after = {name: (run / name).read_bytes() for name in os.listdir(run)}
        self.assertEqual(before, after)

    def test_extra_file_is_refused(self):
        run, driver = self.manifest(extra="whole.json")
        with self.assertRaisesRegex(RuntimeError, "not an onboard manifest"):
            connection_whole.open_run_dir(run, driver)

    def test_group_writable_is_refused(self):
        path = self.root / "open"
        path.mkdir(mode=0o700)
        os.chmod(path, 0o770)
        with self.assertRaisesRegex(RuntimeError, "not owner-private"):
            connection_whole.open_run_dir(path, path / "unused")
        self.assertEqual(mode(path), 0o770)

    def test_failed_and_completed_journals_are_refused(self):
        for state in ("failed", "recipe_completed"):
            run, driver = self.manifest(state=state)
            with self.assertRaisesRegex(RuntimeError, "not running"):
                connection_whole.open_run_dir(run, driver)
            run.rename(self.root / ("done-" + state))

    def test_config_path_mismatch_is_refused(self):
        run, _driver = self.manifest()
        other = self.root / "other.json"
        other.write_text("{}\n")
        with self.assertRaisesRegex(RuntimeError, "config path mismatch"):
            connection_whole.open_run_dir(run, other)

    def test_sha_mismatch_is_refused(self):
        run, driver = self.manifest(digest="0" * 64)
        with self.assertRaisesRegex(RuntimeError, "sha mismatch"):
            connection_whole.open_run_dir(run, driver)

    def test_symlink_is_refused(self):
        real = self.root / "real"
        real.mkdir(mode=0o700)
        os.chmod(real, 0o700)
        link = self.root / "link"
        os.symlink(real, link)
        with self.assertRaisesRegex(RuntimeError, "symlink"):
            connection_whole.open_run_dir(link, real / "driver-config.json")
        self.assertEqual(os.listdir(real), [])


if __name__ == "__main__":
    unittest.main()
