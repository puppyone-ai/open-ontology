"""Host checks for the C5 native producer. No database and no loader."""

from __future__ import annotations

import unittest

import c5_native_join as join


class NativeJoinProducer(unittest.TestCase):
    def test_schema_message_matches_committed_arrows(self):
        for name, expected in join.COMMITTED_SCHEMA_MESSAGE.items():
            blob = (join.RUN / "arrow" / name).read_bytes()
            self.assertEqual(join.arrow_schema_message_sha256(blob), expected)
            self.assertNotEqual(join.sha256_hex(blob), expected)

    def test_ident_typmod_and_preimage(self):
        self.assertEqual(join.stable_ident("ID"), "q_4944")
        self.assertEqual(join.stable_ident("N_FREE"), "q_4e5f46524545")
        self.assertEqual(join.numeric_typmod(16, 6), 1048586)
        preimage = join.schema_preimage("oo04_fix", "t_wide", [("q_4944", 21, -1)])
        self.assertTrue(preimage.endswith("\t\n"))
        self.assertIn("oo04_fix.t_wide\n", preimage)
        digest = join.sha256_hex(preimage.encode())
        self.assertNotEqual(digest, join.COMMITTED_SCHEMA_MESSAGE["t_wide-gen-1.arrow"])

    def test_dictionary_follows_shadow_column_order(self):
        batch = join.dictionary_for("T_BATCH", 1)
        self.assertEqual([column["name"] for column in batch], ["ID", "N16", "N1", "D", "C6", "V2", "R"])
        later = join.dictionary_for("T_BATCH", 8)
        self.assertEqual(later[-1]["name"], "C5_NOTE")
        ts = join.dictionary_for("T_TS", 1)
        self.assertEqual(ts[1]["nullable"], "N")
        self.assertTrue(join.arrow_declaration(ts[1])["nullable"])
        self.assertFalse(join.arrow_declaration(batch[0])["nullable"])
        self.assertEqual(join.sealed_type(batch[0])[:3], (21, -1, "int2"))
        self.assertEqual(join.sealed_type(next(column for column in batch if column["name"] == "N16"))[0], 1700)

    def test_wide_recompute_matches_recorded_failure(self):
        ctx = join.capture_context()
        built = join.build_load(
            "T_WIDE", 1, join.RUN / "arrow" / "t_wide-gen-1.arrow", 1,
            ctx["config_digest"], ctx["audit_digest"], ctx["driver_digest"], ctx["banner"],
            "/run/oo04-trust", persist=False,
        )
        for key, expected in join.RECORDED_WIDE.items():
            self.assertEqual(built[key], expected)
        self.assertEqual(built["load"]["columns"][0]["name"], "ID")
        self.assertEqual(built["load"]["columns"][1]["name"], "N_FREE")
        pairs = join.mapping_pairs(built)
        self.assertEqual(pairs[0]["once_physical"], "pc_6fa7f9b6a5807af7479b3444ec270b5ad65ea30f1db0daaefea443f7eef6")
        self.assertEqual(pairs[0]["twice_physical"], "pc_060bc08ff3ca60a5ba0725d10b54207f8d92ee6c5e9fdf631c7b1b59d38e")
        self.assertEqual(pairs[1]["twice_physical"], "pc_5eb6738b28343ccb010d15cfeecd1fa528866ca4a0272b2caf16d271d8a8")
        self.assertFalse(join.ident_ok_71("ID"))
        self.assertTrue(join.ident_ok_71("q_4944"))

    def test_intake_keeps_oracle_names_and_refuses_the_postgres_pin(self):
        documents = join.intake_documents()
        names = [name for table in documents["catalog"]["tables"] for name in table["columns"]]
        self.assertIn("ID", names)
        self.assertIn("N_FREE", names)
        self.assertIn("C5_NOTE", names)
        self.assertNotIn("id", names)
        verdict = join.intake_verdict(documents)
        self.assertFalse(verdict["consumable_by_current_pin"])
        self.assertIn("catalog_protocol", verdict["refused"])
        self.assertIn("ident_ok", verdict["refused"])
        self.assertIn("ID", verdict["ident_rejected"])
        self.assertFalse(verdict["relabelled_as_postgres"])
        supports = {item["logical_name"]: item["support"] for item in verdict["counterfactual_if_pin_extended_without_weakening_oos"]}
        self.assertEqual(supports["q_4944"], "unknown")
        self.assertEqual(supports["q_4e5f46524545"], "unknown")

    def test_maintenance_records_added_column_without_a_lowercase_alias(self):
        changes = join.maintenance_changes()
        added = [change for change in changes if change["kind"] == "change" and change["columns_added"]]
        self.assertTrue(any("C5_NOTE" in change["columns_added"] for change in added))
        requests = join.maintenance_requests(changes)
        schema = [item for item in requests if item["body"]["trigger"] == "schema_changed"]
        self.assertTrue(schema)
        self.assertTrue(all(item["posted"] is False for item in requests))
        self.assertFalse(any(item["column_ident_ok"] for item in schema))

    def test_child_generation_keeps_applied_pos_zero(self):
        self.assertIn("F4", " ".join(join.load_blockers(8)))
        self.assertNotIn("F4", " ".join(join.load_blockers(1)))


if __name__ == "__main__":
    unittest.main()
