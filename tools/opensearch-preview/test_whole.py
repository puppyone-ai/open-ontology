import json
from pathlib import Path
import tempfile
import unittest

import whole


class WholeReceipt(unittest.TestCase):
    def product(self):
        return {'protocol': 'oo.source-onboarding.product/v1', 'fixture_review': True, 'build_id': whole.BUILD,
                'stages': [{'stage': f'S{i}', 'status': 'succeeded'} for i in range(11)]}

    def test_receipt_requires_live_recovery_and_dialogue(self):
        dialogue = {'outcome': 'verified', 'turns': 3, 'query_ids': ['q_1']}
        recovery = {'recovery_verified': True, 'source_write_attempts': 0, 'watermark_before': 5, 'recovered_source_generation': 6}
        cancel = {'outcome': 'sent', 'sqlstate': '57014', 'rollback': True}
        phase = {'generation_before': 5, 'generation_after': 6, 'reader_mutations': 0}
        self.assertTrue(whole.completion(self.product(), dialogue, recovery, True, cancel, phase)['full_product_chain_complete'])
        self.assertFalse(whole.completion(self.product(), {'outcome': 'verified', 'turns': 1, 'query_ids': []}, recovery, True, cancel, phase)['full_product_chain_complete'])
        self.assertFalse(whole.completion(self.product(), dialogue, {'recovery_verified': False}, True, cancel, phase)['recovery_verified'])
        self.assertFalse(whole.completion(self.product(), dialogue, recovery, True, {'outcome': 'not_sent'}, phase)['live_cancel_verified'])

    def test_customer_connection_never_reaches_capture(self):
        cfg = dict(whole.EXPECTED, password_file='unused')
        cfg['host'] = 'customer.example'
        with self.assertRaises(ValueError):
            whole.require_connection(cfg)

    def test_poisoned_loader_digest_refused_at_put(self):
        class Proc:
            returncode = 1
            stderr = b'error=hash \n'
        self.assertTrue(whole.loader_put_refused(Proc(), {'source_generation': 5, 'applied': True}))
        self.assertFalse(whole.loader_put_refused(Proc(), {'source_generation': 6, 'applied': True}))
        Proc.returncode = 0
        self.assertFalse(whole.loader_put_refused(Proc(), {'source_generation': 5, 'applied': True}))

    def test_phase_floor_uses_birth_only_for_retained_generation_six(self):
        with tempfile.TemporaryDirectory() as directory:
            run = Path(directory)
            (run / 'recovery').mkdir()
            started = run.stat().st_birthtime + 1000
            self.assertEqual(whole.phase_floor(started, run), started)
            (run / 'recovery' / 'verdict.json').write_text(json.dumps({
                'recovery_verified': True, 'recovered_source_generation': 6, 'source_write_attempts': 0}))
            self.assertEqual(whole.phase_floor(started, run), run.stat().st_birthtime)
            (run / 'recovery' / 'verdict.json').write_text(json.dumps({
                'recovery_verified': True, 'recovered_source_generation': 6, 'source_write_attempts': 1}))
            self.assertEqual(whole.phase_floor(started, run), started)

    def test_resume_uses_only_retained_refusal_and_generation_six(self):
        with tempfile.TemporaryDirectory() as directory:
            out = Path(directory)
            (out / 'state-after-refusal').mkdir()
            (out / 'state-after-refusal' / 'stdout').write_text('{"source_generation":5,"applied":true}\n')
            self.assertIsNone(whole.resumed_recovery(out))
            (out / 'state-0').mkdir()
            (out / 'state-0' / 'stdout').write_text('{"source_generation":6,"applied":true}\n')
            self.assertTrue(whole.resumed_recovery(out)['recovery_verified'])


if __name__ == '__main__':
    unittest.main()
