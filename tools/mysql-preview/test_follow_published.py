import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from follow import Follow


@unittest.skipUnless(os.environ.get('OO_MYSQL_PUBLISHED_TEST_CONFIG'), 'explicit owned published-cut configuration required')
class PublishedReuse(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.config = json.loads(Path(os.environ['OO_MYSQL_PUBLISHED_TEST_CONFIG']).read_bytes())
        self.flow = Follow(self.config, self.temp.name)
        self.original = Path(self.flow.p['reuse_published_cut']['path'])
        self.request = json.loads((self.original/'native/request.json').read_bytes())
        self.result = json.loads((self.original/'native/load.json').read_bytes())

    def reuse(self):
        return self.flow.published_entries(self.original/'native', self.request, self.result)

    def test_actual_published_target_identity_and_types(self):
        entries = self.reuse()
        self.assertEqual(len(entries), 5)
        self.assertTrue((Path(self.temp.name)/'published-catalog-reuse.json').is_file())

    def test_changed_request_is_refused_before_target_query(self):
        altered = Path(self.temp.name)/'altered'
        altered.mkdir()
        for name in ['request', 'load', 'ack']:
            (altered/(name+'.json')).write_bytes((self.original/'native'/(name+'.json')).read_bytes())
        (altered/'request.json').write_text('{}')
        with patch.object(self.flow, 'call') as call:
            with self.assertRaisesRegex(ValueError, 'published_cut_identity'):
                self.flow.published_entries(altered, self.request, self.result)
            call.assert_not_called()

    def test_only_matching_successful_active_publication_is_reused(self):
        import shutil
        product = Path(self.temp.name)/'product'
        product.mkdir()
        for name in ['current-role-response.json','current-effective-oos.json','current-visibility.json','current-role-pack.json']:
            shutil.copy2(self.original/'product'/name, product/name)
        active = json.loads((self.original/'active-release.json').read_bytes())
        status = json.loads((self.original/'release-status.json').read_bytes())
        with patch('follow.Flow') as api:
            api.return_value.call.side_effect = [active, dict(status, status='failed')]
            with self.assertRaisesRegex(RuntimeError, 'published_release_drift'):
                self.flow.publish(4, {}, self.result)
        with patch('follow.Flow') as api:
            api.return_value.call.side_effect = [active, status]
            result = self.flow.publish(4, {}, self.result)
            self.assertEqual(result['pack_digest'], active['pack_digest'])
            self.assertEqual(api.return_value.call.call_count, 2)
            self.assertTrue((Path(self.temp.name)/'publication-reuse.json').is_file())

    def test_live_type_or_retired_run_drift_is_refused(self):
        actual = self.flow.call
        for drift in ['type', 'retired']:
            def changed(*args, **kwargs):
                result = json.loads(actual(*args, **kwargs))
                if drift == 'type':
                    next(a for a in result['attrs'] if a[0].startswith('pc_'))[1] = 25
                else:
                    result['run']['retired_at'] = '2026-10-16T00:00:00Z'
                return json.dumps(result).encode()
            with self.subTest(drift=drift), patch.object(self.flow, 'call', side_effect=changed):
                with self.assertRaisesRegex(ValueError, 'published_target_schema|published_target_state'):
                    self.reuse()


if __name__ == '__main__':
    unittest.main()
