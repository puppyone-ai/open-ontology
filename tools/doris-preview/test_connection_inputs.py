#!/usr/bin/env python3
"""Boundary checks for configured source inputs; these are not product receipts."""
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
PRIVATE = Path('/tmp/oo-doris-connection-whole')
spec = importlib.util.spec_from_file_location('capture', Path(__file__).with_name('connection_capture.py'))
capture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(capture)


class InputBoundary(unittest.TestCase):
    def setUp(self):
        self.config = json.loads((PRIVATE / 'run-source-final/driver-config.json').read_text())

    def test_invalid_identity_and_pin_before_network(self):
        capture.validate(self.config)
        for field, value in [('database', "db';DROP"), ('user', 'reader evil'), ('port', 0),
                             ('reader_sha256', '0' * 64), ('secret_ref', 'inline')]:
            config = dict(self.config, **{field: value})
            with self.subTest(field=field), self.assertRaises(ValueError):
                capture.validate(config)

    def test_prepared_input_binds_actual_rows_and_refuses_false_protocol(self):
        with tempfile.TemporaryDirectory(dir=PRIVATE) as directory:
            out = Path(directory) / 'prepared'
            args = [sys.executable, str(Path(__file__).with_name('prepare_connection_import.py')),
                    '--config', str(PRIVATE / 'import-config.json'), '--capture',
                    str(PRIVATE / 'run-source-final/capture/current.json'), '--out', str(out)]
            result = subprocess.run(args, capture_output=True)
            self.assertEqual(result.returncode, 0, result.stderr.decode())
            request = json.loads((out / 'uniq_acct/request-unsealed.json').read_text())
            self.assertEqual(request['logical_schema'], self.config['database'])
            self.assertEqual(request['columns'][1]['precision'], 18)
            self.assertEqual(request['columns'][1]['scale'], 4)
            self.assertTrue(request['columns'][1]['nullable'])
            wrong = json.loads((PRIVATE / 'run-source-final/capture/current.json').read_text())
            wrong['protocol'] = 'fixture'
            bad = Path(directory) / 'wrong.json'
            bad.write_text(json.dumps(wrong))
            args[args.index('--capture') + 1] = str(bad)
            args[-1] = str(Path(directory) / 'refused')
            self.assertNotEqual(subprocess.run(args, capture_output=True).returncode, 0)
            self.assertFalse(Path(args[-1]).exists())


if __name__ == '__main__':
    unittest.main()
