import tempfile
import unittest
from pathlib import Path
import full_source as fs

class SourceAuditTests(unittest.TestCase):
    def check_window(self, text):
        with tempfile.TemporaryDirectory() as tmp:
            original = fs.EVID
            try:
                fs.EVID = Path(tmp)
                return fs.write_audit_classes(text)
            finally:
                fs.EVID = original
    def test_denied_insert_is_still_attempt(self):
        self.assertFalse(self.check_window('audit|OO04_READER|INSERT|2004|OO04_FIX|T_BATCH|INSERT INTO T_BATCH\naudit|done\n'))
    def test_success_insert_fails(self):
        self.assertFalse(self.check_window('audit|OO04_READER|INSERT|0|OO04_FIX|T_BATCH|INSERT INTO T_BATCH\naudit|done\n'))
    def test_exact_driver_session_allowed(self):
        self.assertTrue(self.check_window("audit|OO04_READER|ALTER SESSION|0|-|-|ALTER SESSION SET TIME_ZONE='+00:00'\naudit|done\n"))
    def test_other_session_mutation_fails(self):
        self.assertFalse(self.check_window('audit|OO04_READER|ALTER SESSION|0|-|-|ALTER SESSION SET CURRENT_SCHEMA=SYS\naudit|done\n'))
    def test_missing_session_sql_fails(self):
        self.assertFalse(self.check_window('audit|OO04_READER|ALTER SESSION|0|-|-\naudit|done\n'))
    def test_incomplete_audit_fails(self):
        self.assertFalse(self.check_window('audit|OO04_READER|SELECT|0|OO04_FIX|T_BATCH|SELECT ID FROM T_BATCH\n'))

if __name__ == '__main__': unittest.main()
