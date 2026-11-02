import copy
import unittest
from unittest.mock import patch
import c5_sealed_nochange as c


class NochangeTest(unittest.TestCase):
    def setUp(self):
        self.manifest = dict(binding=dict(source_id="s", identity="i", inventory=[],
            exclusions={}, scope={}, scn="20"), content_sha256="a"*64, rows=12000)
        self.parent = dict(source_engine="oracle", capture_method="c5_diff", source_id="s",
            source_generation=2, snapshot=dict(kind="oracle_scn", value="20"), input_digest="b"*64)
        self.config = dict(protocol="oo.oracle.c5-sealed-nochange/v1", binding="b",
            parent_request={}, parent_snapshot={}, current_snapshot={"sha256": "c"*64})
        self.incoming = dict(protocol="oo.jobs.native-producer/v1", method="C5",
            source_id="s", binding="b", pin=3, acknowledged_source_generation=2)

    def run_case(self, after=None):
        with patch.object(c, "pinned", return_value=self.parent), patch.object(
                c, "snapshot", side_effect=[self.manifest, after or self.manifest]):
            return c.produce(self.config, self.incoming)

    def test_no_data_advance_and_no_live_freshness_claim(self):
        result = self.run_case()
        self.assertEqual(result["source_generation"], 2)
        self.assertFalse(result["data_gen"])
        self.assertFalse(result["source_connected"])
        self.assertFalse(result["source_time_advanced"])
        self.assertEqual(result, self.run_case())

    def test_changed_snapshot_refused(self):
        after = copy.deepcopy(self.manifest)
        after["content_sha256"] = "d"*64
        with self.assertRaisesRegex(ValueError, "snapshot_changed"):
            self.run_case(after)

    def test_cross_source_refused(self):
        self.incoming["source_id"] = "other"
        with self.assertRaisesRegex(ValueError, "occurrence_binding"):
            self.run_case()

    def test_wrong_generation_refused(self):
        self.incoming["acknowledged_source_generation"] = 3
        with self.assertRaisesRegex(ValueError, "occurrence_binding"):
            self.run_case()

    def test_c1_parent_refused(self):
        self.parent["capture_method"] = "c1_cdc"
        with self.assertRaisesRegex(ValueError, "parent_binding"):
            self.run_case()


if __name__ == "__main__":
    unittest.main()
