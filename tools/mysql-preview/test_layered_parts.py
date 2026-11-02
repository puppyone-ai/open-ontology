"""Offline producer contracts. Unit sealer doubles are not admission evidence."""
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('layered_parts_producer', HERE / 'layered-producer.py')
producer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(producer)


def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    data = value if isinstance(value, bytes) else (json.dumps(value) + '\n').encode()
    path.write_bytes(data)
    path.chmod(0o600)
    return producer.file_ref(path)


class Fixture:
    def __init__(self, root, counts=(2, 1), normalize=False):
        self.root = root
        self.capture = root / 'capture'
        self.capture.mkdir()
        self.out = root / 'state' / '00000000-0000-4000-8000-000000000001_1'
        self.out.mkdir(parents=True)
        self.pair = self.out.name
        self.occurrence = dict(binding='00000000-0000-4000-8000-000000000001', pin=1,
                               method='C5', c5_mode='layered')
        self.policy = dict(zero_date_to_null=False, strip_text_nul=normalize)
        driver = root / 'driver'
        admission = root / 'admission'
        write(driver, b'owned driver double, never executed')
        write(admission, b'owned unit sealer double, never executed')
        self.config = dict(
            driver=dict(path=str(driver), sha256=producer.digest(driver)),
            admission=dict(path=str(admission), sha256=producer.digest(admission)),
            state_dir=str(self.out.parent), prepare=str(HERE / 'prepare-native.py'),
            layered_config=str(root / 'layered.json'), target=None, max_arrow_bytes=1024 * 1024,
            secret_ref='secret://owned/reader', source_audit_policy='grant_enforced',
            registry_template=str(root / 'registries' / '{binding}-{pin}.json'))
        self.layered = dict(type_policy=self.policy, max_rows=2, max_bytes=1024 * 1024)
        write(root / 'layered.json', self.layered)
        write(root / 'config.json', self.config)
        self.snapshot = dict(kind='mysql_consistent_snapshot', value='00000000-0000-4000-8000-000000000002')
        self.audit = dict(source='grant_enforced', schema='owned', table='items', connection_id=12,
                          read_only_transaction=True, privileges=['SELECT'])
        self.identity = dict(engine='mysql', server_uuid='owned-server', version='5.7.44')
        write(self.capture / 'driver.json', dict(executable_path=str(driver),
                                                executable_sha256=producer.digest(driver)))
        write(self.capture / 'schema-preimage.json', [dict(table='items', columns=[])])
        write(self.capture / 'type-preimage.json', [])
        write(self.capture / 'capture-started.json', dict(run_id=self.snapshot['value']))
        self.raw = dict(
            protocol=producer.PARTS_PROTOCOL, source_engine='mysql', capture_method='c5_diff',
            source_id='owned', schema='owned', table='items', schema_fingerprint='a' * 64,
            chunk_topology_revision=1, chunk_id='0', lo=None, hi=None, prior_membership_revision=1,
            parent_generation=1, source_generation=2, part_count=len(counts), row_count=sum(counts),
            key_columns=['b', 'a'], type_policy=self.policy, delete_detection='reconcile',
            position_codec='snapshot-zero/v1', applied_pos='0', consistency_grade='B',
            cross_table_consistent=False, target=None, binding=self.occurrence['binding'], pin=1,
            c5_observation={'chunk': {'digest': [sum(counts), 'a', 'b']}},
            c5_completed_layers=[False] * 4, c5_capture_work=dict(rows=sum(counts), bytes=123))
        self.parts = []
        for ordinal, count in enumerate(counts):
            arrow = write(self.capture / f'part-{ordinal}.arrow', b'unit transport bytes ' + str(ordinal).encode())
            self.parts.append(dict(ordinal=ordinal, arrow=arrow, row_count=count))
        self.raw['parts_index'] = write(self.capture / 'parts.jsonl',
            b''.join((json.dumps(part) + '\n').encode() for part in self.parts))
        self.raw['input_digest'] = self.raw['parts_index']['sha256']
        self.complete = dict(
            protocol=producer.PARTS_PROTOCOL, snapshot=self.snapshot, snapshot_complete=True,
            admission_seal=False, source_audit=self.audit, source_identity=self.identity, connection_id=12,
            **{key: self.raw[key] for key in ('source_id', 'schema', 'table', 'source_generation',
                                             'parent_generation', 'parts_index', 'part_count', 'row_count')})
        write(self.capture / 'capture-complete.json', self.complete)
        for part in self.parts:
            ordinal = part['ordinal']
            notes = None
            if normalize and part['row_count']:
                reference = write(self.capture / f'notes-{ordinal}.json', [
                    dict(source_id='owned', schema='owned', table='items', column='note', row_index=0,
                         key_digest='f' * 64, reason='nul_byte', value_len=3, run_generation=2,
                         input_digest=part['arrow']['sha256'], raw_value_recorded=False)])
                notes = {key: reference[key] for key in ('path', 'sha256')}
            source = write(self.capture / f'part-{ordinal}.source.json',
                           dict(ordinal=ordinal, run_id=self.snapshot['value'], connection_id=12,
                                normalization_rejects=notes))
            scope = dict(protocol=producer.PART_PROTOCOL,
                         range={key: self.raw[key] for key in producer.RANGE_KEYS},
                         parts_index=self.raw['parts_index'], part_count=len(counts), total_rows=sum(counts),
                         part=part, snapshot=self.snapshot)
            facts = dict(
                protocol='oo.mysql.full-capture/v1', source_engine='mysql', source_id='owned',
                source_generation=2, run_id=self.snapshot['value'], snapshot=self.snapshot,
                snapshot_complete=True, source_audit=self.audit, source_identity=self.identity,
                part_ordinal=ordinal, part_source=source, range_scope=scope, key_columns=['b', 'a'],
                type_policy=self.policy, normalization_rejects=notes, observed_privileges=['SELECT'],
                config_sha256='c' * 64, max_rows=2, max_bytes=1024 * 1024,
                driver_path=str(self.capture / 'driver.json'),
                schema_preimage_path=str(self.capture / 'schema-preimage.json'),
                type_preimage_path=str(self.capture / 'type-preimage.json'),
                tables=[dict(name='items', schema='owned', arrow=f'part-{ordinal}.arrow',
                             arrow_path=part['arrow']['path'], arrow_sha256=part['arrow']['sha256'],
                             row_count=part['row_count'], route='C5', consistency='B',
                             delete_mode='reconcile', cursor_safety='no_cursor', quality='RANGE_SNAPSHOT')])
            write(self.capture / f'part-{ordinal}.capture' / 'capturer.json', facts)
        self.calls = {'capture': 0, 'prepare': 0, 'seal': 0}
        self.fail_prepare = None
        self.fail_seal = None
        self.original_call = producer.call

    def call(self, argv, out, payload=None):
        if argv[0] == self.config['driver']['path']:
            self.calls['capture'] += 1
            return json.dumps(self.raw).encode()
        if argv[0] == self.config['admission']['path']:
            self.calls['seal'] += 1
            report_path = Path(argv[argv.index('--report') + 1])
            config_path = Path(argv[argv.index('--config') + 1])
            report = producer.load(report_path)
            witness = producer.load(report['capture_witness'])
            facts = producer.load(witness['capturer_path'])
            ordinal = facts['part_ordinal']
            if ordinal == self.fail_seal:
                raise RuntimeError('injected seal failure')
            sealed = Path(argv[argv.index('--out') + 1])
            sealed.mkdir()
            record = dict(
                protocol='oo.source-admission.capture-binding/v1', source_engine='mysql',
                source_id='owned', source_generation=2, range_scope=facts['range_scope'],
                snapshot=facts['snapshot'], run_id=facts['run_id'], driver_path=self.config['driver']['path'],
                driver_digest=self.config['driver']['sha256'],
                schema_digest=producer.digest(facts['schema_preimage_path']),
                type_digest=producer.digest(facts['type_preimage_path']),
                config_digest=producer.digest(config_path), input_digest=self.parts[ordinal]['arrow']['sha256'],
                fields=[dict(name='a', arrow_type='binary', nullable=False, key=True),
                        dict(name='b', arrow_type='utf8', nullable=False, key=True)],
                objects=[dict(logical_schema='owned', logical_table='items',
                              row_count=self.parts[ordinal]['row_count'],
                              arrow_sha256=self.parts[ordinal]['arrow']['sha256'])])
            for name, path in [('report', report_path), ('config', config_path),
                               ('capture_receipt', Path(witness['capturer_path'])),
                               ('catalog', sealed / 'catalog.json'),
                               ('admission_receipt', sealed / 'admission-receipt.json')]:
                if not path.exists():
                    write(path, {'unit_test_double_not_real_admission': True})
                record[name + '_path'] = str(path)
                record[name + '_sha256'] = producer.digest(path)
            write(sealed / 'registry.json',
                  dict(protocol='oo.source-admission.binding-registry/v1', bindings=[record]))
            return b''
        self.calls['prepare'] += 1
        capture = Path(argv[argv.index('--capture') + 1])
        ordinal = producer.load(capture / 'capturer.json')['part_ordinal']
        if ordinal == self.fail_prepare:
            inputs = Path(argv[argv.index('--out') + 1])
            inputs.mkdir()
            write(inputs / 'partial.json', {'incomplete': True})
            raise RuntimeError('injected prepare failure')
        return self.original_call(argv, out, payload)

    def run(self):
        with patch.object(producer, 'call', side_effect=self.call):
            return producer.multipart_request(self.config, self.occurrence, self.raw,
                                               self.out, self.pair, self.layered)

    def main(self):
        output = io.StringIO()
        with patch.object(producer, 'call', side_effect=self.call), \
                patch.object(sys, 'argv', ['producer', '--config', str(self.root / 'config.json')]), \
                patch.object(sys, 'stdin', io.StringIO(json.dumps(self.occurrence))), \
                contextlib.redirect_stdout(output):
            producer.main()
        return json.loads(output.getvalue())


