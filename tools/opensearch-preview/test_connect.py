"""Synthetic boundary checks; these are not live-source acceptance evidence."""
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import connect


class ConnectionRefusals(unittest.TestCase):
    def test_invalid_connections_refuse_before_capture(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            private = root / 'secret'
            private.mkdir()
            credential = private / 'identities.env'
            credential.write_text('reader_user=reader\nreader_password=synthetic-test-only\n')
            credential.chmod(0o600)
            (private / 'endpoint.url').write_text('https://source.example:9200\n')
            reader = root / 'reader'
            reader.write_bytes(b'\x7fELFsynthetic-test-only')
            schedule = root / 'schedule.json'
            schedule.write_text(json.dumps({'approved_index': 'approved', 'secret_ref': 'secret://test/source'}))
            base = {'type': 'opensearch', 'database': 'approved', 'host': 'source.example',
                    'port': 9200, 'user': 'reader', 'secret_ref': 'secret://test/source',
                    'password_file': str(credential), 'reader_bin': str(reader),
                    'reader_sha256': hashlib.sha256(reader.read_bytes()).hexdigest(),
                    'schedule_path': str(schedule), 'image': 'synthetic',
                    'image_sha256': 'sha256:synthetic', 'period_ms': 1000}
            for edit in ({'database': '*'}, {'user': 'other'}, {'host': 'different.example'},
                         {'reader_sha256': '0' * 64}, {'image_sha256': 'wrong'}, {'period_ms': 0}):
                with self.subTest(edit=edit):
                    path = root / 'config.json'
                    path.write_text(json.dumps(base | edit))
                    with patch('connect.subprocess.run') as invoke:
                        invoke.return_value.stdout = 'sha256:synthetic\n'
                        with self.assertRaises(ValueError):
                            connect.run(path, root / 'output', 2)
                    self.assertFalse((root / 'output').exists())


if __name__ == '__main__':
    unittest.main()
