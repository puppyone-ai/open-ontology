#!/usr/bin/env python3
import unittest
from pathlib import Path
import whole_run as w

DATASET = "q_schema.q_table"


def overlay(pii="none"):
    return {"columns": [
        {"dataset": DATASET, "name": "q_key", "pii": pii, "fact_sha256": "a" * 64},
        {"dataset": DATASET, "name": "q_amount", "pii": "none", "fact_sha256": "b" * 64},
    ]}


class DecisionsTest(unittest.TestCase):
    def test_confirmed_reviews_use_overlay_facts(self):
        decisions = w.decisions_from_overlay(overlay(), DATASET, w.ROLE)
        self.assertEqual(decisions["role"], w.ROLE)
        self.assertEqual(decisions["reviews"][0]["binds_fact"], "a" * 64)
        self.assertEqual(decisions["confirm_columns"], [DATASET + ".q_key", DATASET + ".q_amount"])
        self.assertEqual(w.canonical({"b": 2, "a": 1}), '{"a":1,"b":2}')

    def test_non_none_pii_is_refused(self):
        with self.assertRaises(ValueError):
            w.decisions_from_overlay(overlay("unknown"), DATASET, w.ROLE)

    def test_legacy_seal_omits_binding_vector(self):
        digest = "ab" * 32
        config = "cd" * 32
        catalog = {"config_digest": config, "schema_digest": digest, "source_generation": 3, "source_id": w.SOURCE}
        reviewed = {
            "effective_oos": {
                "catalog_version": "ee" * 32, "knowledge": {"id": "know", "version": "1"},
                "ontology_version": "ont", "policy": {"id": "pol", "version": "1"}, "project": "oracle",
            },
            "provenance": {"config_digest": config, "schema_digest": digest, "source_generation": 3, "source_id": w.SOURCE},
        }
        binding = w.gateway_binding(catalog, reviewed, DATASET, ["q_key"])
        self.assertNotIn("binding_vector", binding)
        self.assertEqual(binding["schema_hash"], digest)
        self.assertEqual(binding["config_digest"], config)
        self.assertEqual(binding["generation"], 3)
        self.assertEqual(binding["project"], "oracle")
        self.assertEqual(binding["artifact_sha256"], w.sha_bytes(w.canonical(catalog).encode()))

    def test_copy_secret_refuses_empty(self):
        src = Path("/tmp/oo-oracle-copy-secret-empty")
        dest = Path("/tmp/oo-oracle-copy-secret-dest")
        src.write_bytes(b"\n")
        with self.assertRaises(SystemExit):
            w.copy_secret(src, dest)
        src.unlink()


if __name__ == "__main__":
    unittest.main()
