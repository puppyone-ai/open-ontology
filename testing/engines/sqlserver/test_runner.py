import unittest
from run import decode_audit


class AuditTransport(unittest.TestCase):
    def test_wrapping_preserves_multiline_cdata(self):
        xml = '<root><![CDATA[SELECT 1;\nSELECT 2;]]></root>'
        encoded = xml.encode('utf-16-le').hex()
        wrapped = 'audit_hex\n----------\n' + '\n'.join(encoded[i:i+17] for i in range(0, len(encoded), 17))
        self.assertEqual(decode_audit(wrapped), xml)

    def test_unexpected_output_is_not_discarded(self):
        with self.assertRaises(ValueError):
            decode_audit('audit_hex\nerror from observer\n')


if __name__ == '__main__':
    unittest.main()
