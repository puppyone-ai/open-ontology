import json
from pathlib import Path
import unittest

from audit import classify, logminer_dml, trace_records


def statement(cursor, sql, depth, start, end, uid=0):
    return (f'PARSING IN CURSOR #{cursor} len={len(sql)} dep={depth} uid={uid} oct=7\n'
            f'{sql}\nEND OF STMT\nEXEC #{cursor}:e={end-start},tim={end}\n')


class D11aTests(unittest.TestCase):
    def test_original_41_remain_fail_closed_without_owner_or_context(self):
        records = json.loads(Path(__file__).with_name('fixtures').joinpath('d11-original-41.json').read_text())['records']
        self.assertEqual(len(records), 41)
        self.assertTrue(all(classify(r).startswith('rejected') for r in records))

    def test_sys_and_system_dml_without_depth(self):
        for owner in ('SYS', 'SYSTEM'):
            for sql in (f'DELETE FROM {owner}.LOGMNR_GT_XID_INCLUDE$',
                        f'UPDATE {owner}.LOGMNR_SESSION$ SET FLAGS = :1 WHERE SESSION# = :2',
                        f'INSERT INTO {owner}.LOGMNR_SESSION$ (FLAGS) VALUES (:1)'):
                obj = logminer_dml(sql)
                self.assertIsNotNone(obj)
                self.assertTrue(obj.endswith('$'))
                record = {'action': sql.split()[0], 'sql': sql, 'objects': [obj]}
                self.assertEqual(classify(record), 'd11a_logminer_object_dml')
                self.assertTrue(classify({**record, 'depth': 0}).startswith('rejected'))
                self.assertTrue(classify({**record, 'objects': [obj, 'APP.T']}).startswith('rejected'))
                self.assertTrue(classify(record, 'c5').startswith('rejected'))

    def test_other_writes_and_driver_settings_fail(self):
        for sql in ('DELETE FROM APP.LOGMNR_GT_X', 'DELETE FROM SYS.AUD$',
                    'UPDATE SYSTEM.OTHER SET X=1', 'INSERT INTO AUDSYS.AUD$UNIFIED VALUES (:1)',
                    'DELETE FROM SYS.LOGMNR_GT_X WHERE ID IN (SELECT ID FROM APP.T)',
                    'DELETE FROM SYS.LOGMNR_GT_X; DELETE FROM APP.T',
                    'CREATE TABLE SYS.LOGMNR_GT_X(ID NUMBER)',
                    'BEGIN DBMS_LOGMNR.END_LOGMNR; DELETE FROM APP.T; END;',
                    "ALTER SESSION SET TIME_ZONE='+08:00'"):
            self.assertTrue(classify({'action': sql.split()[0], 'sql': sql, 'depth': 1,
                                     'objects': ['SYS.LOGMNR_GT_X']}).startswith('rejected'))

    def test_unified_select_label_needs_correlated_recursive_dml(self):
        record = {'action': 'SELECT', 'sql': 'DELETE FROM SYS.LOGMNR_GT_X',
                  'objects': ['SYS.LOGMNR_GT_X']}
        self.assertTrue(classify(record).startswith('rejected'))
        record.update(depth=2, trace_sources=['trace-001'])
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


if __name__ == '__main__':
    unittest.main()
