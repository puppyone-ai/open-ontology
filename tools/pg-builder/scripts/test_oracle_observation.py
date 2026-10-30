"""Synthetic contract peers and opt-in isolated PostgreSQL; no Oracle connection."""
import copy
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch

import library_observation as obs
import library_selfbuild as lib
import oracle_observation as oracle


def fixture():
    specs = [
        ('ID', 'NUMBER', 9, 0, 20, -1, 'bigint'),
        ('Amount', 'NUMBER', 38, 9, 1700, (38 << 16) + 9 + 4, 'numeric(38,9)'),
        ('OCCURRED', 'DATE', None, None, 1114, -1, 'timestamp without time zone'),
        ('TS', 'TIMESTAMP(6)', None, 6, 1114, -1, 'timestamp without time zone'),
        ('TZ', 'TIMESTAMP(6) WITH TIME ZONE', None, 6, 1184, -1, 'timestamp with time zone'),
        ('PAYLOAD', 'RAW', None, None, 17, -1, 'bytea'),
        ('q_61', 'VARCHAR2', None, None, 25, -1, 'text'),
    ]
    source_id = 'oracle_test'
    schema, table = oracle.logical('Owner'), oracle.logical('MixedTable')
    dataset = schema + '.' + table
    source = dict(owner='Owner', table='MixedTable', keys=['q_61', 'ID'], issues=[],
                  comment='Source table comment', object=['1|2026-10-27 00:00:00'], columns=[])
    columns, mappings, attrs = [], [], []
    for i, (name, kind, p, s, oid, typmod, pg_type) in enumerate(specs):
        logical, physical = oracle.logical(name), f'pc_{i}'
        source['columns'].append(dict(
            name=name, type=kind, precision=p, scale=s, nullable='N' if name in source['keys'] else 'Y',
            bytes=80, position=i + 1, comment='Original ' + name, default_sql=None))
        columns.append(dict(logical_name=logical, stored_physical=physical, oid=oid, typmod=typmod,
                            source_type='arrow_type_' + str(i), caveat=None, key=name in source['keys']))
        mappings.append(dict(source=source_id, logical_namespace=schema, logical_table=table,
                             logical_column=logical, physical_name=physical, scope_namespace_physical='pn_test',
                             scope_table_physical='pt_test', level='c'))
        attrs.append(dict(nspname='_oo', relname='pt_test', attname=physical,
                          data_type=pg_type, oid=oid, typmod=typmod))
    catalog = dict(protocol=oracle.CATALOG, source_id=source_id, engine='oracle', capture='sealed_import',
                   state='awaiting_publish', shadows_queried=False, source_generation=3, target_generation=5,
                   run_id='11111111-1111-4111-8111-111111111111',
                   config_digest='1' * 64, type_digest='2' * 64, input_digest='3' * 64,
                   quality=dict(loss_concealed=False, stored_rejected_rows=0, manifest_reject_count=0),
                   tables=[dict(logical_schema=schema, logical_table=table, columns=columns,
                                stored_namespace='pn_test', stored_physical='pt_test', accepted_rows=3,
                                rejects=[])])
    rehash(catalog)
    profile = dict(protocol='oo.native.profile/v1', sampled=False,
                   **{k: catalog[k] for k in oracle.IDENTITY},
                   tables=[dict(dataset=dataset, rows=3, full_scan=True,
                                columns=[dict(name=oracle.logical(c['name']), source_type=c['type'])
                                         for c in source['columns']])])
    semantics = dict(protocol='oo.native.semantics/v1', engine='oracle',
                     **{k: catalog[k] for k in oracle.IDENTITY},
                     temporal_policy={'timezone': 'utc_instant'},
                     consistency_grade=None, delete_detection=None)
    schema_doc = dict(protocol='oo.oracle.inventory/v1', source_id=source_id, tables=[source])
    metadata = dict(freshness=[dict(schema_name=schema, table_name=table, source=source_id,
                                   source_generation=3, target_generation=5, physical_name='pt_test')],
                    mappings=mappings, attributes=attrs)
    observed = dict(rows=3, snapshot='1:2:', observed_at='2026-10-27 00:00:00+00',
                    columns=[dict(name=c['logical_name'], non_null=2, distinct=2, min_length=1,
                                  max_length=50, blank_rows=0, digits_rows=0, leading_zero_rows=0)
                             for c in columns])
    return dict(catalog=catalog, source_schema=schema_doc, profile=profile, semantics=semantics), metadata, observed


