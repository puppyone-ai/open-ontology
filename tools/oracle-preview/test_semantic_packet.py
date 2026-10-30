import unittest

import semantic_packet as packet

class SemanticPacketTests(unittest.TestCase):
    def test_exact_source_typed_values_not_arrow_precision_inference(self):
        number = {"type": "NUMBER", "precision": 38, "scale": 9, "nullable": "Y"}
        self.assertEqual(packet.cell({"type": "decimal", "value": "99999999999999999999.123456789"}, number),
                         "99999999999999999999.123456789")
        self.assertEqual(packet.cell({"type": "decimal", "value": "-0.000000000"}, number), "0")
        self.assertEqual(packet.cell({"type": "bytes", "value": "00FF"}, {"type": "RAW", "nullable": "Y"}), "00ff")
        self.assertIsNone(packet.cell({"type": "null", "value": None}, number))

    def test_unknown_temporal_wire_and_invalid_null_or_bytes_refuse(self):
        for value, column in [
            ({"type": "null", "value": None}, {"type": "NUMBER", "nullable": "N"}),
            ({"type": "bytes", "value": "abc"}, {"type": "RAW", "nullable": "Y"}),
            ({"type": "text", "value": "1.2"}, {"type": "NUMBER", "nullable": "Y"}),
            ({"type": "text", "value": "123456"}, {"type": "DATE", "nullable": "Y"}),
        ]:
            with self.assertRaises(ValueError):
                packet.cell(value, column)

if __name__ == "__main__":
    unittest.main()
