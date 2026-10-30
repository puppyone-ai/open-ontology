import unittest

import library_observation as observation


class TextShapeTest(unittest.TestCase):
    def test_only_aggregate_shapes_without_raw_values(self):
        sql = observation.text_shape_sql('"_oo"."pt_test"', 'pc_payload')
        self.assertTrue(sql.startswith('select json_build_object'))
        self.assertIn('"pc_payload" is json object', sql)
        self.assertIn('"pc_payload" is json array', sql)
        self.assertNotIn('::json', sql)
        self.assertNotIn('jsonb_each', sql)
        self.assertNotIn('jsonb_object_keys', sql)

    def test_physical_binding_rejects_expressions(self):
        for physical in ['_oo.pt_test', '"_oo"."pt_test"; DELETE FROM x', 'select 1', None]:
            with self.subTest(physical=physical):
                with self.assertRaises(ValueError):
                    observation.text_shape_sql(physical, 'payload')

    def test_column_is_an_identifier_not_sql(self):
        sql = observation.text_shape_sql('"_oo"."pt_test"', 'a"; select')
        self.assertIn('"a""; select" is json', sql)


if __name__ == '__main__':
    unittest.main()
