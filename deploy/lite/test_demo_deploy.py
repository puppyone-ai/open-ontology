"""Offline checks for the demo adapter; live acceptance is recorded separately."""
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from subprocess import CompletedProcess

import demo_deploy as demo


class DeploymentTests(unittest.TestCase):
    def test_compose_works_without_a_per_user_plugin_directory(self):
        with patch.object(demo.lite.shutil, "which", return_value="/bin/docker-compose"):
            self.assertEqual(demo.lite.docker_command(["compose", "version"], {}),
                             ["/bin/docker-compose", "version"])
        with patch.object(demo.lite.shutil, "which", return_value=None):
            self.assertEqual(demo.lite.docker_command(["compose", "version"], {}),
                             ["docker", "compose", "version"])

    def owner(self, root):
        return dict(project="oo-a-lite-" + hashlib.sha256(str(root).encode()).hexdigest()[:16],
                    lane="v03", image="sha256:" + "a" * 64, subnet="10.240.0.0/24")

    def test_owner_is_bound_to_state_directory_and_lane(self):
        root = Path("/demo/state")
        for change in ({"lane": "other"}, {"project": "oo-a-lite-unrelated"}):
            owner = dict(self.owner(root), **change)
            with patch.object(demo.lite, "read_private", return_value=json.dumps(owner)):
                with self.assertRaisesRegex(ValueError, "DEMO_OWNER"):
                    demo.Deployment(root)

    def test_cleanup_checks_all_labels_before_deletion(self):
        root = Path("/demo/state")
        with patch.object(demo.lite, "read_private", return_value=json.dumps(self.owner(root))):
            deployment = demo.Deployment(root)
        with patch.object(deployment, "resources", side_effect=ValueError("DEMO_OWNERSHIP")), \
             patch.object(demo, "command") as command:
            with self.assertRaises(ValueError):
                deployment.down()
            command.assert_not_called()

    def test_policy_does_not_modify_the_sealed_view_definition(self):
        root = Path("/demo/state")
        with patch.object(demo.lite, "read_private", return_value=json.dumps(self.owner(root))):
            deployment = demo.Deployment(root)
        with patch.object(deployment, "sql") as sql:
            deployment.install_demo_policy()
        statement = sql.call_args.args[1]
        self.assertIn("NOBYPASSRLS", statement)
        self.assertIn("FORCE ROW LEVEL SECURITY", statement)
        self.assertIn("current_setting(''role'')", statement)
        self.assertNotIn("CREATE OR REPLACE VIEW", statement)
        self.assertNotIn("TO oo_gateway_login", statement)

    def test_pending_candidate_is_not_a_general_error_bypass(self):
        pending = json.dumps(dict(state="pending")).encode()
        refusal = json.dumps(dict(error="publication_semantic_admission_required")).encode()
        for code, error, allowed in [(1, refusal, True), (2, refusal, False),
                                     (1, b'{"error":"other"}', False)]:
            result = CompletedProcess(["fixture"], code, pending, error)
            with patch.object(demo.subprocess, "run", return_value=result):
                if allowed:
                    self.assertEqual(demo.command(["fixture"], candidate=True), pending)
                else:
                    with self.assertRaises(ValueError):
                        demo.command(["fixture"], candidate=True)
                with self.assertRaises(ValueError):
                    demo.command(["fixture"])

    def test_absent_refusal_wal_cannot_be_fabricated_from_expected_deny(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            demo.lite.private_dir(root / "work")
            demo.lite.save_json(root / "owner.json", self.owner(root))
            demo.lite.save_json(root / "work/demo-publication.json",
                               dict(receipt_sha256="a" * 64, config_digest="b" * 64))
            deployment = demo.Deployment(root)
            with patch.object(deployment, "sql", return_value="1") as sql:
                with self.assertRaises(OSError):
                    deployment.collect_audit([dict(query_id="q_missing", denied=True)] * 4)
                self.assertEqual(sql.call_count, 1)  # Publication check only; no INSERT.

    def test_audit_metadata_requires_a_private_directory_and_safe_file(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            wal = root / "event.wal"
            wal.write_bytes(b'{"phase":"refuse"}')
            wal.chmod(0o644)
            self.assertEqual(demo.read_audit(wal), wal.read_bytes())
            wal.chmod(0o666)
            with self.assertRaises(ValueError):
                demo.read_audit(wal)
            wal.chmod(0o644)
            link = root / "link.wal"
            link.symlink_to(wal)
            with self.assertRaises(OSError):
                demo.read_audit(link)
            root.chmod(0o755)
            with self.assertRaises(ValueError):
                demo.read_audit(wal)


if __name__ == "__main__":
    unittest.main()
