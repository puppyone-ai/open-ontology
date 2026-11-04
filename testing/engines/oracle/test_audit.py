import json
from pathlib import Path
import unittest

from audit import classify, normalized, temp_delete, trace_records


class D11Tests(unittest.TestCase):
    def test_original_41_are_retained_and_missing_depth_is_not_invented(self):
        records = json.loads(Path(__file__).with_name('fixtures').joinpath('d11-original-41.json').read_text())['records']
        self.assertEqual(len(records), 41)
        outcomes = [classify(r) for r in records]
        self.assertEqual(outcomes.count('d11_driver_session_setting'), 1)
        self.assertEqual(outcomes.count('rejected_missing_or_nonrecursive_depth'), 40)
        for record in records:
            if record['action'] == 'ALTER SESSION':
                continue
            sql = normalized(record['sql'])
            obj = temp_delete(sql)
            # Explicit synthetic depth/transaction-context cases; these are
            # not asserted to be trace evidence for the historical records.
            if obj:
                enriched = {**record, 'depth': 1, 'objects': [obj]}
                self.assertEqual(classify(enriched), 'd11_recursive_temporary_write')
                for depth in (None, 0, -1):
                    self.assertTrue(classify({**enriched, 'depth': depth}).startswith('rejected'))
                self.assertTrue(classify({**enriched, 'objects': [obj, 'APP.T']}).startswith('rejected'))
            elif sql == 'COMMIT':
                self.assertTrue(classify({**record, 'depth': 1, 'objects': []}).startswith('rejected'))
                self.assertEqual(classify({**record, 'depth': 1, 'objects': ['SYS.LOGMNR_GT_XID_INCLUDE$']}), 'd11_recursive_temporary_write')
            else:
                self.fail('unaccounted historical record')

    def test_foreign_object_and_top_level_writes_fail(self):
        for sql in ['DELETE FROM APP.LOGMNR_GT_X', 'DELETE FROM SYS.OTHER',
                    'DELETE FROM SYS.LOGMNR_GT_X WHERE ID IN (SELECT ID FROM APP.T)',
                    'DELETE FROM SYS.LOGMNR_GT_X; DELETE FROM APP.T',
                    'CREATE TABLE SYS.LOGMNR_GT_X(ID NUMBER)',
                    'BEGIN DBMS_LOGMNR.END_LOGMNR; DELETE FROM APP.T; END;']:
            self.assertTrue(classify({'action': 'EXECUTE', 'sql': sql, 'depth': 1,
                                     'objects': ['SYS.LOGMNR_GT_X']}).startswith('rejected'))
        self.assertTrue(classify({'action': 'ALTER SESSION', 'sql': "ALTER SESSION SET CURRENT_SCHEMA='SYS'"}).startswith('rejected'))

    def test_trace_parser_keeps_depth(self):
        trace = 'PARSING IN CURSOR #123 len=40 dep=1 uid=0 oct=7\nDELETE FROM SYS.LOGMNR_GT_XID_INCLUDE$\nEND OF STMT\n'
        record, = trace_records(trace)
        self.assertEqual(record['depth'], 1)
        self.assertEqual(classify(record), 'd11_recursive_temporary_write')


if __name__ == '__main__':
    unittest.main()
