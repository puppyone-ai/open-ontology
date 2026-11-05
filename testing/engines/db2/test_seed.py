import unittest
from seed import statements


class SeedTests(unittest.TestCase):
    def test_full_scale_has_distinct_tables_and_bounded_admin_transactions(self):
        sql, tables, rows = statements(full=True)
        self.assertEqual(rows, 2_000_000)
        self.assertEqual(len(tables), 231)
        self.assertEqual(len(set(tables)), 231)
        self.assertEqual(sql.count("INSERT INTO FIXTURE.EVENTS"), 20)
        self.assertEqual(sql.count("GRANT SELECT ON FIXTURE."), 231)
        self.assertIn("C299 VARCHAR(8)", sql)
        self.assertNotIn("C300", sql)
        self.assertIn("AS TIMESTAMP(12)", sql)

    def test_small_fixture_is_not_full_acceptance(self):
        sql, tables, rows = statements()
        self.assertEqual(rows, 2000)
        self.assertEqual(len(tables), 4)
        self.assertEqual(sql.count("INSERT INTO FIXTURE.EVENTS"), 1)
