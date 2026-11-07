#!/usr/bin/env python3
"""Scalar and composite identity controls end to end, without network or database.

    cargo build --locked -p oo-cli --bin oo -p oo-builder --bin selfbuild-budget
    python3 tools/release-coordinator/tests/test_retained_controls.py

Each control runs the real community runner (replay model) and the real
`oo review` binary with an accept-all owner decision file, and must reproduce
the committed fixtures byte for byte. The publisher half
(`reviewed`/`bind`/`gateway_document`, Pack recompilation) runs on those
fixtures in `cargo test --lib retained` of release-coordinator.
"""
import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import retained_fixture  # noqa: E402

FIXTURES = Path(__file__).resolve().parent / 'fixtures'


class RetainedControls(unittest.TestCase):
    def test_scalar_and_composite_controls_review_and_compile(self):
        for binary in ('oo', 'selfbuild-budget'):
            self.assertTrue((retained_fixture.BIN / binary).is_file(), binary + ' is not built')
        with tempfile.TemporaryDirectory() as tmp:
            files = retained_fixture.generate(Path(tmp) / 'controls')
        self.assertEqual(len(files), 4)
        for name, raw in files.items():
            self.assertEqual(raw, (FIXTURES / name).read_bytes(), name)
        keys = {}
        for name in ('retained_reviewed.json', 'retained_reviewed_composite.json'):
            reviewed = json.loads(files[name])
            self.assertEqual(reviewed['status'], 'reviewed')
            self.assertIs(reviewed['published'], False)
            [entity] = reviewed['compiled']['effective']['entities']
            keys[name] = (entity['key'], entity['display'])
        self.assertEqual(keys['retained_reviewed.json'], ('amount', 'amount'))
        self.assertEqual(keys['retained_reviewed_composite.json'],
                         (['order_id', 'line_no'], ['order_id', 'line_no']))


if __name__ == '__main__':
    unittest.main()