def rehash(catalog):
    text = ''
    for table in catalog['tables']:
        text += table['logical_schema'] + '.' + table['logical_table'] + '\n'
        for c in table['columns']:
            text += f"{c['logical_name']}\t{c['oid']}\t{c['typmod']}\t{c.get('caveat') or ''}\n"
    catalog['schema_digest'] = hashlib.sha256(text.encode()).hexdigest()


def pins(root, docs):
    config = dict(source_id='oracle_test')
    for key, doc in docs.items():
        path = Path(root) / (key + '.json')
        path.write_text(lib.canonical(doc))
        config[key] = dict(path=str(path), sha256=hashlib.sha256(path.read_bytes()).hexdigest())
    return config


class OracleObservationTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.docs, self.metadata, self.result = fixture()

    def collect(self, after=None, config_update=None):
        config = pins(self.root, self.docs)
        config.update(config_update or {})
        db = Mock(query=Mock(side_effect=[self.metadata, self.result, after or self.metadata]))
        tables = lib.collect_tables(db, oracle_input=config, report_path=self.root / 'report.json')
        return tables, config, db

    def test_temporal_proof_does_not_turn_c1_into_c5(self):
        columns = self.docs['catalog']['tables'][0]['columns']
        zoned = next(c for c in columns if c['oid'] == 1184)
        columns.remove(zoned)
        columns.append(zoned)
        rehash(self.docs['catalog'])
        for key in ('profile', 'semantics'):
            self.docs[key]['schema_digest'] = self.docs['catalog']['schema_digest']
        self.docs['semantics']['measurement_protocol'] = 'oo.builder.oracle-target-observation/v1'
        proof = {'columns': {'Owner.MixedTable.TZ': {
            'source_type': 'TIMESTAMP(6) WITH TIME ZONE', 'submicroseconds_zero': True}}}
        with patch('oracle_target_observation.validate_evidence', return_value=proof):
            source = next(iter(oracle.load(pins(self.root, self.docs))['tables'].values()))
        binding = source['oracle']['binding']
        self.assertNotIn('oracle_c5_binding', binding)
        self.assertEqual(oracle.profile_query(binding, 'select ordinary_profile'), 'select ordinary_profile')

    def test_complete_native_columns_and_source_keys_preserved(self):
        tables, config, db = self.collect()
        table = tables[0]
        self.assertEqual(table['source_engine'], 'oracle')
        self.assertEqual(table['primary_key'], [oracle.logical('q_61'), oracle.logical('ID')])
        columns = {c['source_name']: c for c in table['columns']}
        self.assertEqual(columns['Amount']['source_type'], 'NUMBER(38,9)')
        self.assertEqual(columns['PAYLOAD']['data_type'], 'bytea')
        self.assertEqual(columns['OCCURRED']['source_type'], 'DATE')
        self.assertEqual(columns['TZ']['target_contract'], 'utc_instant_original_zone_not_preserved')
        self.assertEqual(columns['q_61']['comment'], 'Original q_61')
        self.assertEqual(table['oracle']['pins']['source_schema_sha256'], config['source_schema']['sha256'])
        self.assertEqual(table['oracle']['pins']['adapter_sha256'],
                         hashlib.sha256(Path(oracle.__file__).read_bytes()).hexdigest())
        self.assertEqual(table['oracle']['business_dictionary']['state'], 'missing')
        self.assertIsNone(table['oracle']['source_semantics']['consistency_grade'])
        self.assertEqual(db.query.call_count, 3)
        lib.validate_retained_tables(Mock(query=Mock(return_value=self.metadata)), tables, oracle_input=config)
        facts = lib.build_facts(tables, lambda *a: {})
        lib.require_complete_facts(facts, tables)
        column = next(c for c in facts['tables'][0]['columns'] if c['source_name'] == 'OCCURRED')
        self.assertEqual(column['classification_candidate']['semantic_type'], 'event_time')
        column = next(c for c in facts['tables'][0]['columns'] if c['source_name'] == 'Amount')
        self.assertEqual(column['classification_candidate']['semantic_type'], 'money')
        self.assertTrue(json.loads((self.root / 'report.json').read_text())['full_source_inventory_observed'])

    def test_excluded_inventory_is_explicit_not_partial_success_claim(self):
        lob = dict(owner='Owner', table='LOB', keys=[], issues=['no_validated_primary_key'],
                   columns=[dict(name='BODY', type='CLOB', nullable='Y', precision=None, scale=None)])
        self.docs['source_schema']['tables'].append(lob)
        self.collect()
        report = json.loads((self.root / 'report.json').read_text())
        self.assertFalse(report['full_source_inventory_observed'])
        entry = next(i for i in report['source_inventory'] if i['source_table'] == 'LOB')
        self.assertEqual(entry['disposition'], 'not_in_sealed_catalog')
        self.assertEqual(entry['columns'][0]['disposition'], 'unsupported_source_type')
        self.assertEqual(entry['key_state'], 'no_validated_primary_key')

    def test_comments_defaults_absent_are_unmeasured_not_invented(self):
        for col in self.docs['source_schema']['tables'][0]['columns']:
            col.pop('comment')
            col.pop('default_sql')
        table = self.collect()[0][0]
        self.assertTrue(all(c['comment_state'] == 'unmeasured' and 'default_sql' not in c
                            for c in table['columns']))

    def test_current_generation_mapping_and_type_drift_refuse(self):
        for key, changed in [('generation', 'generation'), ('mapping', 'mapping'), ('type', 'type')]:
            with self.subTest(key=key), tempfile.TemporaryDirectory() as root:
                docs, meta, result = fixture()
                if changed == 'generation':
                    meta['freshness'][0]['target_generation'] += 1
                elif changed == 'mapping':
                    meta['mappings'][0]['physical_name'] = 'wrong'
                else:
                    meta['attributes'][0]['typmod'] = 9
                db = Mock(query=Mock(return_value=meta))
                with self.assertRaisesRegex(ValueError, 'oracle_current_.*_drift'):
                    lib.collect_tables(db, oracle_input=pins(root, docs))
                self.assertEqual(db.query.call_count, 1)

    def test_post_observation_generation_drift_and_row_drift(self):
        after = copy.deepcopy(self.metadata)
        after['freshness'][0]['source_generation'] += 1
        with self.assertRaisesRegex(ValueError, 'observation_binding_drift'):
            self.collect(after=after)
        (self.root / 'report.json').unlink()
        self.result['rows'] = 4
        with self.assertRaisesRegex(ValueError, 'oracle_target_row_drift'):
            self.collect()

    def test_pins_and_cross_source_inputs_refuse_before_pg(self):
        for name, key, value in [
            ('catalog', 'engine', 'mysql'), ('catalog', 'source_id', 'other'),
            ('source_schema', 'source_id', 'other'), ('profile', 'source_id', 'other'),
            ('semantics', 'engine', 'mysql'), ('semantics', 'target_generation', 6)]:
            with self.subTest(name=name, key=key):
                docs, _, _ = fixture()
                docs[name][key] = value
                db = Mock()
                with self.assertRaisesRegex(ValueError, 'oracle_'):
                    lib.collect_tables(db, oracle_input=pins(self.root, docs))
                db.query.assert_not_called()
        config = pins(self.root, self.docs)
        Path(config['source_schema']['path']).write_text('{}')
        with self.assertRaisesRegex(ValueError, 'oracle_input_digest'):
            oracle.load(config)

    def test_missing_dictionary_required_and_wrong_source_dictionary(self):
        config = pins(self.root, self.docs)
        with self.assertRaisesRegex(ValueError, 'oracle_business_dictionary_missing'):
            oracle.load(dict(config, require_business_dictionary=True))
        file = self.root / 'dictionary.txt'
        file.write_text('Synthetic dictionary fixture; NOT a customer dictionary')
        pin = dict(path=str(file), sha256=hashlib.sha256(file.read_bytes()).hexdigest(),
                   source_id='mysql_other', source_schema_sha256=config['source_schema']['sha256'])
        with self.assertRaisesRegex(ValueError, 'oracle_dictionary_source_binding'):
            oracle.load(dict(config, business_dictionary=pin))
        pin['source_id'] = 'oracle_test'
        observed = oracle.load(dict(config, business_dictionary=pin))
        self.assertEqual(observed['business_dictionary']['state'], 'pinned_unreviewed')
        self.assertFalse(observed['business_dictionary']['business_semantics_approved'])

    def test_no_pk_nanos_lob_and_unconstrained_number_fail_with_inventory(self):
        for change, code in [('pk', 'source_inventory_issue'), ('nanos', 'submicrosecond'),
                             ('lob', 'unsupported_source_type'), ('number', 'number_encoding')]:
            with self.subTest(change=change), tempfile.TemporaryDirectory() as root:
                docs, meta, result = fixture()
                source = docs['source_schema']['tables'][0]
                if change == 'pk':
                    source['keys'] = []
                else:
                    col = source['columns'][1]
                    col['type'] = {'nanos': 'TIMESTAMP(9)', 'lob': 'CLOB', 'number': 'NUMBER'}[change]
                    col['precision'] = None
                    docs['profile']['tables'][0]['columns'][1]['source_type'] = col['type']
                report = Path(root) / 'report.json'
                db = Mock()
                with self.assertRaisesRegex(ValueError, code):
                    lib.collect_tables(db, oracle_input=pins(root, docs), report_path=report)
                db.query.assert_not_called()
                self.assertEqual(json.loads(report.read_text())['state'], 'incomplete')
                self.assertEqual(len(json.loads(report.read_text())['source_inventory'][0]['columns']), 7)

    def test_no_temporal_numeric_or_raw_downcast(self):
        for index, oid, typmod, code in [(1, 1700, (10 << 16) + 6, 'number_target_precision'),
                                         (2, 1082, -1, 'temporal_target_type'),
                                         (3, 1114, 3, 'temporal_target_precision'),
                                         (5, 25, -1, 'raw_target_type')]:
            with self.subTest(index=index):
                docs, _, _ = fixture()
                docs['catalog']['tables'][0]['columns'][index].update(oid=oid, typmod=typmod)
                with self.assertRaisesRegex(ValueError, code):
                    oracle.load(pins(self.root, docs))
        self.docs['semantics'].pop('temporal_policy')
        with self.assertRaisesRegex(ValueError, 'timezone_policy_required'):
            oracle.load(pins(self.root, self.docs))

    def test_column_budget_and_pg_permission_fail_closed(self):
        with self.assertRaisesRegex(ValueError, 'column_observation_budget'):
            oracle.load(dict(pins(self.root, self.docs), max_columns=2))
        for sqlstate in ('42501', '57014'):
            db = Mock(query=Mock(side_effect=[self.metadata,
                      RuntimeError('read_only_aggregate_query_failed:' + sqlstate)]))
            report = self.root / (sqlstate + '.json')
            with self.assertRaisesRegex(RuntimeError, sqlstate):
                lib.collect_tables(db, oracle_input=pins(self.root, self.docs), report_path=report)
            audit = json.loads(report.read_text())
            self.assertEqual(audit['tables'][0]['state'], 'failed')
            self.assertEqual(audit['failures'], ['read_only_aggregate_query_failed:' + sqlstate])

    def test_retained_oracle_evidence_and_classification_cannot_be_substituted(self):
        tables, config, _ = self.collect()
        stale = copy.deepcopy(tables)
        stale[0]['oracle']['business_dictionary']['state'] = 'approved'
        with self.assertRaisesRegex(ValueError, 'column_observation_binding'):
            lib.require_complete_columns(stale)
        facts = lib.build_facts(tables, lambda *a: {})
        facts['tables'][0]['columns'][2]['classification_candidate']['semantic_type'] = 'date'
        facts['facts_sha256'] = lib.digest({k: v for k, v in facts.items() if k != 'facts_sha256'})
        with self.assertRaisesRegex(ValueError, 'retained_classification_rule_drift'):
            lib.require_complete_facts(facts)

    def test_collection_vector_and_profile_binding(self):
        catalog = self.docs['catalog']
        table = catalog['tables'][0]
        vector = [dict(dataset=table['logical_schema'] + '.' + table['logical_table'],
                       **{k: catalog[k] for k in oracle.IDENTITY},
                       stored_namespace=table['stored_namespace'], stored_physical=table['stored_physical'])]
        collection = dict(protocol=oracle.COLLECTION, entries=[catalog], binding_vector=vector)
        collection['collection_sha256'] = oracle.digest(collection)
        self.docs['catalog'] = collection
        for key in ('profile', 'semantics'):
            self.docs[key].update(collection_sha256=hashlib.sha256(lib.canonical(collection).encode()).hexdigest(),
                                  binding_vector_sha256=oracle.digest(vector))
        oracle.load(pins(self.root, self.docs))
        self.docs['semantics']['binding_vector_sha256'] = 'f' * 64
        with self.assertRaisesRegex(ValueError, 'profile_semantics_binding'):
            oracle.load(pins(self.root, self.docs))

    def test_identifier_contract_matches_connector_reserved_and_nfc_rules(self):
        ident = Path(__file__).resolve().parents[3] / 'crates/oo-connectors/src/ident.rs'
        reserved = ident.read_text().split('pub const PG17_RESERVED:')[1].split('];')[0]
        self.assertEqual(oracle.RESERVED, set(re.findall(r'"([a-z_]+)"', reserved)))
        for name in ['select', 'a' * 64, 'q_61', 'Mixed', 'e\u0301']:
            self.assertTrue(oracle.logical(name).startswith('q_'))
        self.assertEqual(oracle.logical('e\u0301'), oracle.logical('\u00e9'))
        source = dict(columns={'e\u0301': {'source_type': 'VARCHAR2'}}, primary_key=['e\u0301'])
        bound = obs.bind_ddl_columns(source, [dict(logical_column=oracle.logical('e\u0301'))])
        self.assertEqual(next(iter(bound['columns'].values()))['source_name'], 'e\u0301')

    def test_native_full_profile_reuses_source_bound_without_arrow_relabel(self):
        catalog = self.docs['catalog']
        raw = self.docs['source_schema']['tables'][0]['columns']
        bound = dict(protocol='oo.source-semantics.source-bound/v1', engine='oracle',
                     **{k: catalog[k] for k in oracle.IDENTITY}, scn='123', published_scn='123',
                     registry_snapshot='snapshot-123', banner='Oracle 23.26.3.0.0',
                     dictionary={'protocol': 'oo.source-semantics.type-dictionary/v1', 'columns': [
                         '|'.join([c['name'], c['type'],
                                   '-' if c['precision'] is None else str(c['precision']),
                                   '-' if c['scale'] is None else str(c['scale']), '0']) for c in raw]},
                     bindings=[dict(sealed_column=oracle.logical(c['name']), dictionary_column=c['name'])
                               for c in raw])
        self.docs['source_bound'] = bound
        self.docs['profile']['protocol'] = 'oo.oracle.native-full-profile/v1'
        for column, c, stored in zip(self.docs['profile']['tables'][0]['columns'], raw, catalog['tables'][0]['columns']):
            column.update(source_name=c['name'], source_type=stored['source_type'])
        self.docs['semantics'].update(protocol='oo.oracle.native-c5-source-semantics/v1', executed=True,
                                      evidence={'source-bound': {'sha256': hashlib.sha256(lib.canonical(bound).encode()).hexdigest()}})
        for doc in (self.docs['profile'], self.docs['semantics']):
            for key in oracle.IDENTITY[3:]:
                doc.pop(key)
        parsed = oracle.load(pins(self.root, self.docs))
        table = next(iter(parsed['tables'].values()))
        self.assertEqual(table['columns']['Amount']['source_type'], 'NUMBER(38,9)')
        self.assertEqual(table['columns']['Amount']['captured_type'], 'arrow_type_1')
        self.docs['semantics']['executed'] = False
        with self.assertRaisesRegex(ValueError, 'source_semantics_unmeasured'):
            oracle.load(pins(self.root, self.docs))
        self.docs['semantics']['executed'] = True
        self.docs['source_bound']['dictionary']['columns'].pop()
        self.docs['semantics']['evidence']['source-bound']['sha256'] = hashlib.sha256(
            lib.canonical(self.docs['source_bound']).encode()).hexdigest()
        with self.assertRaisesRegex(ValueError, 'type_dictionary_binding'):
            oracle.load(pins(self.root, self.docs))

    def test_header_only_catalog_and_missing_columns_are_not_a_seal(self):
        for change in ('missing_tables', 'missing_column'):
            docs = copy.deepcopy(self.docs)
            if change == 'missing_tables':
                docs['catalog'].pop('tables')
            else:
                docs['catalog']['tables'][0]['columns'].pop()
            db = Mock()
            with self.assertRaisesRegex(ValueError, 'oracle_catalog_(table_missing|column_coverage)'):
                lib.collect_tables(db, oracle_input=pins(self.root, docs))
            db.query.assert_not_called()

    def test_source_and_dictionary_drift_change_semantic_inputs_without_model(self):
        tables, config, _ = self.collect()
        before = lib.draft_batches(lib.build_facts(tables, lambda *a: {}))[0]['semantic_input_digest']
        self.docs['source_schema']['tables'][0]['columns'][1]['comment'] = 'Corrected source comment'
        config = pins(self.root, self.docs)
        with self.assertRaisesRegex(ValueError, 'oracle_retained_source_drift'):
            lib.validate_retained_tables(Mock(query=Mock(return_value=self.metadata)), tables, oracle_input=config)
        db = Mock(query=Mock(side_effect=[self.metadata, self.result, self.metadata]))
        new = lib.collect_tables(db, oracle_input=config)
        after = lib.draft_batches(lib.build_facts(new, lambda *a: {}))[0]['semantic_input_digest']
        self.assertNotEqual(before, after)