class PartsProducer(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='c5-parts-unit-')
        self.root = Path(self.temp.name).resolve()
        self.mask = os.umask(0o077)

    def tearDown(self):
        os.umask(self.mask)
        self.temp.cleanup()

    def test_exact_transport_shape_and_real_prepare_wrapper(self):
        f = Fixture(self.root, normalize=True)
        result = f.main()
        expected = dict(f.raw, protocol=producer.REQUEST_PROTOCOL, admitted_index=result['admitted_index'])
        self.assertEqual(result, expected)
        self.assertNotIn('native_request', result)
        producer.verify_ref(result['admitted_index'])
        with Path(result['admitted_index']['path']).open() as stream:
            lines = [json.loads(line) for line in stream]
        self.assertEqual(len(lines), 2)
        for ordinal, line in enumerate(lines):
            self.assertEqual(set(line), {'scope', 'registry', 'native_request', 'type_policy', 'normalization_rejects'})
            self.assertEqual(line['scope']['part'], f.parts[ordinal])
            self.assertEqual(line['scope']['snapshot'], f.snapshot)
            self.assertEqual(line['native_request']['key_columns'], ['b', 'a'])
            self.assertEqual(line['native_request']['audit_digest'],
                             producer.digest(f.capture / f'part-{ordinal}.capture' / 'capturer.json'))
            self.assertIsNotNone(line['normalization_rejects'])
            producer.verify_ref(line['registry'])
            prepared = producer.load(f.out / f'part-{ordinal}' / 'prepared.json')
            witness = producer.load(prepared['witness']['path'])
            config = producer.load(prepared['config']['path'])
            self.assertEqual(witness['arrow_path'], f.parts[ordinal]['arrow']['path'])
            self.assertEqual(witness['schema_preimage_path'], str(f.capture / 'schema-preimage.json'))
            self.assertNotIn('multipart_anchor', witness)
            self.assertEqual(config['budget']['hard_limit'], 2)
            self.assertEqual(config['source_audit_policy'], 'grant_enforced')
        self.assertEqual(lines[0]['native_request']['batch_id'], f.pair)
        self.assertEqual(lines[1]['native_request']['batch_id'], f.pair)
        self.assertTrue(all('mirror' not in line['native_request'] for line in lines))
        descriptor = producer.load(next((self.root / 'registries').iterdir()))
        self.assertEqual(descriptor, dict(protocol='oo.source-admission.c5-parts-index/v1',
            admitted_index=result['admitted_index'], parts_index=f.raw['parts_index'],
            part_count=2, row_count=3, source_id='owned', source_generation=2))
        self.assertEqual(f.main(), result)
        self.assertEqual(f.calls, dict(capture=1, prepare=2, seal=2))

    def test_prepare_failure_resumes_without_recapture_and_keeps_failed_attempt(self):
        f = Fixture(self.root)
        f.fail_prepare = 1
        with self.assertRaisesRegex(RuntimeError, 'prepare failure'):
            f.main()
        self.assertFalse((f.out / 'request.json').exists())
        self.assertFalse((self.root / 'registries').exists())
        f.fail_prepare = None
        f.main()
        self.assertEqual(f.calls, dict(capture=1, prepare=3, seal=2))
        self.assertEqual(len(list((f.out / 'part-1').glob('prepare-*'))), 2)

    def test_seal_failure_reuses_prepared_inputs_without_recapture(self):
        f = Fixture(self.root)
        f.fail_seal = 1
        with self.assertRaisesRegex(RuntimeError, 'seal failure'):
            f.main()
        f.fail_seal = None
        f.main()
        self.assertEqual(f.calls, dict(capture=1, prepare=2, seal=3))

    def test_failed_first_prepare_cannot_adopt_changed_later_witness(self):
        f = Fixture(self.root)
        f.fail_prepare = 0
        with self.assertRaisesRegex(RuntimeError, 'prepare failure'):
            f.main()
        path = f.capture / 'part-1.capture' / 'capturer.json'
        path.write_bytes(path.read_bytes() + b' ')
        f.fail_prepare = None
        with self.assertRaisesRegex(RuntimeError, 'changed'):
            f.main()
        self.assertEqual(f.calls, dict(capture=1, prepare=1, seal=0))

    def test_preexisting_real_source_stdout_is_adopted_without_capture(self):
        f = Fixture(self.root)
        write(f.out / 'capture-call' / 'stdout', json.dumps(f.raw).encode())
        f.main()
        self.assertEqual(f.calls['capture'], 0)

    def test_source_reference_key_order_is_not_resume_drift(self):
        f = Fixture(self.root)
        for ordinal in range(len(f.parts)):
            path = f.capture / f'part-{ordinal}.capture' / 'capturer.json'
            facts = producer.load(path)
            facts['part_source'] = dict(sorted(facts['part_source'].items()))
            write(path, facts)
        first = f.main()
        self.assertEqual(f.main(), first)
        self.assertEqual(f.calls, dict(capture=1, prepare=2, seal=2))

    def test_empty_and_exact_page_have_no_aggregate_admission(self):
        for counts in [(0,), (2, 2), (100000, 100000, 1)]:
            with self.subTest(counts=counts), tempfile.TemporaryDirectory(dir=self.root) as name:
                f = Fixture(Path(name), counts)
                result = f.run()
                self.assertEqual(result['row_count'], sum(counts))
                self.assertEqual(f.calls['seal'], len(counts))

    def test_no_whole_file_helpers_in_multipart_path(self):
        f = Fixture(self.root)
        with patch.object(Path, 'read_bytes', side_effect=AssertionError('unbounded read')), \
                patch.object(Path, 'read_text', side_effect=AssertionError('unbounded read')):
            f.main()
            f.main()

    def test_index_order_digest_missing_and_count_fail_closed(self):
        for kind in ['order', 'hash', 'missing', 'count', 'unterminated', 'empty_nonempty', 'external', 'mode', 'symlink']:
            with self.subTest(kind=kind), tempfile.TemporaryDirectory(dir=self.root) as name:
                f = Fixture(Path(name))
                path = f.capture / 'parts.jsonl'
                if kind == 'order':
                    f.raw['parts_index'] = write(path, b''.join(
                        (json.dumps(part) + '\n').encode() for part in reversed(f.parts)))
                elif kind == 'hash':
                    f.raw['parts_index']['sha256'] = '0' * 64
                elif kind == 'missing':
                    Path(f.parts[1]['arrow']['path']).unlink()
                elif kind == 'count':
                    f.raw['row_count'] += 1
                elif kind == 'unterminated':
                    f.raw['parts_index'] = write(path, path.read_bytes().rstrip(b'\n'))
                elif kind == 'empty_nonempty':
                    f.parts[0]['row_count'] = 0
                    f.raw['parts_index'] = write(path, b''.join(
                        (json.dumps(part) + '\n').encode() for part in f.parts))
                elif kind == 'external':
                    f.parts[0]['arrow']['path'] = str(self.root / 'part-0.arrow')
                    f.raw['parts_index'] = write(path, b''.join(
                        (json.dumps(part) + '\n').encode() for part in f.parts))
                elif kind == 'mode':
                    path.chmod(0o644)
                else:
                    target = path.with_name('actual-index')
                    path.rename(target)
                    path.symlink_to(target)
                with self.assertRaises((RuntimeError, OSError)):
                    for _ in producer.iter_parts(f.raw):
                        pass

    def test_resume_rejects_source_policy_and_prepared_evidence_drift(self):
        for kind in ['arrow', 'notes', 'witness', 'prepare', 'layered', 'source', 'scope',
                     'registry', 'descriptor', 'request', 'request_protocol']:
            with self.subTest(kind=kind), tempfile.TemporaryDirectory(dir=self.root) as name:
                f = Fixture(Path(name), normalize=True)
                f.main()
                if kind == 'arrow':
                    path = Path(f.parts[1]['arrow']['path'])
                elif kind == 'notes':
                    path = f.capture / 'notes-1.json'
                elif kind == 'witness':
                    prepared = producer.load(f.out / 'part-1' / 'prepared.json')
                    path = Path(prepared['witness']['path'])
                elif kind == 'prepare':
                    prepared = producer.load(f.out / 'part-1' / 'prepared.json')
                    path = Path(prepared['config']['path'])
                elif kind == 'layered':
                    path = f.root / 'layered.json'
                elif kind == 'source':
                    path = f.out / 'source.json'
                elif kind == 'scope':
                    path = f.capture / 'part-1.capture' / 'capturer.json'
                elif kind == 'registry':
                    admitted = producer.load(f.out / 'part-1' / 'admitted.json')
                    path = Path(admitted['line']['registry']['path'])
                elif kind in ('request', 'request_protocol'):
                    path = f.out / 'request.json'
                else:
                    path = next((f.root / 'registries').iterdir())
                path.write_bytes(path.read_bytes() + b' ')
                if kind == 'request_protocol':
                    request = producer.load(path)
                    request['protocol'] = 'oo.jobs.native-c5-nochange/v1'
                    write(path, request)
                with self.assertRaises(RuntimeError):
                    f.main()
                self.assertEqual(f.calls, dict(capture=1, prepare=2, seal=2))

    def test_part_snapshot_audit_and_normalization_cannot_be_spliced(self):
        for key in ['snapshot', 'source_audit', 'normalization_rejects', 'range_scope']:
            with self.subTest(key=key), tempfile.TemporaryDirectory(dir=self.root) as name:
                f = Fixture(Path(name), normalize=True)
                path = f.capture / 'part-1.capture' / 'capturer.json'
                facts = producer.load(path)
                facts[key] = {}
                write(path, facts)
                with self.assertRaises(RuntimeError):
                    f.run()
                self.assertFalse((f.out / 'request.json').exists())

    def test_normalization_stream_has_no_total_row_quota(self):
        path = self.root / 'notes.json'
        with path.open('w') as stream:
            stream.write('[')
            for index in range(100001):
                stream.write((',' if index else '') + '{"reason":"nul_byte"}')
            stream.write(']')
        self.assertEqual(sum(1 for _ in producer.json_array(path)), 100001)
        for invalid in [b'[{}', b'[{},]', b'[{}]x', b'{}', b'[{"x":"' + b'x' * 65536 + b'"}]']:
            write(path, invalid)
            with self.assertRaises(RuntimeError):
                for _ in producer.json_array(path):
                    pass

    def test_nochange_legacy_cached_output_is_preserved(self):
        f = Fixture(self.root)
        f.raw = dict(protocol='oo.jobs.native-c5-nochange/v1', empty_diff=True)
        self.assertEqual(f.main(), f.raw)
        self.assertEqual(f.main(), f.raw)
        self.assertEqual(f.calls, dict(capture=1, prepare=0, seal=0))

    def test_legacy_one_part_still_emits_standard_registry_and_native_request(self):
        f = Fixture(self.root, counts=(2,))
        f.raw.update(protocol='oo.pipeline.native-c5-range/v1',
                     arrow_path=f.parts[0]['arrow']['path'], arrow_sha256=f.parts[0]['arrow']['sha256'],
                     members=[], normalization_rejects=None)
        for key in ('parts_index', 'part_count', 'input_digest'):
            f.raw.pop(key)
        facts = producer.load(f.capture / 'part-0.capture' / 'capturer.json')
        facts['range_scope'] = {key: f.raw[key] for key in (
            'protocol', *producer.RANGE_KEYS, 'capture_method', 'delete_detection',
            'position_codec', 'applied_pos', 'arrow_sha256', 'row_count', 'members')}
        write(f.capture / 'capturer.json', facts)
        result = f.main()
        self.assertEqual(result['protocol'], 'oo.pipeline.native-c5-range/v1')
        self.assertEqual(result['native_request']['batch_id'], f.pair)
        self.assertEqual(result['native_request']['arrow_path'], f.parts[0]['arrow']['path'])
        self.assertNotIn('admitted_index', result)
        registry = producer.load(next((self.root / 'registries').iterdir()))
        self.assertEqual(registry['protocol'], 'oo.source-admission.binding-registry/v1')
        self.assertEqual(f.main(), result)
        self.assertEqual(f.calls, dict(capture=1, prepare=1, seal=1))

    def test_real_prepare_refuses_relocated_part_witness(self):
        f = Fixture(self.root)
        capture = f.capture / 'wrong-location'
        write(capture / 'capturer.json', producer.load(f.capture / 'part-0.capture' / 'capturer.json'))
        directory = f.out / 'wrong-prepare'
        directory.mkdir()
        with self.assertRaisesRegex(RuntimeError, 'stage refused'):
            producer.prepare_part(f.config, capture, directory, 'items')
        self.assertFalse((directory / 'prepared.json').exists())


if __name__ == '__main__':
    unittest.main()
