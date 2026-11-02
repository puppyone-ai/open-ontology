import copy
import unittest
from unittest.mock import patch

import library_runtime as runtime
import library_selfbuild as lib


class RuntimeBundleTests(unittest.TestCase):
    def documents(self):
        effective = {'project': 'shop'}
        visibility = {'role': 'oo_r_admin', 'principal': 'admin',
                      'datasets': [{'dataset': 'app.items'}]}
        review = {'project': 'shop', 'role': 'oo_r_admin', 'principal': 'admin',
                  'review_origin': 'operator_pinned_file'}
        producer = {'protocol': 'oo.selfbuild.library-pack/v1', 'business_owner_signed': False,
                    'published': False, 'artifact': {'pack_digest': 'digest'}, 'pack_digest': 'digest',
                    'effective_sha256': lib.digest(effective),
                    'visibility_sha256': lib.digest(visibility), 'review_sha256': lib.digest(review)}
        binding = {'bindings': [{'dataset': 'app.items'}],
                   'publication': {'receipt': {}, 'receipt_sha256': lib.digest({})},
                   'login': 'reader', 'ceilings': {}}
        delivery = {'stage': 'packaged', 'pack_digest': 'digest'}
        return [producer, effective, visibility, review, binding, delivery]

    @patch.object(lib, 'verify_pack_artifact')
    def test_local_bundle_does_not_forge_api_authentication_or_publish(self, verify):
        result = runtime.bundle(*self.documents())
        verify.assert_called_once()
        envelope = result['envelope']
        self.assertEqual(envelope['management_identity'], 'operator_local')
        self.assertEqual(envelope['contract'], 'oo.operator.library-pack/v1')
        self.assertFalse(envelope['active'])
        self.assertFalse(envelope['producer_authenticated'])
        self.assertEqual(envelope['data_grants'], 'none')

    @patch.object(lib, 'verify_pack_artifact')
    def test_drift_missing_bindings_and_forged_approval_fail(self, _verify):
        documents = self.documents()
        mutations = [(0, 'review_sha256', 'wrong'), (0, 'business_owner_signed', True),
                     (1, 'project', 'other'), (2, 'role', 'oo_r_other'),
                     (3, 'review_origin', 'untrusted'), (4, 'bindings', []),
                     (4, 'bindings', [{'dataset': 'app.items'}] * 2),
                     (5, 'stage', 'draft')]
        for index, key, value in mutations:
            bad = copy.deepcopy(documents)
            bad[index][key] = value
            with self.assertRaises(ValueError):
                runtime.bundle(*bad)


if __name__ == '__main__':
    unittest.main()
