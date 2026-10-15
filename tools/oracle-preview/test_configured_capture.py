import unittest
from unittest.mock import patch
import configured_capture as capture
import c5_witness as witness

class ConfiguredCaptureTest(unittest.TestCase):
    def test_dictionary_controls_decimal_and_names(self):
        col={'name':'TOTAL_DUE','data_type':'NUMBER','precision':17,'scale':3,'nullable':'N'}
        self.assertEqual(capture.column_spec(col),{'name':'TOTAL_DUE','nullable':False,'type':'decimal128','precision':17,'scale':3})
    def test_unknown_type_and_negative_scale_refused(self):
        for kind,p,s in [('BINARY_DOUBLE',None,None),('NUMBER',12,-2)]:
            with self.assertRaises(ValueError): capture.column_spec({'name':'VALUE','data_type':kind,'precision':p,'scale':s,'nullable':'Y'})
    def test_arrow_decimal_zero_scale_uses_flatbuffer_default(self):
        with patch.object(witness,'_field_slot',side_effect=lambda buf,table,index: 0 if index==0 else None):
            result=witness._name_type((4).to_bytes(4,'little',signed=True),7,1)
        self.assertEqual((result['precision'],result['scale']),(4,0))

if __name__=='__main__':unittest.main()
