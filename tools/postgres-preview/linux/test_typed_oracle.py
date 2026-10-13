import unittest
from typed_oracle import value_matches
class ExactOracle(unittest.TestCase):
    def test_no_decimal_context_rounding(self):
        v={'kind':'numeric','coefficient':'12345678901234567890123456789012345678','negative':True,'scale':4}
        self.assertTrue(value_matches('-1234567890123456789012345678901234.5678',v,1700))
        v['coefficient']='12345678901234567890123456789012345679'
        self.assertFalse(value_matches('-1234567890123456789012345678901234.5678',v,1700))
    def test_type_and_null_cannot_be_forged(self):
        self.assertFalse(value_matches('1',{'kind':'text','value':'1'},20))
        self.assertTrue(value_matches(None,{'kind':'null'},17))
        self.assertFalse(value_matches(None,{'kind':'bytea','value':''},17))
    def test_binary_and_timestamp(self):
        self.assertTrue(value_matches('\\x00fffe',{'kind':'bytea','value':'00fffe'},17))
        self.assertFalse(value_matches('\\x00fffe',{'kind':'bytea','value':'00fffd'},17))
        self.assertTrue(value_matches('0001-01-01 00:00:00+00',{'kind':'timestamptz','value':'0001-01-01T00:00:00.000000Z'},1184))
        self.assertFalse(value_matches('2020-01-01 00:00:00.000001+00',{'kind':'timestamptz','value':'2020-01-01T00:00:00.000002Z'},1184))
if __name__ == '__main__': unittest.main()
