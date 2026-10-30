"""Synthetic C5 contracts only. Real published-target receipts are separate."""
import copy
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

import oracle_observation as oracle
import oracle_target_observation as target
from test_oracle_target_observation import measured_fixture

EXPORT = Path(__file__).resolve().parents[2] / 'source-onboarding/export_native_collection.py'
spec = importlib.util.spec_from_file_location('native_collection_export', EXPORT)
export = importlib.util.module_from_spec(spec)
spec.loader.exec_module(export)


def fixture(root):
    config, db, metadata, state, aggregate = measured_fixture(root, raw_names=True)
    docs = {k: oracle.read_pin(config[k]) for k in ('catalog', 'source_schema', 'source_catalog')}
    catalog, schema, dictionary = (docs[k] for k in ('catalog', 'source_schema', 'source_catalog'))
    stored, native = catalog['tables'][0], schema['tables'][0]
    native['keys'] = ['ID']
    dictionary['tables'][0]['primary_key'] = ['ID']
    for col in stored['columns']:
        col['key'] = col['logical_name'] == 'ID'
    identity = dict(engine='oracle', dbid='1', resetlogs_change='2', pdb_guid='ABC',
                    pdb_name='FREEPDB1', db_unique_name='FREE')
    dictionary['source_identity'] = identity
    schema['identity'] = '1|2|ABC|FREEPDB1|FREE|RO'
    snapshot = dict(identity=schema['identity'], scn='220', manifest_sha256='8' * 64,
                    source_schema=native['owner'], source_table=native['table'])
    scope = dict(protocol='oo.pipeline.native-c5-range/v1', source_engine='oracle',
        source_id=catalog['source_id'], source_generation=catalog['source_generation'],
        parent_generation=catalog['source_generation']-1, capture_method='c5_diff',
        delete_detection='reconcile', position_codec='snapshot-zero/v1', applied_pos='0',
        schema=native['owner'], table=native['table'],
        schema_fingerprint=state['metadata']['full_import_run'][0]['schema_digest'],
        arrow_sha256=catalog['input_digest'], row_count=1, chunk_id='0',
        chunk_topology_revision=1, prior_membership_revision=1, lo=None, hi=None,
        members=[[dict(kind='int', value=1)]], oracle_snapshot=snapshot)
    vector = dict(run_id=catalog['run_id'], physical_name=stored['stored_physical'],
        schema=stored['logical_schema'], table=stored['logical_table'], accepted_rows=3,
        canonical_digest=stored['canonical_digest'], rejects=[], source_generation=catalog['source_generation'],
        target_generation=catalog['target_generation'], applied_pos='0',
        capture_method='c5_diff', delete_detection='reconcile', consistency_grade='S0')
    receipt = dict(protocol='oo.pipeline.native-publication-receipt/v1', collection_mode='per_table',
        source_id=catalog['source_id'], run_id=catalog['run_id'], run_ids=[catalog['run_id']],
        source_generation=catalog['source_generation'], target_generation=catalog['target_generation'],
        position_codec='snapshot-zero/v1', position_lineage='oracle.snapshot', applied_pos='0',
        consistency='per_table', consistency_grade='S0', cross_table_consistent=False, tables=[vector])
    catalog['state'] = 'published'
    catalog['oracle_c5_binding'] = dict(protocol=oracle.C5_BINDING,
        **{k: catalog[k] for k in ('source_id', 'run_id', 'source_generation', 'target_generation')},
        captured_rows=1, effective_rows=3, scope=scope, publication_receipt_sha256=oracle.digest(receipt))
    watermark = state['metadata']['source_watermark']
    for key in ('source_epoch',):
        watermark[0].pop(key, None)
    watermark[0].update({k: receipt[k] for k in ('position_codec', 'position_lineage', 'applied_pos',
        'consistency', 'consistency_grade', 'cross_table_consistent')})
    state['metadata'].pop('source_instance')
    state['metadata']['native_publication_receipt'] = [dict(
        **{k: receipt[k] for k in ('run_id', 'source_id', 'source_generation', 'target_generation',
            'position_codec', 'applied_pos', 'consistency', 'consistency_grade', 'cross_table_consistent')},
        receipt_sha256=oracle.digest(receipt), table_vector=[vector])]
    state['metadata']['c5_oracle_binding'] = [dict(source=catalog['source_id'],
        schema_name=stored['logical_schema'], table_name=stored['logical_table'],
        source_schema=native['owner'], source_table=native['table'], identity=schema['identity'],
        snapshot_scn='220', manifest_sha256='8'*64, key_ident=oracle.logical('ID'))]
    state['metadata']['c5_block_receipt'] = [dict(source=catalog['source_id'], run_id=catalog['run_id'],
        source_generation=catalog['source_generation'], target_generation=catalog['target_generation'],
        chunk_id='0', scope=dict(scope, schema=stored['logical_schema'], table=stored['logical_table']))]
    state['metadata']['table_quality'][0].update(capture_method='c5_diff', delete_detection='reconcile')
    metadata['freshness'][0]['last_pos'] = 0
    def put(name, value):
        path = root / (name + '.json')
        path.write_text(json.dumps(value, sort_keys=True, ensure_ascii=False))
        return target.pin(path)
    config['catalog'] = put('c5-catalog', catalog)
    config['source_catalog'] = put('c5-native-dictionary', dictionary)
    schema['metadata_observation']['dictionary'] = config['source_catalog']
    config['source_schema'] = put('c5-native-inventory', schema)
    config['publication'] = put('c5-publication', receipt)
    db.query.side_effect = [metadata, state, aggregate, copy.deepcopy(metadata), copy.deepcopy(state)]
    return config, db, metadata, state, aggregate


