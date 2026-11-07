import json
from pathlib import Path
import unittest

from audit import classify, logminer_dml, trace_records
from recursive_sql import LOGMINER_RECURSIVE_BLOCKS


def statement(cursor, sql, depth, start, end, uid=0):
    return (f'PARSING IN CURSOR #{cursor} len={len(sql)} dep={depth} uid={uid} oct=7\n'
            f'{sql}\nEND OF STMT\nEXEC #{cursor}:e={end-start},tim={end}\n')


class D11aTests(unittest.TestCase):
    def test_recursive_blocks_are_exact_and_context_bound(self):
        for sql, category in LOGMINER_RECURSIVE_BLOCKS.items():
            record = {'sql': sql, 'depth': 1, 'uid': 0, 'logminer_context': True}
            self.assertEqual(classify(record), category)
            self.assertTrue(classify(record, 'c5').startswith('rejected'))
            for changes in ({'depth': 0}, {'uid': 99}, {'logminer_context': False},
                            {'sql': sql + ' DELETE FROM APP.T'},
                            {'sql': sql.replace('SYS.', 'APP.').replace('SYSTEM.', 'APP.').replace('SELECT ', 'DELETE ')}):
                self.assertTrue(classify({**record, **changes}).startswith('rejected'))

    def test_original_41_only_session_settings_allowed_by_d12(self):
        records = json.loads(Path(__file__).with_name('fixtures').joinpath('d11-original-41.json').read_text())['records']
        self.assertEqual(len(records), 41)
        for record in records:
            if record.get('sql', '').strip().upper().startswith('ALTER SESSION SET '):
                self.assertEqual(classify(record), 'd12_session_state')
            else:
                self.assertTrue(classify(record).startswith('rejected'))

    def test_sys_and_system_dml_requires_recursive_context(self):
        for owner in ('SYS', 'SYSTEM'):
            for sql in (f'DELETE FROM {owner}.LOGMNR_GT_XID_INCLUDE$',
                        f'UPDATE {owner}.LOGMNR_SESSION$ SET FLAGS = :1 WHERE SESSION# = :2',
                        f'INSERT INTO {owner}.LOGMNR_SESSION$ (FLAGS) VALUES (:1)'):
                obj = logminer_dml(sql)
                self.assertIsNotNone(obj)
                self.assertTrue(obj.endswith('$'))
                record = {'action': sql.split()[0], 'sql': sql, 'objects': [obj],
                          'depth': 1, 'logminer_context': True}
                self.assertEqual(classify(record), 'd11a_logminer_object_dml')
                self.assertTrue(classify({**record, 'depth': None}).startswith('rejected'))
                self.assertTrue(classify({**record, 'logminer_context': False}).startswith('rejected'))
                self.assertTrue(classify({**record, 'depth': 0}).startswith('rejected'))
                self.assertTrue(classify({**record, 'objects': [obj, 'APP.T']}).startswith('rejected'))
                self.assertTrue(classify(record, 'c5').startswith('rejected'))

    def test_other_writes_and_driver_settings_fail(self):
        for sql in ('DELETE FROM APP.LOGMNR_GT_X', 'DELETE FROM SYS.AUD$',
                    'UPDATE SYSTEM.OTHER SET X=1', 'INSERT INTO AUDSYS.AUD$UNIFIED VALUES (:1)',
                    'DELETE FROM SYS.LOGMNR_GT_X WHERE ID IN (SELECT ID FROM APP.T)',
                    'DELETE FROM SYS.LOGMNR_GT_X; DELETE FROM APP.T',
                    'CREATE TABLE SYS.LOGMNR_GT_X(ID NUMBER)',
                    'BEGIN DBMS_LOGMNR.END_LOGMNR; DELETE FROM APP.T; END;'):
            self.assertTrue(classify({'action': sql.split()[0], 'sql': sql, 'depth': 1,
                                     'objects': ['SYS.LOGMNR_GT_X']}).startswith('rejected'))

    def test_unified_select_label_needs_correlated_recursive_dml(self):
        record = {'action': 'SELECT', 'sql': 'DELETE FROM SYS.LOGMNR_GT_X',
                  'objects': ['SYS.LOGMNR_GT_X']}
        self.assertTrue(classify(record).startswith('rejected'))
        record.update(depth=2, trace_sources=['trace-001'], logminer_context=True)
        self.assertEqual(classify(record), 'd11a_logminer_object_dml')
        self.assertTrue(classify({**record, 'depth': 0}).startswith('rejected'))
        self.assertTrue(classify({**record, 'objects': ['SYS.AUD$']}).startswith('rejected'))
        self.assertTrue(classify(record, 'c5').startswith('rejected'))

    def test_commit_requires_enclosing_call_in_same_session(self):
        call = 'BEGIN DBMS_LOGMNR.END_LOGMNR; END;'
        trace = ('*** SESSION ID:(1.2) now\n' + statement(1, call, 0, 100, 300, 99)
                 + statement(2, 'COMMIT', 1, 150, 200)
                 + statement(2, 'COMMIT', 1, 350, 400)
                 + '*** SESSION ID:(2.3) now\n' + statement(3, 'COMMIT', 1, 150, 200))
        records = trace_records(trace)
        self.assertEqual(classify(records[1]), 'd11a_logminer_commit')
        self.assertTrue(classify(records[2]).startswith('rejected'))
        self.assertTrue(classify(records[3]).startswith('rejected'))
        self.assertTrue(classify({'action': 'COMMIT', 'sql': 'COMMIT', 'depth': 0,
                                  'logminer_context': True}).startswith('rejected'))

    def test_all_commit_executions_must_be_enclosed(self):
        trace = ('*** SESSION ID:(1.2) now\n'
                 + statement(1, 'SELECT * FROM V$LOGMNR_CONTENTS', 0, 100, 300, 99)
                 + statement(2, 'COMMIT', 1, 150, 200)
                 + 'EXEC #2:e=50,tim=400\n')
        self.assertFalse(trace_records(trace)[1]['logminer_context'])

    def test_c5_only_reads_and_session_lifecycle(self):
        self.assertEqual(classify({'action': 'SELECT', 'sql': 'SELECT X FROM APP.T'}, 'c5'), 'read')
        self.assertEqual(classify({'action': 'LOGON'}, 'c5'), 'read_session')
        self.assertEqual(classify({'action': 'LOGOFF BY CLEANUP'}, 'c5'), 'read_session')
        for sql, action in [('COMMIT', 'COMMIT'),
                            ('BEGIN DBMS_LOGMNR.END_LOGMNR; END;', 'EXECUTE')]:
            self.assertTrue(classify({'sql': sql, 'action': action}, 'c5').startswith('rejected'))

    def test_d12_settings_are_not_arbitrary_calls(self):
        for mode in ('c1', 'c5'):
            self.assertEqual(classify({'sql': "ALTER SESSION SET TIME_ZONE='+08:00'",
                                       'depth': 0, 'action': 'ALTER SESSION'}, mode),
                             'd12_session_state')
            for sql in ("ALTER SYSTEM SET X=1", "ALTER SESSION SET X=1; DELETE FROM APP.T",
                        "BEGIN DBMS_STATS.GATHER_TABLE_STATS('APP','T'); END;"):
                self.assertTrue(classify({'sql': sql, 'depth': 0}, mode).startswith('rejected'))

    def test_blank_object_insert_is_resolved_from_trace_not_ignored(self):
        sql = 'INSERT /* QOSD */ INTO EXP_HEAD$ (EXP_ID) VALUES (:1)'
        text = '*** SESSION ID:(1.2) now\n' + statement(1, sql, 1, 100, 200)
        text = text.replace('oct=7', "oct=7 sqlid='abc123'")
        record = trace_records(text)[0]
        self.assertEqual((record['owner'], record['object'], record['sql_id']),
                         ('SYS', 'EXP_HEAD$', 'abc123'))
        self.assertEqual(classify(record, 'c5'), 'd12_dictionary_bookkeeping')
        for changes in ({'objects': []}, {'uid': 99}, {'depth': 0},
                        {'objects': ['APP.EXP_HEAD$']}, {'sql': 'INSERT INTO MYSTERY VALUES (:1)'}):
            self.assertTrue(classify({**record, **changes}, 'c5').startswith('rejected'))

    def test_d12_bookkeeping_requires_system_recursive_provenance(self):
        for sql, obj, category in (
            ('UPDATE SYS.USER$ SET X=:1', 'SYS.USER$', 'd12_logon_bookkeeping'),
            ('UPDATE SEQ$ SET X=:1', 'SYS.SEQ$', 'd12_dictionary_bookkeeping'),
            ('UPDATE SEG$ SET X=:1', 'SYS.SEG$', 'd12_dictionary_bookkeeping'),
            ('INSERT INTO SYS.AUD$ VALUES (:1)', 'SYS.AUD$', 'd12_audit_trail'),
            ('INSERT INTO AUDSYS.AUD$UNIFIED VALUES (:1)', 'AUDSYS.AUD$UNIFIED', 'd12_audit_trail')):
            record = {'sql': sql, 'action': sql.split()[0], 'objects': [obj], 'uid': 0, 'depth': 1}
            self.assertEqual(classify(record, 'c5'), category)
            for changes in ({'depth': 0}, {'depth': None}, {'uid': 99}, {'objects': ['APP.T']}):
                self.assertTrue(classify({**record, **changes}).startswith('rejected'))

    def test_internal_block_requires_enclosing_miner_call(self):
        sql = ('DECLARE BEGIN SYS.DBMS_LOGMNR_INTERNAL.DELETE_TABLE_INCLUSION_RULE(); '
               'SYS.DBMS_LOGMNR_INTERNAL.DELETE_USER_INCLUSION_RULE(); '
               'SYS.DBMS_LOGMNR_INTERNAL.DELETE_XID_INCLUSION_RULE(); END;')
        record = {'sql': sql, 'depth': 1, 'uid': 0, 'logminer_context': True}
        self.assertEqual(classify(record), 'd11a_logminer_internal')
        for changes in ({'depth': 0}, {'logminer_context': False}, {'uid': 99},
                        {'sql': sql.replace('DELETE_XID_INCLUSION_RULE()', 'OTHER()')}):
            self.assertTrue(classify({**record, **changes}).startswith('rejected'))


if __name__ == '__main__':
    unittest.main()
