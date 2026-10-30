import copy
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import Mock, patch

import library_control as control
import library_observation as obs
import library_selfbuild as lib


DDL = """CREATE TABLE `items` (
  `id` bigint unsigned NOT NULL,
  `bank_account` varchar(32) DEFAULT NULL COMMENT '收款账户',
  PRIMARY KEY (`id`)
);"""


def metadata():
    return dict(
        freshness=[dict(schema_name='app', table_name='items', source='fixture',
                        source_generation=1, target_generation=2, physical_name='items')],
        mappings=[dict(source='fixture', logical_namespace='app', logical_table='items',
                       logical_column=name, physical_name=name, scope_namespace_physical='_oo',
                       scope_table_physical='items', level='c') for name in ['id', 'bank_account']],
        attributes=[dict(nspname='_oo', relname='items', attname=name, data_type=typ)
                    for name, typ in [('id', 'bigint'), ('bank_account', 'text')]])


def result():
    return dict(rows=3, snapshot='1:2:', observed_at='2026-10-21 00:00:00+00',
                columns=[dict(name=name, non_null=2, distinct=2, min_length=1,
                              max_length=3, blank_rows=0, digits_rows=2,
                              leading_zero_rows=1) for name in ['id', 'bank_account']])


class ObservationTest(unittest.TestCase):
    def test_observation_checkpoints_bind_metadata_and_reject_corruption(self):
        with tempfile.TemporaryDirectory() as root:
            root = Path(root)
            (root / 'items.sql').write_text(DDL)
            cache = root / 'cache'
            db = Mock(query=Mock(side_effect=[metadata(), result(), metadata()]))
            first = lib.collect_tables(db, root, checkpoint_dir=cache)
            db = Mock(query=Mock(side_effect=[metadata(), metadata()]))
            self.assertEqual(first, lib.collect_tables(db, root, checkpoint_dir=cache))
            self.assertEqual(db.query.call_count, 2)
            file = next(cache.glob('*.json'))
            value = json.loads(file.read_text())
            value['table']['columns'][0]['profile']['rows'] = 999
            file.write_text(json.dumps(value))
            with self.assertRaisesRegex(ValueError, 'observation_checkpoint_binding'):
                lib.collect_tables(Mock(query=Mock(return_value=metadata())), root, checkpoint_dir=cache)

    def test_old_physical_generation_does_not_duplicate_current_columns(self):
        meta = metadata()
        meta['mappings'].extend([dict(m, scope_table_physical='retired_items')
                                 for m in meta['mappings']])
        active = lib.active_column_mappings(meta, meta['freshness'][0])
        self.assertEqual(len(active), 2)
        self.assertTrue(all(m['scope_table_physical'] == 'items' for m in active))
        meta['mappings'].append(copy.deepcopy(active[0]))
        self.assertEqual(len(lib.active_column_mappings(meta, meta['freshness'][0])), 3,
                         'duplicates in the active generation must still fail later validation')

    def test_observation_queries_use_bounded_read_only_sessions(self):
        with patch.object(lib.subprocess, 'run', return_value=Mock(returncode=0, stdout='{}')) as run:
            lib.ReadOnlyPostgres(['psql'], 5000).query('select 1')
        statement = run.call_args.kwargs['input']
        self.assertIn('REPEATABLE READ READ ONLY', statement)
        self.assertIn("work_mem='4MB'", statement)
        self.assertIn('max_parallel_workers_per_gather=0', statement)
        self.assertIn('statement_timeout = 5000', statement)
        self.assertTrue(statement.endswith('ROLLBACK;'))

    def test_encoded_source_names_bind_without_dropping_columns(self):
        source = dict(columns={'refEnum': {'comment': 'dictionary code'},
                               'q_61': {}, 'id': {}}, primary_key=['refEnum'])
        names = ['q_726566456e756d', 'q_715f3631', 'id']
        bound = obs.bind_ddl_columns(source, [{'logical_column': n} for n in names])
        self.assertEqual(set(bound['columns']), set(names))
        self.assertEqual(bound['columns'][names[0]]['source_name'], 'refEnum')
        self.assertEqual(bound['columns'][names[1]]['source_name'], 'q_61')
        self.assertEqual(bound['primary_key'], [names[0]])
        self.assertNotIn('source_name', source['columns']['refEnum'])
        self.assertNotIn('source_name', bound['columns']['id'])
        for bad in [names[:-1], names + ['id'], ['q_726566456E756D'],
                    ['q_ff'], ['refenum'], ['q_61']]:
            with self.subTest(bad=bad), self.assertRaisesRegex(ValueError, 'column_mapping_'):
                obs.bind_ddl_columns(source, [{'logical_column': n} for n in bad])

    def test_encoded_column_collect_retained_validation_and_classification(self):
        encoded = 'q_' + b'bankAccount'.hex()
        ddl = DDL.replace('bank_account', 'bankAccount')
        meta, observed = metadata(), result()
        meta['mappings'][1]['logical_column'] = encoded
        for mapping in meta['mappings']:
            mapping['scope_namespace_physical'] = 'pn_fixture_identity'
        observed['columns'][1]['name'] = encoded
        with tempfile.TemporaryDirectory() as root:
            Path(root, 'items.sql').write_text(ddl)
            db = Mock(query=Mock(side_effect=[meta, observed, meta]))
            tables = lib.collect_tables(db, root)
            column = next(c for c in tables[0]['columns'] if c['name'] == encoded)
            self.assertEqual(column['source_name'], 'bankAccount')
            self.assertEqual(tables[0]['physical'], '"_oo"."items"')
            self.assertEqual(tables[0]['mapping_namespace'], 'pn_fixture_identity')
            lib.validate_retained_tables(Mock(query=Mock(return_value=meta)), tables, root)
            drift = copy.deepcopy(meta)
            for mapping in drift['mappings']:
                mapping['scope_namespace_physical'] = 'pn_other_identity'
            with self.assertRaisesRegex(ValueError, 'retained_s0_mappings'):
                lib.validate_retained_tables(Mock(query=Mock(return_value=drift)), tables, root)
            facts = lib.build_facts(tables, lambda *args: {})
            lib.require_complete_facts(facts, tables)
            column = next(c for c in facts['tables'][0]['columns'] if c['name'] == encoded)
            self.assertEqual(column['classification_candidate']['pii'], 'sensitive')
            compact = lib.draft_batches(facts)[0]['tables'][0]['columns']
            self.assertEqual(next(c for c in compact if c['name'] == encoded)['source_name'],
                             'bankAccount')

    def collect(self, meta=None, observed=None, after=None, expected=None):
        meta = metadata() if meta is None else meta
        observed = result() if observed is None else observed
        db = Mock()
        db.query.side_effect = [meta, observed, meta if after is None else after]
        with tempfile.TemporaryDirectory() as root:
            Path(root, 'items.sql').write_text(DDL.replace(');', ") COMMENT='Item ledger';"))
            return lib.collect_tables(db, root, expected=expected)

    def test_semantic_rules_do_not_confuse_accounts_and_amounts(self):
        for name in ['bank_account', 'yj_payee_account', 'bankAccount']:
            candidate = lib.semantic(name, 'bigint', '')
            self.assertEqual((candidate['semantic_type'], candidate['pii']), ('identifier', 'sensitive'))
        for name, comment in [('amount_freight_pt', '结算方式'),
                              ('amount_cod_service', '代收时效'), ('service_level', '服务等级')]:
            self.assertEqual(lib.semantic(name, 'int', comment)['semantic_type'], 'category')
        self.assertEqual(lib.semantic('item_name', 'text', '货号')['semantic_type'], 'identifier')
        self.assertEqual(lib.semantic('discount', 'text', '')['semantic_type'], 'free_text')
        self.assertEqual(lib.semantic('amount', 'text', '')['evidence'], 'name_type_conflict')
        self.assertFalse(lib.numeric_type('print'))
        self.assertFalse(lib.compatible('print', 'integer'))
        self.assertEqual(lib.semantic('price', 'decimal(10,2)', '')['semantic_type'], 'money')

    def test_accounts_stay_hidden_under_auto_accept_and_fixture_review(self):
        facts = lib.build_facts(self.collect(), lambda *a: {})
        for options in [dict(auto_accept=True), dict(fixture_review=True)]:
            _, visibility, _ = lib.effective_documents(facts, [], **options)
            self.assertTrue(all('bank_account' not in t['columns'] for t in visibility['datasets']))

    def test_corrected_semantic_hint_is_not_automatic_business_approval(self):
        tables = self.collect()
        column = tables[0]['columns'][1]
        column.update(name='amount_freight_pt', data_type='integer', source_type='int', comment='结算方式')
        facts = lib.build_facts(tables, lambda *a: {})
        effective, visibility, _ = lib.effective_documents(facts, [], auto_accept=True)
        corrected = next(c for c in effective['datasets'][0]['columns'] if c['name'] == 'amount_freight_pt')
        self.assertEqual(corrected['semantic_type'], 'category')
        self.assertEqual(corrected['classification']['state'], 'pending')
        self.assertTrue(all('amount_freight_pt' not in t['columns'] for t in visibility['datasets']))

    def test_ddl_preserves_source_details_and_case_sensitive_literals(self):
        ddl = r"""-- CREATE TABLE `decoy` (comment only)
        CREATE TABLE `app`.`items` (
        `id` bigint(20) UNSIGNED NOT NULL AUTO_INCREMENT,
        `kind` enum('A','b,c') DEFAULT 'A' COMMENT 'it''s \'quoted\'',
        `price` decimal(10,2) DEFAULT -1.25,
        `updated` timestamp NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
        PRIMARY KEY (`id`), UNIQUE KEY `uq_kind` (`kind`)) ENGINE=InnoDB;"""
        with tempfile.TemporaryDirectory() as root:
            Path(root, 'table.sql').write_text(ddl)
            parsed = obs.ddl_metadata(root)['items']
        self.assertEqual(parsed['columns']['kind']['source_type'], "enum('A','b,c')")
        self.assertEqual(parsed['columns']['id']['source_type'], 'bigint(20) unsigned')
        self.assertFalse(parsed['columns']['id']['nullable'])
        self.assertEqual(parsed['columns']['price']['default_sql'], '-1.25')
        self.assertEqual(parsed['columns']['updated']['on_update_sql'], 'CURRENT_TIMESTAMP(6)')
        self.assertEqual(len(parsed['constraints_sql']), 2)
        self.assertEqual(parsed['schema'], 'app')

    def test_ddl_refuses_unparsed_duplicate_or_truncated_definitions(self):
        bad = ['CREATE TABLE', 'CREATE TABLE `a`', 'CREATE TABLE `a` (oops)',
               'CREATE TABLE `a` (`id` int, `id` int)',
               'CREATE TABLE `a` (`id` int GENERATED ALWAYS AS (1))',
               'CREATE TABLE `a` (`id` int, PRIMARY)',
               "CREATE TABLE `a` (`id` int) /*! comment */;"]
        for ddl in bad:
            with self.subTest(ddl=ddl), tempfile.TemporaryDirectory() as root:
                Path(root, 'a.sql').write_text(ddl)
                with self.assertRaises(ValueError):
                    obs.ddl_metadata(root)

    def test_collection_observes_every_column_and_preserves_ddl(self):
        tables = self.collect()
        self.assertEqual(tables[0]['table_comment'], 'Item ledger')
        self.assertEqual(obs.require_complete_columns(tables), tables)
        account = next(c for c in tables[0]['columns'] if c['name'] == 'bank_account')
        self.assertEqual(account['profile']['leading_zero_rows'], 1)
        self.assertTrue(account['default_present'])
        self.assertEqual(account['default_sql'], 'NULL')
        self.assertEqual(account['profile']['null_count'], 1)

    def test_table_comments_are_not_column_comments(self):
        for tail, expected in [
                ('', ''), ("COMMENT 'ledger'", 'ledger'),
                (r"ENGINE=InnoDB COMMENT='it''s \'quoted\''", "it's 'quoted'")]:
            with self.subTest(tail=tail), tempfile.TemporaryDirectory() as root:
                Path(root, 'items.sql').write_text(DDL.replace(');', ') ' + tail + ';'))
                self.assertEqual(obs.ddl_metadata(root)['items']['table_comment'], expected)
        for tail in ["COMMENT", "COMMENT=", "COMMENT='a' COMMENT='b'", "COMMENT=123"]:
            with self.subTest(tail=tail), tempfile.TemporaryDirectory() as root:
                Path(root, 'items.sql').write_text(DDL.replace(');', ') ' + tail + ';'))
                with self.assertRaises(ValueError):
                    obs.ddl_metadata(root)

    def test_source_label_reaches_draft_and_generated_documents(self):
        facts = lib.build_facts(self.collect(), lambda *a: {})
        self.assertEqual(lib.draft_batches(facts)[0]['tables'][0]['table_comment'], 'Item ledger')
        effective, _, _ = lib.effective_documents(facts, [])
        self.assertEqual(effective['datasets'][0]['label'], 'Item ledger')
        for comment in ['', '\nunsafe', 'x' * 257, None]:
            self.assertEqual(lib.declared_table_label(dict(dataset='app.items', table_comment=comment)),
                             'app.items')

    def test_no_silent_missing_or_duplicate_bindings(self):
        variants = []
        for key in ['freshness', 'mappings', 'attributes']:
            m = metadata()
            m[key].pop()
            variants.append(m)
        m = metadata()
        m['mappings'].append(copy.deepcopy(m['mappings'][0]))
        variants.append(m)
        m = metadata()
        m['mappings'][1]['physical_name'] = 'id'
        variants.append(m)
        m = metadata()
        m['mappings'][1]['scope_namespace_physical'] = 'other'
        variants.append(m)
        for m in variants:
            with self.subTest(metadata=m), self.assertRaises(ValueError):
                self.collect(meta=m, expected=['app.items'])

    def test_selected_inventory_ignores_explicitly_excluded_tables(self):
        m = metadata()
        m['freshness'].append(dict(m['freshness'][0], table_name='excluded'))
        self.assertEqual(len(self.collect(meta=m, expected=['app.items'])), 1)
        with self.assertRaisesRegex(ValueError, 'ddl_table_coverage'):
            self.collect(meta=m)

    def test_generation_drift_and_partial_results_are_not_success(self):
        after = metadata()
        after['freshness'][0]['target_generation'] += 1
        with self.assertRaisesRegex(ValueError, 'observation_binding_drift'):
            self.collect(after=after)
        partial = result()
        partial['columns'].pop()
        with self.assertRaisesRegex(ValueError, 'observation_column_coverage'):
            self.collect(observed=partial)

    def test_empty_all_null_and_invalid_counts(self):
        raw = dict(name='x', non_null=0, distinct=0, min_length=None, max_length=None,
                   blank_rows=0, digits_rows=0, leading_zero_rows=0)
        for rows, state in [(0, 'empty'), (4, 'all_null')]:
            p = obs.profiles(dict(rows=rows, columns=[raw]), ['x'])['x']
            self.assertEqual(p['state'], state)
            self.assertEqual(p['null_count'], rows)
        for bad in [dict(raw, distinct=1), dict(raw, non_null=-1), dict(raw, min_length=0)]:
            with self.assertRaises(ValueError):
                obs.profiles(dict(rows=4, columns=[bad]), ['x'])

    def test_failure_report_is_private_explicit_and_has_no_raw_errors(self):
        db = Mock()
        db.query.side_effect = [metadata(), RuntimeError('secret row value')]
        with tempfile.TemporaryDirectory() as root:
            Path(root, 'items.sql').write_text(DDL)
            report = Path(root, 'report.json')
            with self.assertRaises(RuntimeError):
                lib.collect_tables(db, root, report_path=report)
            evidence = json.loads(report.read_text())
            self.assertEqual(evidence['state'], 'incomplete')
            self.assertEqual(evidence['tables'][0]['state'], 'failed')
            self.assertEqual(len(evidence['tables'][0]['columns']), 2)
            self.assertNotIn('secret', report.read_text())
            self.assertEqual(report.stat().st_mode & 0o777, 0o600)

    def test_transport_errors_do_not_leak_commands_or_values(self):
        errors = [subprocess.TimeoutExpired(['secret'], 1, output='secret'), OSError('secret')]
        for error in errors:
            with patch.object(subprocess, 'run', side_effect=error):
                with self.assertRaisesRegex(RuntimeError, '^read_only_aggregate_transport_failed$'):
                    lib.ReadOnlyPostgres(['secret']).query('select 1')
        with patch.object(subprocess, 'run', return_value=Mock(returncode=0, stdout='secret')):
            with self.assertRaisesRegex(RuntimeError, '^read_only_aggregate_result_invalid$'):
                lib.ReadOnlyPostgres(['secret']).query('select 1')

    def test_retained_missing_evidence_cannot_reach_model_or_compiler(self):
        facts = lib.build_facts(self.collect(), lambda *a: {})
        del facts['tables'][0]['observation']
        facts['facts_sha256'] = lib.digest({k: v for k, v in facts.items() if k != 'facts_sha256'})
        with tempfile.TemporaryDirectory() as root, patch.object(lib, 'call_model') as model:
            for fn in [lambda: lib.run_drafts(facts, {}, root),
                       lambda: control.drafts(facts, {}, Path(root)),
                       lambda: lib.compile_pack(facts, [], {}, root)]:
                with self.assertRaisesRegex(ValueError, 'column_observation_incomplete'):
                    fn()
            model.assert_not_called()

    def test_retained_facts_must_match_s0_and_own_hash(self):
        tables = self.collect()
        facts = lib.build_facts(tables, lambda *a: {})
        lib.require_complete_facts(facts, tables)
        changed = copy.deepcopy(tables)
        changed[0]['columns'][0]['comment'] = 'different'
        with self.assertRaisesRegex(ValueError, 'retained_facts_observation_binding'):
            lib.require_complete_facts(facts, changed)
        facts['facts_sha256'] = 'wrong'
        with self.assertRaisesRegex(ValueError, 'facts_digest_mismatch'):
            lib.require_complete_facts(facts)

    def test_retained_classification_cannot_resurrect_a_visible_bank_account(self):
        facts = lib.build_facts(self.collect(), lambda *a: {})
        column = next(c for c in facts['tables'][0]['columns'] if c['name'] == 'bank_account')
        column['classification_candidate'] = {
            'semantic_type': 'quantity', 'pii': 'none',
            'confidence': '0.70', 'evidence': 'name_comment_heuristic'}
        facts['facts_sha256'] = lib.digest({k: v for k, v in facts.items() if k != 'facts_sha256'})
        with tempfile.TemporaryDirectory() as root, patch.object(lib, 'call_model') as model:
            for fn in [lambda: lib.require_complete_facts(facts),
                       lambda: lib.run_drafts(facts, {}, root),
                       lambda: control.drafts(facts, {}, Path(root)),
                       lambda: lib.compile_pack(facts, [], {}, root)]:
                with self.assertRaisesRegex(ValueError, 'retained_classification_rule_drift'):
                    fn()
            model.assert_not_called()

    def test_retained_s0_rechecks_live_generation_ddl_and_physical_columns(self):
        tables = self.collect()
        with tempfile.TemporaryDirectory() as root:
            Path(root, 'items.sql').write_text(DDL.replace(');', ") COMMENT='Item ledger';"))
            lib.validate_retained_tables(Mock(query=Mock(return_value=metadata())), tables, root)
            stale = copy.deepcopy(tables)
            stale[0].pop('table_comment')
            with self.assertRaisesRegex(ValueError, 'retained_s0_'):
                lib.validate_retained_tables(Mock(query=Mock(return_value=metadata())), stale, root)
            for kind in ['generation', 'mapping', 'type']:
                current = metadata()
                if kind == 'generation':
                    current['freshness'][0]['target_generation'] += 1
                elif kind == 'mapping':
                    current['mappings'][0]['physical_name'] = 'moved'
                else:
                    current['attributes'][0]['data_type'] = 'text'
                with self.subTest(kind=kind), self.assertRaisesRegex(ValueError, 'retained_s0_'):
                    lib.validate_retained_tables(Mock(query=Mock(return_value=current)), tables, root)
            Path(root, 'items.sql').write_text(DDL.replace('varchar(32)', 'varchar(64)'))
            with self.assertRaisesRegex(ValueError, 'retained_s0_ddl'):
                lib.validate_retained_tables(Mock(query=Mock(return_value=metadata())), tables, root)

    def test_observation_binding_and_counts_cannot_be_silently_changed(self):
        for kind in ['generation', 'counts', 'missing_column', 'mapping']:
            tables = self.collect()
            if kind == 'generation':
                tables[0]['target_generation'] += 1
            elif kind == 'counts':
                tables[0]['columns'][0]['profile']['observed_rows'] = 2
            elif kind == 'missing_column':
                tables[0]['columns'].pop()
            else:
                tables[0]['columns'][1]['physical_name'] = tables[0]['columns'][0]['physical_name']
            with self.subTest(kind=kind), self.assertRaises(ValueError):
                obs.require_complete_columns(tables)

    def test_control_stages_reject_incomplete_evidence_without_receipts(self):
        tables = self.collect()
        facts = lib.build_facts(tables, lambda *a: {})
        del facts['tables'][0]['observation']
        manifest = dict(protocol='oo.library.manifest/v1',
                        inventory=[dict(schema='app', table='items')], exclusions=[],
                        sources=[dict(schema='app', source_id='fixture', tables=['items'])])
        for stage in ['S0', 'S1', 'S2', 'S3', 'S4', 'S5', 'S6', 'S7', 'S8', 'compile']:
            with self.subTest(stage=stage), tempfile.TemporaryDirectory() as root, patch.object(lib, 'call_model') as model:
                root = Path(root)
                lib.write(root / 'manifest.json', manifest)
                lib.write(root / 's0-s2.json', tables)
                lib.write(root / 'facts.json', facts)
                config = dict(psql_command=[], ddl_directory=str(root))
                if stage == 'S0':
                    lib.write(root / 'retained.json', facts['tables'])
                    config.update(s0_input=str(root / 'retained.json'),
                                  s0_sha256=lib.hashlib.sha256((root / 'retained.json').read_bytes()).hexdigest())
                if stage == 'S1':
                    config.update(facts_input=str(root / 'facts.json'),
                                  facts_sha256=lib.hashlib.sha256((root / 'facts.json').read_bytes()).hexdigest())
                lib.write(root / 'config.json', config)
                request = dict(stage=stage, work_dir=str(root), config=str(root / 'config.json'),
                               manifest=str(root / 'manifest.json'), datasets=['app.items'], max_tables=1)
                with self.assertRaisesRegex(ValueError, 'column_observation_incomplete'):
                    control.run(request)
                self.assertFalse((root / (stage + '-receipt.json')).exists())
                model.assert_not_called()

    def test_structured_relationship_error_is_not_an_observed_zero(self):
        for observed in [dict(error='failed'), {}, dict(non_null=1, matched=2, parent_unique=True),
                         dict(non_null=1, matched=1, parent_unique=True, non_null_rows=2)]:
            with self.subTest(observed=observed), self.assertRaisesRegex(ValueError, 'relationship_observation_invalid'):
                lib.validate_containment(observed)

    def test_failed_relations_or_enums_block_the_completion_gate(self):
        facts = lib.build_facts(self.collect(), lambda *a: {})
        for key, item, reason in [
            ('relationship_candidates', dict(evidence=dict(observation_error='failed')), 'relationship'),
            ('enum_candidates', dict(observation_error='failed'), 'enum')]:
            broken = copy.deepcopy(facts)
            broken[key].append(item)
            with self.assertRaisesRegex(ValueError, reason + '_observation_incomplete'):
                lib.require_complete_relationship_observations(broken)

    def test_richer_evidence_does_not_expand_model_context(self):
        facts = lib.build_facts(self.collect(), lambda *a: {})
        before = lib.draft_batches(facts)
        facts['tables'][0]['constraints_sql'] = ['x' * 300000]
        facts['tables'][0]['columns'][0]['definition_sql'] = 'x' * 300000
        after = lib.draft_batches(facts)
        self.assertEqual(after[0]['tables'], before[0]['tables'])
        self.assertEqual(len(lib.canonical(after)), len(lib.canonical(before)))
        self.assertNotEqual(after[0]['semantic_input_digest'], before[0]['semantic_input_digest'])
        self.assertNotIn('leading_zero_rows', lib.canonical(before))


