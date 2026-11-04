import importlib.util
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
spec = importlib.util.spec_from_file_location('draft', HERE/'draft.py')
draft = importlib.util.module_from_spec(spec)
spec.loader.exec_module(draft)

class Tests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(dir=ROOT/'evidence')
        self.root = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def test_state_never_claims_publication_or_fabricates_metrics(self):
        state = draft.initial('fingerprint')
        self.assertEqual(state['publication'],'not_published')
        self.assertEqual(state['review'],'unreviewed')
        self.assertIsNone(state['next_due'])
        self.assertTrue(all(s['rows_per_second'] is None for s in state['stages']))

    def test_resume_rejects_changed_config_or_artifact(self):
        state = draft.initial('fingerprint')
        draft.write_private(self.root/'artifact',b'original')
        state['stages'][0]['artifacts'] = {'artifact':draft.digest(b'original')}
        draft.check_resume(state,'fingerprint',self.root)
        with self.assertRaisesRegex(ValueError,'RESUME_CONFIG_CHANGED'):
            draft.check_resume(state,'different',self.root)
        draft.write_private(self.root/'artifact',b'changed')
        with self.assertRaisesRegex(ValueError,'RESUME_ARTIFACT_CHANGED'):
            draft.check_resume(state,'fingerprint',self.root)

    def test_private_file_rejects_symlinks_hardlinks_and_public_modes(self):
        path = self.root/'file'
        draft.write_private(path,b'private')
        alias = self.root/'alias'
        alias.symlink_to(path)
        with self.assertRaises(OSError):
            draft.read_private(alias)
        alias.unlink()
        os.link(path,alias)
        with self.assertRaises(ValueError):
            draft.read_private(path)
        alias.unlink()
        path.chmod(0o644)
        with self.assertRaises(ValueError):
            draft.read_private(path)

    def test_process_group_cancellation_saves_failure_and_releases_lock(self):
        binary = self.root/'oo'
        binary.write_text('#!/usr/bin/env python3\nimport time\ntime.sleep(60)\n')
        binary.chmod(0o700)
        config = json.loads((ROOT/'crates/oo-stack/example.stack.json').read_text())
        request = {'stack':config,'state_dir':str(self.root/'state'),'bin_dir':str(self.root),
                   'resume':False,'egress_code':''}
        child = subprocess.Popen([sys.executable,str(HERE/'draft.py')],stdin=subprocess.PIPE,
                                 stdout=subprocess.PIPE,stderr=subprocess.PIPE,start_new_session=True)
        try:
            child.stdin.write(json.dumps(request).encode())
            child.stdin.close()
            path = self.root/'state/run.json'
            until = time.monotonic()+10
            while time.monotonic()<until:
                if path.exists() and json.loads(path.read_text())['stages'][0]['status']=='running':
                    break
                time.sleep(0.05)
            else:
                self.fail('stage not started')
            os.killpg(child.pid,signal.SIGTERM)
            self.assertEqual(child.wait(timeout=10),1)
            state = json.loads(path.read_text())
            self.assertEqual(state['status'],'failed')
            self.assertIn(state['stages'][0]['error'],('BUILD_INTERRUPTED','STAGE_COMMAND_FAILED'))
            self.assertTrue(state['last_failure'])
            with self.assertRaises(ProcessLookupError):
                os.killpg(child.pid,0)
        finally:
            if child.poll() is None:
                os.killpg(child.pid,signal.SIGKILL)
                child.wait()
            child.stdout.close()
            child.stderr.close()

if __name__ == '__main__':
    unittest.main()
