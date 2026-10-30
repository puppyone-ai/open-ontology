"""Read-only actual 0096 checks. Provision separately; never install, replay or capture here."""
import copy
import json
import os
from pathlib import Path
import subprocess
import unittest

import library_observation as observation
import library_selfbuild as lib
import oracle_observation as oracle


@unittest.skipUnless(os.environ.get('OO_ORACLE_OBSERVER_TEST_CONFIG'),
                     'requires an explicitly admitted Oracle C5 observer')
class OracleObserverPostgresTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.config = json.loads(Path(os.environ['OO_ORACLE_OBSERVER_TEST_CONFIG']).read_text())
        cls.entry = oracle.read_pin(cls.config['catalog'])
        cls.binding = oracle.observation_binding(cls.entry)
        cls.columns = [{'name': c['logical_name'], 'physical_name': c['stored_physical']}
                       for c in cls.binding['columns']]
        cls.db = lib.ReadOnlyPostgres(cls.config['psql_command'], 30000)

    def sql(self, binding=None, columns=None):
        args = [self.binding if binding is None else binding,
                self.columns if columns is None else columns]
        return 'select _oo.c5_oracle_profile(' + ','.join(
            observation.literal(json.dumps(a)) + '::jsonb' for a in args) + ')'

    def test_actual_profile_matches_pinned_effective_profile(self):
        measured = self.db.query(self.sql())
        expected = oracle.read_pin(self.config['gold_profile'])['tables'][0]
        observation.profiles(measured, [c['name'] for c in self.columns])
        self.assertEqual(measured['rows'], expected['rows'])
        self.assertEqual(measured['rows'], self.binding['accepted_rows'])
        self.assertNotEqual(measured['rows'], self.binding['oracle_c5_binding']['captured_rows'])
        for column in measured['columns']:
            gold = next(c for c in expected['columns'] if c['name'] == column['name'])
            self.assertEqual(column, {k: gold[k] for k in column})

    def test_alias_profiles_preserve_same_counts(self):
        aliases = [dict(c, name=oracle.logical(c['name'])) for c in self.columns]
        measured = self.db.query(self.sql(columns=aliases))
        self.assertEqual(measured['rows'], self.binding['accepted_rows'])
        self.assertEqual([c['name'] for c in measured['columns']], [c['name'] for c in aliases])

    def test_current_state_is_protected_and_contains_no_owner_token(self):
        state = self.db.query(oracle.observation_state_query(self.entry))
        self.assertEqual(state['principal'], self.config['observer_role'])
        self.assertEqual(state['database'], self.config['target_database'])
        self.assertEqual(state['read_only'], 'on')
        self.assertNotIn('owner_token', json.dumps(state))
        self.assertNotIn('source_instance', state['metadata'])
        run = state['metadata']['full_import_run'][0]
        self.assertEqual(run['run_id'], self.entry['run_id'])
        self.assertEqual(run['source_generation'], self.entry['source_generation'])

    def test_wrong_or_stale_bindings_and_nonadmitted_table_fail(self):
        changes = {
            'source_id': 'oracle_not_admitted', 'source_generation': 4, 'target_generation': 4,
            'run_id': '00000000-0000-0000-0000-000000000000', 'config_digest': '0' * 64,
            'schema_digest': '0' * 64, 'type_digest': '0' * 64, 'input_digest': '0' * 64,
            'stored_physical': 'full_import_run', 'stored_namespace': 'other',
            'accepted_rows': self.binding['accepted_rows'] + 1,
        }
        for key, value in changes.items():
            with self.subTest(key=key), self.assertRaisesRegex(RuntimeError, '42501'):
                self.db.query(self.sql(dict(self.binding, **{key: value})))
        for key, value in [('source_schema', 'OTHER'), ('identity', 'OTHER'), ('scn', '1')]:
            bad = copy.deepcopy(self.binding)
            bad['oracle_c5_binding']['scope']['oracle_snapshot'][key] = value
            with self.subTest(snapshot=key), self.assertRaisesRegex(RuntimeError, '42501'):
                self.db.query(self.sql(bad))
        bad = copy.deepcopy(self.binding)
        bad['oracle_c5_binding']['publication_receipt_sha256'] = '0' * 64
        with self.assertRaisesRegex(RuntimeError, '42501'):
            self.db.query(self.sql(bad))
        bad = copy.deepcopy(self.binding)
        bad['oracle_c5_binding']['scope']['members'] = []
        with self.assertRaisesRegex(RuntimeError, '42501'):
            self.db.query(self.sql(bad))

    def test_partial_duplicate_unknown_and_injected_columns_fail(self):
        for columns in ([], self.columns[:-1], [self.columns[0]] * len(self.columns),
                        [dict(c, name='invented') for c in self.columns],
                        [dict(c, physical_name='x"; select 1; --') for c in self.columns]):
            with self.subTest(columns=columns), self.assertRaisesRegex(RuntimeError, '22023'):
                self.db.query(self.sql(columns=columns))

    def test_no_raw_control_or_owner_access(self):
        role = self.db.query("select json_build_object('current',current_user,'session',session_user,"
                             "'memberships',(select count(*) from pg_auth_members "
                             "where member=(select oid from pg_roles where rolname=current_user)))")
        self.assertEqual(role, dict(current=self.config['observer_role'],
                                    session=self.config['observer_role'], memberships=0))
        tables = [self.binding['stored_physical'], 'full_import_run', 'c5_oracle_observer_scope',
                  'c5_block_receipt', 'c5_member', 'c5_overlay', 'native_library_head',
                  *self.config['raw_relations']]
        for table in tables:
            with self.subTest(table=table), self.assertRaisesRegex(RuntimeError, '42501'):
                self.db.query('select count(*) from _oo.' + observation.quote(table))
        with self.assertRaisesRegex(RuntimeError, '42501'):
            self.db.query("select _oo.c5_oracle_observation_check('{}'::jsonb)")

    def test_readwrite_and_read_committed_sessions_fail(self):
        for begin in ('BEGIN', 'BEGIN READ ONLY'):
            sql = begin + '; ' + self.sql() + '; ROLLBACK;'
            result = subprocess.run(self.config['psql_command'] +
                ['-X', '-w', '-qAt', '-v', 'ON_ERROR_STOP=1', '-v', 'VERBOSITY=sqlstate'],
                input=sql, text=True, capture_output=True, timeout=45)
            self.assertEqual(result.returncode, 3, result.stderr)
            self.assertIn('42501', result.stderr)


if __name__ == '__main__':
    unittest.main()
