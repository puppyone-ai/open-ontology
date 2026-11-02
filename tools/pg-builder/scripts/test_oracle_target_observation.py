"""Synthetic target peers; actual target evidence is produced by the CLI, not fixtures."""
import copy
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock

import library_selfbuild as lib
import oracle_observation as oracle
import oracle_target_observation as target
from test_oracle_observation import fixture, rehash


def measured_fixture(root, raw_names=False):
    docs, metadata, aggregate = fixture()
    # This dedicated fixture uses the current NUMBER/RAW four-column slice.
    keep = [0, 1, 5, 6]
    catalog = docs['catalog']
    stored = catalog['tables'][0]
    source = docs['source_schema']['tables'][0]
    stored['columns'] = [stored['columns'][i] for i in keep]
    source['columns'] = [source['columns'][i] for i in keep]
    for key in ('mappings', 'attributes'):
        metadata[key] = [metadata[key][i] for i in keep]
    aggregate['columns'] = [aggregate['columns'][i] for i in keep]
    if raw_names:
        for c, raw, a in zip(stored['columns'], source['columns'], aggregate['columns']):
            # q_61 is intentionally still encoded: bare q_ names are ambiguous.
            if raw['name'] != 'q_61':
                c['logical_name'] = raw['name']
                a['name'] = raw['name']
    rehash(catalog)
    stored['canonical_digest'] = 'e' * 64
    fresh = metadata['freshness'][0]
    fresh.update(state='live', last_pos=4, source_committed_at=None, applied_at='2026-10-27T00:00:00Z')
    for mapping in metadata['mappings']:
        mapping.update(config_digest=catalog['config_digest'], run_generation=catalog['source_generation'])
    identity = {'engine': 'oracle', 'dbid': 'fixture'}
    dictionary = dict(protocol='oo.oracle.source-catalog-observation/v1', engine='oracle',
                      source_id=catalog['source_id'], source_identity=identity,
                      database={'observed_scn': '100', 'observer_principal': 'SYS'},
                      tables=[dict(owner=source['owner'], table=source['table'],
                                   primary_key=source['keys'], columns=[
                                       dict(c, data_type=c['type'], data_length=c['bytes'])
                                       for c in source['columns']])])
    def put(name, value):
        p = root / (name + '.json')
        p.write_text(lib.canonical(value))
        return target.pin(p)
    dictionary_pin = put('source-catalog', dictionary)
    schema = docs['source_schema']
    schema['metadata_observation'] = dict(observed_scn='100', dictionary=dictionary_pin)
    receipt = dict(protocol='oo.pipeline.native-publication-receipt/v1',
                   source_id=catalog['source_id'], run_id=catalog['run_id'],
                   source_generation=catalog['source_generation'], target_generation=catalog['target_generation'],
                   source_epoch=1, position_codec='oracle-capture-sequence/v1', position_lineage='oracle.fixture',
                   consistency='transactional', consistency_grade='T', cross_table_consistent=True, applied_pos='4',
                   oracle_buffer={'source_identity': identity},
                   tables=[dict(run_id=catalog['run_id'], physical_name=stored['stored_physical'],
                                schema=source['owner'], table=source['table'],
                                storage_schema=stored['logical_schema'], storage_table=stored['logical_table'],
                                canonical_digest=stored['canonical_digest'], accepted_rows=3, rejects=[])])
    publication = dict(protocol='oo.pipeline.native-c1-publication-result/v1',
                       publication={'outcome': 'published'}, receipt=receipt, receipt_sha256=oracle.digest(receipt))
    watermark = {k: receipt[k] for k in (
        'source_generation', 'target_generation', 'source_epoch', 'position_codec',
        'position_lineage', 'consistency', 'consistency_grade', 'cross_table_consistent', 'applied_pos')}
    watermark.update(source=catalog['source_id'], state='live', source_committed_at=None)
    pub = {k: receipt[k] for k in (
        'run_id', 'source_id', 'source_generation', 'target_generation', 'position_codec',
        'applied_pos', 'consistency', 'consistency_grade', 'cross_table_consistent')}
    pub.update(receipt_sha256=publication['receipt_sha256'], table_vector=receipt['tables'])
    run = {k: catalog[k] for k in oracle.IDENTITY}
    run.update(state='published', schema_digest='a' * 64)
    state = dict(database='owned_fixture', principal='reader', read_only='on',
                 metadata={'source_watermark': [watermark], 'full_import_run': [run],
                           'native_publication_receipt': [pub],
                           'source_instance': [dict(source=catalog['source_id'], engine='oracle',
                                                    measured_id='oracle.fixture', source_epoch=1)],
                           'table_quality': [dict(schema_name=stored['logical_schema'], table_name=stored['logical_table'],
                                                  rejects_open=0, partial_txn_in_progress=False,
                                                  position_state='ok', budget_state='ok', capture_method='c1_cdc',
                                                  delete_detection='native')]})
    config = dict(source_id=catalog['source_id'], target_database='owned_fixture',
                  catalog=put('catalog', catalog), source_schema=put('schema', schema),
                  source_catalog=dictionary_pin, publication=put('publication', publication))
    db = Mock(query=Mock(side_effect=[metadata, state, aggregate, copy.deepcopy(metadata), copy.deepcopy(state)]))
    return config, db, metadata, state, aggregate


class OracleTargetObservationTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def test_live_target_measurement_preserves_raw_seal_and_unknown_clock(self):
        config, db, metadata, _, _ = measured_fixture(self.root, raw_names=True)
        original = Path(config['catalog']['path']).read_bytes()
        result = target.measure(db, config, self.root / 'out')
        self.assertEqual(db.query.call_count, 5)
        self.assertEqual(Path(config['catalog']['path']).read_bytes(), original)
        measured = oracle.load(result)
        source = next(iter(measured['tables'].values()))
        oracle.validate_target(source, metadata['freshness'][0], metadata['mappings'], metadata['attributes'])
        self.assertEqual(source['columns']['Amount']['source_type'], 'NUMBER(38,9)')
        semantics = source['oracle']['source_semantics']
        self.assertIsNone(semantics['source_clock']['source_committed_at'])
        self.assertFalse(semantics['source_clock']['metadata_scn_is_row_clock'])
        self.assertEqual(semantics['source_metadata_observation']['observer_principal'], 'SYS')
        self.assertFalse(semantics['business_semantics_approved'])
        self.assertNotEqual(source['oracle']['binding']['schema_digest'],
                            semantics['target_observation']['metadata']['full_import_run'][0]['schema_digest'])
        self.assertTrue(any(c['sealed_column'] == 'ID' and c['logical_column'] == oracle.logical('ID')
                            for c in source['oracle']['binding']['column_mappings']))

    def test_drift_and_unpublished_state_refuse_without_facts(self):
        for field, value in [('state', 'awaiting_publish'), ('source_generation', 6), ('last_pos', 99)]:
            with self.subTest(field=field), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                cfg, db, meta, state, aggregate = measured_fixture(root)
                meta['freshness'][0][field] = value
                db.query.side_effect = [meta, state]
                with self.assertRaisesRegex(ValueError, 'oracle_current_generation_drift'):
                    target.measure(db, cfg, root / 'out')
                self.assertFalse((root / 'out/oracle-input.json').exists())

    def test_after_scan_drift_and_wrong_rows_are_not_success(self):
        for change in ('mapping', 'rows'):
            with self.subTest(change=change), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                cfg, db, meta, state, aggregate = measured_fixture(root)
                after = copy.deepcopy(meta)
                if change == 'mapping':
                    after['mappings'][0]['physical_name'] = 'other'
                else:
                    aggregate['rows'] = 4
                db.query.side_effect = [meta, state, aggregate, after, copy.deepcopy(state)]
                with self.assertRaisesRegex(ValueError, 'oracle_(target_observation_drift|target_row_drift)'):
                    target.measure(db, cfg, root / 'out')
                self.assertFalse((root / 'out/oracle-input.json').exists())

    def test_target_metadata_permissions_fail_closed(self):
        cfg, db, _, _, _ = measured_fixture(self.root)
        db.query.side_effect = RuntimeError('read_only_aggregate_query_failed:42501')
        with self.assertRaisesRegex(RuntimeError, '42501'):
            target.measure(db, cfg, self.root / 'out')

    def test_rehashed_semantic_projection_cannot_claim_a_source_clock_or_approval(self):
        cfg, db, _, _, _ = measured_fixture(self.root)
        result = target.measure(db, cfg, self.root / 'out')
        path = Path(result['semantics']['path'])
        baseline = json.loads(path.read_text())
        for change in ('clock', 'approval', 'grade'):
            document = copy.deepcopy(baseline)
            if change == 'clock':
                document['source_clock']['source_committed_at'] = '2026-10-27T00:00:00Z'
            elif change == 'approval':
                document['business_semantics_approved'] = True
            else:
                document['consistency_grade'] = 'A'
            document['measurement_sha256'] = oracle.digest(
                {k: v for k, v in document.items() if k != 'measurement_sha256'})
            path.write_text(lib.canonical(document))
            result['semantics'] = target.pin(path)
            with self.subTest(change=change), self.assertRaisesRegex(ValueError, 'binding'):
                oracle.load(result)

    def test_source_and_publication_cross_binding_rejects(self):
        cfg, db, _, _, _ = measured_fixture(self.root)
        path = Path(cfg['source_catalog']['path'])
        wrong = json.loads(path.read_text())
        wrong['source_id'] = 'oracle_frozen_iud_20261027'
        path.write_text(lib.canonical(wrong))
        cfg['source_catalog'] = target.pin(path)
        with self.assertRaisesRegex(ValueError, 'oracle_cross_source'):
            target.measure(db, cfg, self.root / 'out')
        db.query.assert_not_called()

    def test_default_unmeasured_does_not_become_absent(self):
        cfg, db, _, _, _ = measured_fixture(self.root)
        result = target.measure(db, cfg, self.root / 'out')
        # Exercise the generic adapter on the older metadata shape: explicit
        # unmeasured dominates legacy default_sql:null.
        from test_oracle_observation import fixture, pins
        docs, _, _ = fixture()
        for col in docs['source_schema']['tables'][0]['columns']:
            col['default_observation'] = {'state': 'unmeasured'}
        source = next(iter(oracle.load(pins(self.root, docs))['tables'].values()))
        self.assertTrue(all('default_sql' not in c and 'default_present' not in c
                            for c in source['columns'].values()))


if __name__ == '__main__':
    unittest.main()