@unittest.skipUnless(os.environ.get('OO_TEST_PG_BIN'), 'set OO_TEST_PG_BIN for isolated PostgreSQL integration')
class PostgresObservationTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.root = tempfile.TemporaryDirectory(prefix='oo-observe-', dir='/tmp')
        cls.bin = Path(os.environ['OO_TEST_PG_BIN'])
        cls.data = Path(cls.root.name, 'db')
        cls.socket = Path(cls.root.name, 'sock')
        cls.socket.mkdir()
        cls.addClassCleanup(cls.root.cleanup)
        subprocess.run([str(cls.bin / 'initdb'), '-D', str(cls.data), '-A', 'trust', '--no-locale'],
                       check=True, capture_output=True)
        subprocess.run([str(cls.bin / 'pg_ctl'), '-D', str(cls.data), '-l', str(Path(cls.root.name, 'pg.log')),
                        '-o', f"-k {cls.socket} -c listen_addresses='' -c unix_socket_permissions=0700",
                        '-w', 'start'], check=True, capture_output=True)
        cls.addClassCleanup(lambda: subprocess.run(
            [str(cls.bin / 'pg_ctl'), '-D', str(cls.data), '-m', 'fast', '-w', 'stop'],
            check=True, capture_output=True))
        cls.command = [str(cls.bin / 'psql'), '-h', str(cls.socket), '-d', 'postgres']
        cls.db = lib.ReadOnlyPostgres(cls.command)

    def sql(self, sql):
        subprocess.run(self.command + ['-X', '-v', 'ON_ERROR_STOP=1'], input=sql,
                       text=True, check=True, capture_output=True)

    def test_actual_full_scan_empty_null_shape_and_wide_columns(self):
        self.sql("""CREATE TABLE shapes (id bigint, account text, missing text);
                    INSERT INTO shapes VALUES (1,'001',NULL),(2,'12',NULL),(3,' ',NULL),(4,NULL,NULL);""")
        columns = [dict(name=c, physical_name=c) for c in ['id', 'account', 'missing']]
        raw = self.db.query(obs.profile_sql('"shapes"', columns))
        p = obs.profiles(raw, [c['name'] for c in columns])
        self.assertEqual(p['account']['leading_zero_rows'], 1)
        self.assertEqual(p['account']['digits_rows'], 2)
        self.assertEqual(p['account']['blank_rows'], 1)
        self.assertEqual(p['account']['null_count'], 1)
        self.assertEqual(p['missing']['state'], 'all_null')
        self.assertTrue(raw['snapshot'])
        self.sql('CREATE TABLE wide (' + ','.join(f'c{i} text' for i in range(520)) + ');')
        wide = [dict(name=f'c{i}', physical_name=f'c{i}') for i in range(520)]
        empty = obs.profiles(self.db.query(obs.profile_sql('"wide"', wide)), [c['name'] for c in wide])
        self.assertEqual(len(empty), 520)
        self.assertTrue(all(c['state'] == 'empty' for c in empty.values()))
        self.sql("INSERT INTO wide (c0,c519) VALUES ('001','last');")
        filled = obs.profiles(self.db.query(obs.profile_sql('"wide"', wide)), [c['name'] for c in wide])
        self.assertEqual(filled['c0']['leading_zero_rows'], 1)
        self.assertEqual(filled['c519']['distinct'], 1)
        self.assertEqual(filled['c260']['state'], 'all_null')

    def test_actual_collector_reads_catalog_and_bindings(self):
        m = metadata()
        self.sql("""CREATE SCHEMA _oo;
          CREATE TABLE _oo.table_freshness (schema_name text, table_name text, source text,
            source_generation int, target_generation int, physical_name text);
          CREATE TABLE _oo.physical_mappings (source text, logical_namespace text, logical_table text,
            logical_column text, physical_name text, scope_namespace_physical text, scope_table_physical text, level text);
          CREATE TABLE _oo.items (id bigint, bank_account text);
          INSERT INTO _oo.items VALUES (1,'001'),(2,'02'),(3,NULL);""")
        for table, values in [('table_freshness', m['freshness']), ('physical_mappings', m['mappings'])]:
            for row in values:
                self.sql('INSERT INTO _oo.' + table + ' (' + ','.join(row) + ') VALUES (' +
                         ','.join(lib.literal(v) for v in row.values()) + ');')
        with tempfile.TemporaryDirectory() as root:
            Path(root, 'items.sql').write_text(DDL)
            tables = lib.collect_tables(self.db, root)
        self.assertEqual(len(tables), 1)
        lib.require_complete_facts(lib.build_facts(tables, lib.database_containment(self.db)), tables)


if __name__ == '__main__':
    unittest.main()
