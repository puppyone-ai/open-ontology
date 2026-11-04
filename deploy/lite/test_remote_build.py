"""Offline checks for the remote runtime build action (D16 over SSH)."""
import importlib.util
import json
import os
from pathlib import Path
import tempfile
import types
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location('remote_agent', Path(__file__).with_name('remote.py'))
remote = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(remote)


class RemoteBuild(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        root = Path(self.tmp.name) / 'lite'
        state = root / 'work/build/.oo'
        state.mkdir(parents=True)
        (root / 'work/operator').mkdir()
        for path in (Path(self.tmp.name), root, root / 'work', root / 'work/build', state):
            os.chmod(path, 0o700)
        self.patch = patch.object(remote, 'ROOT', root)
        self.patch.start()
        self.root, self.state = root, state

    def tearDown(self):
        self.patch.stop()
        self.tmp.cleanup()

    def provision(self):
        remote.put(self.root / 'work/operator/build-runtime.json', b'{}', [])
        remote.put(self.root / 'owner.json', json.dumps(
            {'project': 'oo-a-lite-x', 'image': 'sha256:' + 'a' * 64, 'lane': 'a4'}).encode(), [])

    def test_refuses_before_provisioning(self):
        with self.assertRaisesRegex(remote.Refused, 'BUILD_LITE_NOT_PROVISIONED'):
            remote.build({}, None)

    def test_runs_one_hardened_runtime_phase_and_returns_review_input(self):
        self.provision()
        calls = []

        def run(argv, timeout=180, **kw):
            calls.append(argv)
            if argv[:2] == ['docker', 'inspect']:
                return types.SimpleNamespace(returncode=1, stdout=b'')
            if argv[:2] == ['docker', 'ps']:
                service = argv[-3].rsplit('=', 1)[1]
                return types.SimpleNamespace(returncode=0, stdout=b'agentid\n' if service == 'agent-db' else b'')
            remote.put(self.state / 'chain.json', json.dumps({'status': 'blocked'}).encode(), [])
            remote.put(self.state / 'review-input.json', b'{"draft":1}', [])
            return types.SimpleNamespace(returncode=1, stdout=b'')
        with patch.object(remote, 'run', side_effect=run):
            reply = remote.build({'fresh': True}, None)
        phase = calls[-1]
        self.assertEqual(phase[:2], ['docker', 'run'])
        self.assertIn('oo-a-lite-x_default', phase)  # no Gateway yet
        for flag in ('--read-only', '--cap-drop', 'no-new-privileges:true', 'oo.lane=a4'):
            self.assertIn(flag, phase)
        self.assertNotIn('--resume', phase)
        self.assertEqual(reply['review_input'], {'draft': 1})
        with self.assertRaisesRegex(remote.Refused, 'BUILD_EXISTS_USE_RESUME'):
            remote.build({'fresh': True}, None)

    def test_handoff_never_replaces_a_different_registry(self):
        self.provision()
        ok = types.SimpleNamespace(returncode=0, stdout=b'agentid\n')
        with patch.object(remote, 'run', return_value=ok):
            self.assertTrue(remote.handoff({'reviewed': {'status': 'reviewed'}}, None)['handed_off'])
            with self.assertRaisesRegex(remote.Refused, 'BUILD_REVIEW_INPUT_CHANGED'):
                remote.handoff({'reviewed': {'status': 'other'}}, None)


if __name__ == '__main__':
    unittest.main()
