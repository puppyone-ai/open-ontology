"""Offline checks for the remote lite agent (no SSH, Docker or systemd)."""
import importlib.util
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import types
import unittest

SPEC = importlib.util.spec_from_file_location('remote_agent', Path(__file__).with_name('remote.py'))
remote = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(remote)


class RemoteAgent(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name)
        os.chmod(self.dir, 0o700)

    def tearDown(self):
        self.tmp.cleanup()

    def test_credentials_are_private_and_never_silently_replaced(self):
        changed = []
        path = self.dir / 'lite-ca-key'
        remote.put(path, b'one', changed, immutable=True)
        self.assertEqual(path.stat().st_mode & 0o777, 0o600)
        remote.put(path, b'one', changed, immutable=True)
        self.assertEqual(len(changed), 1)
        with self.assertRaises(remote.Refused) as raised:
            remote.put(path, b'two', changed, immutable=True)
        self.assertEqual(str(raised.exception), 'REMOTE_CREDENTIAL_CHANGE_REQUIRES_EXPLICIT_ROTATION')
        self.assertEqual(path.read_bytes(), b'one')
        os.chmod(path, 0o644)
        with self.assertRaises(remote.Refused):
            remote.read_private(path)

    def test_units_run_the_persisted_agent_on_the_stack_interval(self):
        units = remote.units('/usr/bin/python3', 3600)
        timer = units[remote.UNIT + '.timer']
        self.assertIn('OnUnitActiveSec=3600s', timer)
        self.assertIn('OnActiveSec=3600s', timer)
        self.assertIn('WantedBy=timers.target', timer)
        service = units[remote.UNIT + '.service']
        self.assertIn('Type=oneshot', service)
        self.assertTrue(service.splitlines()[-1].endswith('oo-remote.py cycle'))

    def test_requests_need_protocol_and_known_action(self):
        for body in ({'protocol': 'other', 'action': 'probe'},
                     {'protocol': remote.PROTOCOL, 'action': 'shell'}):
            raw = io.BytesIO((json.dumps(body) + '\n').encode())
            stdin = sys.stdin
            sys.stdin = types.SimpleNamespace(buffer=types.SimpleNamespace(raw=raw))
            try:
                with self.assertRaises(remote.Refused):
                    remote.serve()
            finally:
                sys.stdin = stdin

    def test_image_references_are_constrained(self):
        with self.assertRaises(remote.Refused):
            remote.load({'tag': 'evil;rm', 'expect': {}}, None)
        with self.assertRaises(remote.Refused):
            remote.pull({'ref': 'postgres:latest'}, None)


if __name__ == '__main__':
    unittest.main()
