import importlib.util
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
import sys

spec = importlib.util.spec_from_file_location('layered_producer', Path(__file__).with_name('layered-producer.py'))
producer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(producer)


class CaptureRetry(unittest.TestCase):
    def test_failed_invocation_is_archived_and_same_checkpoint_can_retry(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            call = root / 'capture-call'
            call.mkdir()
            checkpoint = root / 'driver-checkpoint'
            checkpoint.write_bytes(b'same binding, pin, generation and offset')
            with self.assertRaisesRegex(RuntimeError, 'source stage refused'):
                producer.call([sys.executable, '-c',
                    'import sys; print("client_timeout",file=sys.stderr);sys.exit(1)'], call)
            failed = {p.name: p.read_bytes() for p in call.iterdir()}
            producer.archive_failed_capture(call)
            archive = next((root / 'capture-failures').iterdir())
            self.assertEqual({p.name: p.read_bytes() for p in archive.iterdir()}, failed)
            self.assertEqual(checkpoint.read_bytes(), b'same binding, pin, generation and offset')
            result = producer.call([sys.executable, '-c', 'print("{\\"status\\":\\"complete\\"}")'], call)
            self.assertEqual(json.loads(result), {'status': 'complete'})
            producer.archive_failed_capture(call)
            self.assertEqual(len(list((root / 'capture-failures').iterdir())), 1)
            self.assertEqual({p.name: p.read_bytes() for p in archive.iterdir()}, failed)

    def test_unknown_or_successful_capture_is_not_deleted(self):
        with tempfile.TemporaryDirectory() as folder:
            call = Path(folder) / 'capture-call'
            call.mkdir()
            (call / 'stdout').write_bytes(b'incomplete response')
            producer.archive_failed_capture(call)
            self.assertEqual((call / 'stdout').read_bytes(), b'incomplete response')
            producer.save(call / 'exit.json', 0)
            producer.archive_failed_capture(call)
            self.assertFalse((Path(folder) / 'capture-failures').exists())
            producer.save(call / 'exit.json', False)
            with self.assertRaisesRegex(RuntimeError, 'exit metadata'):
                producer.archive_failed_capture(call)

    def test_symlinked_history_is_refused(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            call = root / 'capture-call'
            call.mkdir()
            producer.save(call / 'exit.json', 1)
            target = root / 'elsewhere'
            target.mkdir()
            (root / 'capture-failures').symlink_to(target, target_is_directory=True)
            with self.assertRaisesRegex(RuntimeError, 'history directory'):
                producer.archive_failed_capture(call)
            self.assertTrue((call / 'exit.json').exists())

class RegistryTransport(unittest.TestCase):
    def test_sealed_registry_is_copied_byte_for_byte(self):
        with tempfile.TemporaryDirectory(prefix='oo-wf-c5e2e-registry-') as root:
            root = Path(root)
            sealed = root / 'sealed.json'
            registered = root / 'registered.json'
            sealed.write_bytes(b'{"bindings":[]}\n')
            producer.publish_registry(registered, sealed)
            self.assertEqual(registered.read_bytes(), sealed.read_bytes())
            producer.publish_registry(registered, sealed)
            sealed.write_bytes(b'{ "bindings":[] }\n')
            with self.assertRaisesRegex(RuntimeError, 'changed'):
                producer.publish_registry(registered, sealed)


class SourceMetadata(unittest.TestCase):
    def test_composite_primary_key_keeps_source_order_not_field_order(self):
        fields = [{'name': 'a', 'key': True}, {'name': 'b', 'key': True}, {'name': 'note', 'key': False}]
        raw = {'key_columns': ['b', 'a']}
        self.assertEqual(producer.source_key_columns(raw, raw, fields), ['b', 'a'])
        for wrong in [None, ['a'], ['b', 'b'], ['b', 'a', 'note'], ['b', 1]]:
            with self.subTest(keys=wrong), self.assertRaises(RuntimeError):
                producer.source_key_columns({'key_columns': wrong}, raw, fields)
        with self.assertRaises(RuntimeError):
            producer.source_key_columns(raw, {'key_columns': ['a', 'b']}, fields)

    def test_policy_is_closed_and_boolean(self):
        self.assertEqual(producer.type_policy(None), dict(zero_date_to_null=False, strip_text_nul=False))
        for wrong in [[], {'strip_text_nul': 1}, {'zero_date_to_null': 'true'}, {'unknown': True}]:
            with self.subTest(policy=wrong), self.assertRaises(RuntimeError):
                producer.type_policy(wrong)

    def test_normalization_notes_are_bound_masked_and_policy_checked(self):
        with tempfile.TemporaryDirectory(prefix='c5-normalization-') as folder:
            capture = Path(folder)
            path = capture / 'notes.json'
            policy = dict(zero_date_to_null=True, strip_text_nul=True)
            record = dict(source_id='owned', schema='fixture', table='items', column='note',
                          row_index=0, key_digest='a'*64, reason='nul_byte', value_len=4,
                          run_generation=2, input_digest='b'*64, raw_value_recorded=False)
            data = json.dumps([record]).encode()
            path.write_bytes(data)
            path.chmod(0o600)
            reference = dict(path=str(path), sha256=hashlib.sha256(data).hexdigest())
            raw = dict(type_policy=policy, normalization_rejects=reference,
                       source_id='owned', schema='fixture', table='items',
                       source_generation=2, arrow_sha256='b'*64, row_count=1)
            facts = dict(type_policy=policy, normalization_rejects=reference)
            expected = dict(type_policy=policy, normalization_rejects=reference)
            self.assertEqual(producer.normalization_metadata(raw, facts, capture, policy), expected)
            with self.assertRaisesRegex(RuntimeError, 'policy mismatch'):
                producer.normalization_metadata(raw, facts, capture, {})
            for field, bad in [('reason', 'bool_domain'), ('source_id', 'other'),
                               ('input_digest', 'c'*64), ('raw_value_recorded', True),
                               ('value_len', 'unmasked-text'), ('row_index', 1)]:
                changed_data = json.dumps([dict(record, **{field: bad})]).encode()
                path.write_bytes(changed_data)
                ref = dict(reference, sha256=hashlib.sha256(changed_data).hexdigest())
                with self.subTest(field=field), self.assertRaises(RuntimeError):
                    producer.normalization_metadata(dict(raw, normalization_rejects=ref),
                                                    dict(facts, normalization_rejects=ref), capture, policy)
            path.write_bytes(data)
            path.chmod(0o644)
            with self.assertRaisesRegex(RuntimeError, 'budget/path'):
                producer.normalization_metadata(raw, facts, capture, policy)
            path.chmod(0o600)
            path.write_bytes(data+b' ')
            with self.assertRaisesRegex(RuntimeError, 'digest'):
                producer.normalization_metadata(raw, facts, capture, policy)
            path.write_bytes(data)
            link = capture / 'linked.json'
            link.symlink_to(path)
            linked = dict(reference, path=str(link))
            with self.assertRaisesRegex(RuntimeError, 'budget/path'):
                producer.normalization_metadata(dict(raw, normalization_rejects=linked),
                                                dict(facts, normalization_rejects=linked), capture, policy)

    def test_no_notes_does_not_guess_a_stale_file(self):
        with tempfile.TemporaryDirectory() as folder:
            capture = Path(folder)
            (capture / 'rejects.json').write_text('[{"secret":"not-current-evidence"}]')
            result = producer.normalization_metadata({}, {}, capture, {})
            self.assertEqual(result, {'type_policy': {'zero_date_to_null': False, 'strip_text_nul': False}})

if __name__ == '__main__':
    unittest.main()