@unittest.skipUnless(os.environ.get('OO_TEST_PG_BIN'), 'requires owned socket-only PostgreSQL')
class OraclePostgresTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory(prefix='oo-oracle-semantics-', dir='/tmp')
        cls.addClassCleanup(cls.tmp.cleanup)
        cls.root = Path(cls.tmp.name)
        cls.bin = Path(os.environ['OO_TEST_PG_BIN'])
        cls.data, cls.sock = cls.root / 'data', cls.root / 'sock'
        cls.sock.mkdir()
        subprocess.run([str(cls.bin / 'initdb'), '-D', str(cls.data), '-A', 'trust', '--no-locale'],
                       capture_output=True, check=True)
        subprocess.run([str(cls.bin / 'pg_ctl'), '-D', str(cls.data), '-l', str(cls.root / 'pg.log'),
                        '-o', f"-k {cls.sock} -c listen_addresses='' -c unix_socket_permissions=0700",
                        '-w', 'start'], capture_output=True, check=True)
        cls.addClassCleanup(lambda: subprocess.run(
            [str(cls.bin / 'pg_ctl'), '-D', str(cls.data), '-m', 'fast', '-w', 'stop'],
            capture_output=True, check=True))
        cls.command = [str(cls.bin / 'psql'), '-h', str(cls.sock), '-d', 'postgres']
        cls.db = lib.ReadOnlyPostgres(cls.command, 5000)
        cls.docs, cls.meta, _ = fixture()
        sql = """CREATE SCHEMA _oo;
        CREATE TABLE _oo.table_freshness (schema_name text, table_name text, source text,
          source_generation int, target_generation int, physical_name text);
        CREATE TABLE _oo.physical_mappings (source text, logical_namespace text, logical_table text,
          logical_column text, physical_name text, scope_namespace_physical text, scope_table_physical text, level text);
        CREATE TABLE _oo.pt_test (pc_0 bigint, pc_1 numeric(38,9), pc_2 timestamp, pc_3 timestamp,
                                 pc_4 timestamptz, pc_5 bytea, pc_6 text);
        INSERT INTO _oo.pt_test VALUES
        (1,12345678901234567890123456789.123456789,'2026-10-27 12:34:56','2026-10-27 12:34:56.123456',
         '2026-10-27 12:34:56.123456+05:45',decode('0000ff','hex'),'001'),
        (2,-0.000000001,'1960-01-01 01:02:03',NULL,'1960-01-01 01:02:03-00:30',decode('00','hex'),'2'),
        (3,NULL,NULL,NULL,NULL,NULL,'3');
        CREATE ROLE oracle_observer LOGIN;
        GRANT USAGE ON SCHEMA _oo TO oracle_observer;
        GRANT SELECT ON _oo.table_freshness, _oo.physical_mappings TO oracle_observer;"""
        for table, values in [('table_freshness', cls.meta['freshness']), ('physical_mappings', cls.meta['mappings'])]:
            for row in values:
                sql += 'INSERT INTO _oo.' + table + '(' + ','.join(row) + ') VALUES (' + ','.join(
                    lib.literal(v) for v in row.values()) + ');'
        cls.sql(sql)
        cls.config = pins(cls.root, cls.docs)

    @classmethod
    def sql(cls, text):
        return subprocess.run(cls.command + ['-X', '-v', 'ON_ERROR_STOP=1'], input=text,
                              text=True, capture_output=True, check=True)

    def test_actual_target_all_columns_and_facts_only_cli(self):
        output = self.root / 'facts-only'
        config = dict(psql_command=self.command, oracle_observation_input=self.config,
                      statement_timeout_ms=5000, builder_binary='/must-not-run', require_complete_drafts=True)
        path = self.root / 'config.json'
        path.write_text(lib.canonical(config))
        command = [sys.executable, str(Path(lib.__file__)), '--config', str(path),
                   '--output', str(output), '--stop-after-facts']
        run = subprocess.run(command, capture_output=True, text=True, timeout=60)
        self.assertEqual(run.returncode, 0, run.stderr)
        facts = json.loads((output / 'facts.json').read_text())
        lib.require_complete_facts(facts)
        cols = {c['source_name']: c for c in facts['tables'][0]['columns']}
        self.assertEqual(cols['Amount']['profile']['distinct'], 2)
        self.assertEqual(cols['OCCURRED']['profile']['max_length'], 19)
        self.assertEqual(cols['PAYLOAD']['profile']['max_length'], 8)
        self.assertEqual(cols['q_61']['profile']['leading_zero_rows'], 1)
        report = json.loads((output / 'report.json').read_text())
        self.assertEqual(report['model_calls'], 0)
        self.assertEqual(report['s5'], 'not_run')
        self.assertFalse((output / 'drafts.json').exists())
        self.assertFalse((output / 'pack.json').exists())
        proof = self.db.query("""select json_build_object(
          'number_exact',(select pc_1::text from _oo.pt_test where pc_0=1)='12345678901234567890123456789.123456789',
          'date_keeps_time',(select pc_2::time from _oo.pt_test where pc_0=1)='12:34:56'::time,
          'utc_instant',(select pc_4 from _oo.pt_test where pc_0=1)='2026-10-27 06:49:56.123456+00'::timestamptz,
          'raw_exact',(select encode(pc_5,'hex') from _oo.pt_test where pc_0=1)='0000ff')""")
        self.assertTrue(all(proof.values()))
        compiled = None
        if os.environ.get('OO_OOS_TREE_BIN'):
            import oos_tree
            generated, _, _ = lib.effective_documents(facts, [])
            effective, visibility, review = lib.effective_documents(facts, [], fixture_review=True)
            binding = oos_tree.write_source_tree(output / 'ontology', generated, effective)
            for name, value in [('binding', binding), ('visibility', visibility),
                                ('ceilings', visibility['limits']), ('review', review)]:
                lib.write(output / (name + '.json'), value)
            command = [os.environ['OO_OOS_TREE_BIN'], str(output / 'ontology'), str(output / 'binding.json'),
                       str(output / 'visibility.json'), str(output / 'ceilings.json'), str(output / 'compiled.json')]
            run = subprocess.run(command, capture_output=True, text=True, timeout=60)
            self.assertEqual(run.returncode, 0, run.stderr)
            compiled = json.loads((output / 'compiled.json').read_text())
            self.assertEqual(compiled['effective_oos'], effective)
            self.assertFalse(compiled['published'])
            self.assertEqual(len(effective['datasets'][0]['columns']), 7)
            self.assertTrue(any(c['data_type'] == 'bytea' for c in effective['datasets'][0]['columns']))
            lib.write(output / 'yaml-command.json', dict(
                argv=command, exit_code=run.returncode,
                binary_sha256=hashlib.sha256(Path(command[0]).read_bytes()).hexdigest(),
                review='fixture/test', s5='not_run', model_calls=0, published=False))
        evidence = os.environ.get('OO_ORACLE_OBSERVATION_EVIDENCE')
        if evidence:
            dest = Path(evidence)
            dest.mkdir(parents=True, exist_ok=False)
            for name in ('s0-s2.json', 'facts.json', 'report.json', 'inventory-audit.json', 'observation-report.json'):
                (dest / name).write_bytes((output / name).read_bytes())
            if compiled is not None:
                for name in ('binding.json', 'visibility.json', 'ceilings.json', 'review.json',
                             'compiled.json', 'yaml-command.json'):
                    (dest / name).write_bytes((output / name).read_bytes())
                import shutil
                shutil.copytree(output / 'ontology', dest / 'ontology')
            (dest / 'input').mkdir()
            for key, doc in self.docs.items():
                lib.write(dest / 'input' / (key + '.json'), doc)
            (dest / 'target-proof.json').write_text(lib.canonical(dict(
                scope='synthetic_oracle_contract_real_owned_postgres', source_read=False, checks=proof,
                socket=str(self.sock), tcp_listen=False, model_calls=0)))

    def test_actual_permissions_and_readonly_transactions(self):
        observer = lib.ReadOnlyPostgres(self.command + ['-U', 'oracle_observer'], 5000)
        report = self.root / 'permission-report.json'
        with self.assertRaisesRegex(RuntimeError, '42501'):
            lib.collect_tables(observer, oracle_input=self.config, report_path=report)
        self.assertEqual(json.loads(report.read_text())['state'], 'incomplete')
        with self.assertRaisesRegex(RuntimeError, '25006'):
            self.db.query("select 1; insert into _oo.pt_test(pc_0) values(999)")
        self.assertEqual(self.db.query('select count(*) from _oo.pt_test'), 3)

    def test_actual_statement_budget(self):
        db = lib.ReadOnlyPostgres(self.command, 10)
        with self.assertRaisesRegex(RuntimeError, '57014'):
            db.query('select pg_sleep(1)')


if __name__ == '__main__':
    unittest.main()