class OracleC5ContractTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def test_published_c5_has_real_method_and_no_c1_identity_or_clock(self):
        config, db, _, _, _ = fixture(self.root)
        result = target.measure(db, config, self.root / 'out')
        source = next(iter(oracle.load(result)['tables'].values()))
        proof = source['oracle']['binding']['oracle_c5_binding']
        self.assertEqual((proof['captured_rows'], proof['effective_rows']), (1, 3))
        semantics = source['oracle']['source_semantics']
        self.assertEqual(semantics['capture_method'], 'c5_diff')
        self.assertNotIn('source_instance', semantics['target_observation']['metadata'])
        self.assertNotIn('oracle_buffer', semantics['publication_receipt'])
        self.assertIsNone(semantics['source_clock']['source_committed_at'])
        self.assertFalse(semantics['source_clock']['metadata_scn_is_row_clock'])
        self.assertEqual(source['oracle']['business_dictionary']['state'], 'missing')
        self.assertIn('_oo.c5_oracle_profile(', db.query.call_args_list[2].args[0])
        self.assertNotIn('_oo.source_instance', db.query.call_args_list[1].args[0])

    def test_legacy_published_or_c1_proof_is_not_a_c5_binding(self):
        config, _, _, _, _ = fixture(self.root)
        entry = oracle.read_pin(config['catalog'])
        for mutate in (
            lambda c: c.pop('oracle_c5_binding'),
            lambda c: c.update(engine='mysql'),
            lambda c: c['oracle_c5_binding']['scope'].update(capture_method='c1_cdc'),
            lambda c: c.update(state='awaiting_publish'),
        ):
            bad = copy.deepcopy(entry)
            mutate(bad)
            with self.assertRaises(ValueError):
                oracle.c5_binding(bad)
            with self.assertRaises(AssertionError):
                export.collection([bad], [(entry['tables'][0]['logical_schema'], entry['tables'][0]['logical_table'])])

    def test_rehashed_but_cross_bound_proof_is_refused(self):
        config, _, _, _, _ = fixture(self.root)
        baseline = oracle.read_pin(config['catalog'])
        for key, value in [('run_id', 'other'), ('source_id', 'other'), ('source_generation', 4),
                           ('target_generation', 6), ('captured_rows', 2), ('effective_rows', 1),
                           ('publication_receipt_sha256', 'bad')]:
            bad = copy.deepcopy(baseline)
            bad['oracle_c5_binding'][key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                oracle.c5_binding(bad)
            with self.subTest(export=key), self.assertRaises(AssertionError):
                export.collection([bad], [(baseline['tables'][0]['logical_schema'], baseline['tables'][0]['logical_table'])])

    def test_collection_preserves_the_proof_and_canonical_digest(self):
        config, _, _, _, _ = fixture(self.root)
        entry = oracle.read_pin(config['catalog'])
        collection = export.collection([entry], [(entry['tables'][0]['logical_schema'], entry['tables'][0]['logical_table'])])
        self.assertEqual(oracle.catalogs(collection), [entry])
        self.assertEqual(collection['entries'][0]['oracle_c5_binding'], entry['oracle_c5_binding'])
        self.assertNotEqual(entry['schema_digest'], entry['oracle_c5_binding']['scope']['schema_fingerprint'])

    def test_owner_mode_is_explicit_readonly_and_never_agent_acceptance(self):
        cfg, db, meta, state, aggregate = fixture(self.root)
        cfg['profile_access'] = 'internal_owner_readonly'
        state['principal'] = 'oo_target_owner'
        db.query.side_effect = [meta, state, aggregate, copy.deepcopy(meta), copy.deepcopy(state)]
        measured = target.measure(db, cfg, self.root / 'out')
        source = next(iter(oracle.load(measured)['tables'].values()))['oracle']
        self.assertEqual(source['binding']['profile_access'], 'internal_owner_readonly')
        self.assertFalse(source['source_semantics']['agent_read_accepted'])
        query = db.query.call_args_list[2].args[0]
        self.assertIn("current_user='oo_target_owner'", query)
        self.assertIn("current_setting('transaction_read_only')='on'", query)
        self.assertNotIn('_oo.c5_oracle_profile(', query)
        measured['profile_access'] = 'internal_aggregate_api'
        with self.assertRaisesRegex(ValueError, 'c5_profile_principal'):
            oracle.load(measured)

    def test_owner_mode_rejects_other_principal_or_write_session(self):
        for mutation in ('principal', 'read_only', 'schema_digest'):
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as directory:
                cfg, db, meta, state, _ = fixture(Path(directory))
                cfg['profile_access'] = 'internal_owner_readonly'
                state['principal'] = 'oo_target_owner'
                if mutation == 'schema_digest':
                    state['metadata']['full_import_run'][0]['schema_digest'] = 'b' * 64
                else:
                    state[mutation] = 'reader' if mutation == 'principal' else 'off'
                db.query.side_effect = [meta, state]
                with self.assertRaises(ValueError):
                    target.measure(db, cfg, Path(directory) / 'out')

    def test_current_scope_identity_scn_or_method_drift_is_refused(self):
        for mutation in ('identity', 'scn', 'members', 'method', 'clock'):
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                cfg, db, meta, state, aggregate = fixture(root)
                if mutation == 'identity':
                    state['metadata']['c5_oracle_binding'][0]['identity'] = 'other'
                elif mutation == 'scn':
                    state['metadata']['c5_oracle_binding'][0]['snapshot_scn'] = '221'
                elif mutation == 'members':
                    state['metadata']['c5_block_receipt'][0]['scope']['members'] = []
                elif mutation == 'method':
                    state['metadata']['table_quality'][0]['capture_method'] = 'c1_cdc'
                else:
                    state['metadata']['source_instance'] = []
                db.query.side_effect = [meta, state]
                with self.assertRaisesRegex(ValueError, 'oracle_'):
                    target.measure(db, cfg, root / 'out')
                self.assertFalse((root / 'out/oracle-input.json').exists())

    def test_relabelled_c5_semantics_is_rejected_after_rehash(self):
        cfg, db, _, _, _ = fixture(self.root)
        result = target.measure(db, cfg, self.root / 'out')
        path = Path(result['semantics']['path'])
        doc = json.loads(path.read_text())
        doc['capture_method'] = 'c1_cdc'
        doc['measurement_sha256'] = oracle.digest({k:v for k,v in doc.items() if k != 'measurement_sha256'})
        path.write_text(json.dumps(doc))
        result['semantics'] = target.pin(path)
        with self.assertRaisesRegex(ValueError, 'oracle_c5_semantics_projection'):
            oracle.load(result)

    def test_c1_profile_query_is_unchanged(self):
        self.assertEqual(oracle.profile_query({'source_id': 'old'}, 'select old_profile'), 'select old_profile')

    def test_protected_state_and_profile_bind_all_admitted_inputs(self):
        cfg, db, _, _, _ = fixture(self.root)
        entry = oracle.read_pin(cfg['catalog'])
        result = target.measure(db, cfg, self.root / 'out')
        binding = next(iter(oracle.load(result)['tables'].values()))['oracle']['binding']
        self.assertEqual(oracle.observation_binding(entry), oracle.observation_binding(binding))
        state_sql = db.query.call_args_list[1].args[0]
        profile_sql = db.query.call_args_list[2].args[0]
        self.assertTrue(state_sql.startswith('select _oo.c5_oracle_observation_state('))
        self.assertNotIn('from _oo.full_import_run', state_sql)
        for key in (*oracle.IDENTITY, 'oracle_c5_binding', 'stored_physical', 'stored_namespace',
                    'columns', 'accepted_rows'):
            self.assertIn('"' + key + '":', state_sql)
            self.assertIn('"' + key + '":', profile_sql)
        self.assertNotIn('owner_token', state_sql)
        self.assertNotIn('select forbidden', oracle.profile_query(binding, 'select forbidden'))

    def test_observation_binding_rejects_missing_identity_or_table(self):
        cfg, _, _, _, _ = fixture(self.root)
        entry = oracle.read_pin(cfg['catalog'])
        for key in oracle.IDENTITY:
            bad = copy.deepcopy(entry)
            del bad[key]
            with self.subTest(key=key), self.assertRaisesRegex(ValueError, 'c5_observation_binding'):
                oracle.observation_binding(bad)
        for tables in ([], entry['tables'] * 2):
            with self.assertRaisesRegex(ValueError, 'c5_observation_table'):
                oracle.observation_binding(dict(entry, tables=tables))

    def test_aggregate_mode_rejects_owner_evidence(self):
        for principal in ('oo_target_owner', 'postgres', None):
            with self.subTest(principal=principal), tempfile.TemporaryDirectory() as directory:
                cfg, db, metadata, state, _ = fixture(Path(directory))
                state['principal'] = principal
                db.query.side_effect = [metadata, state]
                with self.assertRaisesRegex(ValueError, 'c5_profile_principal'):
                    target.measure(db, cfg, Path(directory) / 'out')

    def test_profile_preserves_mapped_aliases_without_arbitrary_renaming(self):
        cfg, db, _, _, _ = fixture(self.root)
        measured = target.measure(db, cfg, self.root / 'out')
        binding = next(iter(oracle.load(measured)['tables'].values()))['oracle']['binding']
        columns = [{'name': c['logical_column'], 'physical_name': c['physical_name']}
                   for c in binding['column_mappings']]
        query = oracle.profile_query(binding, 'select forbidden_direct_view', columns)
        self.assertIn('_oo.c5_oracle_profile', query)
        self.assertIn(oracle.logical('ID'), query)
        columns[0]['name'] = 'invented'
        with self.assertRaisesRegex(ValueError, 'c5_profile_columns'):
            oracle.profile_query(binding, 'select forbidden', columns)


if __name__ == '__main__':
    unittest.main()
