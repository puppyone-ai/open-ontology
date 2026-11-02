import copy
import unittest
from unittest.mock import Mock, patch

import declared_enums as adapter
import library_selfbuild as lib
from observation_test_support import observed_fixture


def fixture():
    roles = {
        "controls": ["id", "entity_key", "table_schema", "table_name", "table_column"],
        "headers": ["ctl_id", "entity_key", "dictionary_type"],
        "dictionary": ["type_code", "code", "label", "active", "tenant", "company"],
        "items": ["status"],
    }
    config = {"schema_aliases": {"legacy": "app"}, "max_rows": 500}
    tables = []
    for role, fields in roles.items():
        columns = []
        for name in fields:
            source = "refEnum" if role == "headers" and name == "dictionary_type" else name
            logical = "q_" + source.encode().hex() if source == "refEnum" else source
            columns.append(dict(name=logical, source_name=source, physical_name="p_" + name,
                                data_type="text", comment="status" if name == "status" else "",
                                profile={"rows": 2, "distinct": 2}))
        tables.append(dict(dataset="app." + role, physical='"_oo"."'+role+'"',
                           source_id="fixture", source_generation=1, target_generation=1,
                           primary_key=[], columns=columns))
        if role != "items":
            config[role] = {"dataset": "app." + role,
                            "columns": {k: "refEnum" if k == "dictionary_type" else k
                                        for k in adapter.ROLES[role]}}
    config["dictionary"]["scope"] = {"tenant": "101", "company": "1001"}
    tables = sorted(observed_fixture(tables), key=lambda t: t["dataset"])
    declaration = {"table_schema": "legacy", "table_name": "items",
                   "table_column": "status", "dictionary_type": "Payment"}
    value = {"type_code": "Payment", "code": "1", "label": "paid", "active": "1"}
    return tables, config, declaration, value


class DeclaredEnumsTest(unittest.TestCase):
    def test_scoped_source_bound_and_pending(self):
        tables, config, declaration, value = fixture()
        db = Mock(query=Mock(side_effect=[[declaration], [value]]))
        evidence = adapter.discover(db, tables, config)
        first, second = [c.args[0] for c in db.query.call_args_list]
        self.assertIn('h."p_ctl_id"=c."p_id"', first)
        self.assertIn('h."p_entity_key"=c."p_entity_key"', first)
        self.assertIn("limit 501", second)
        self.assertIn('d."p_tenant"=\'101\'', second)
        self.assertIn('d."p_company"=\'1001\'', second)
        binding = evidence["bindings"][0]
        self.assertEqual(binding["enum"], "app.items.status")
        self.assertEqual(binding["review"], "pending")
        self.assertEqual(binding["schema_alias_review"], "pending")
        self.assertFalse(evidence["published"])

    def test_overflow_conflicts_and_unknown_columns(self):
        tables, config, declaration, value = fixture()
        for responses in [[[declaration] * 501], [[declaration], [value] * 501]]:
            with self.assertRaisesRegex(ValueError, "truncated"):
                adapter.discover(Mock(query=Mock(side_effect=responses)), tables, config)
        evidence = adapter.discover(Mock(query=Mock(side_effect=[
            [declaration], [value, dict(value, label="unpaid")]])), tables, config)
        self.assertEqual(evidence["bindings"][0]["state"], "conflict")
        for field, bad in [("table_name", "missing"), ("table_column", "missing")]:
            evidence = adapter.discover(Mock(query=Mock(side_effect=[
                [dict(declaration, **{field: bad})], [value]])), tables, config)
            self.assertEqual(evidence["bindings"], [])
            self.assertEqual(len(evidence["unbound"]), 1)

    def test_invalid_config_does_not_query(self):
        tables, config, _, _ = fixture()
        for mutate in [
            lambda c: c.update(max_rows=True),
            lambda c: c.update(schema_aliases={}),
            lambda c: c["dictionary"].update(scope={}),
            lambda c: c["headers"]["columns"].update(dictionary_type="not_present"),
        ]:
            bad = copy.deepcopy(config)
            mutate(bad)
            db = Mock()
            with self.assertRaises(ValueError):
                adapter.discover(db, tables, bad)
            db.query.assert_not_called()

    def test_product_facts_drafts_and_review_queue_share_evidence(self):
        tables, config, declaration, value = fixture()
        evidence = adapter.discover(Mock(query=Mock(side_effect=[[declaration], [value]])), tables, config)
        facts = lib.build_facts(tables, lambda *args: {}, declared=evidence)
        lib.require_complete_facts(facts, tables, config)
        candidate = next(e for e in facts["enum_candidates"] if e["enum"] == "app.items.status")
        self.assertEqual(candidate["declared_binding"], evidence["bindings"][0])
        compact = next(e for e in lib.draft_batches(facts)[0]["enum_candidates"]
                       if e["enum"] == "app.items.status")
        self.assertEqual(compact["declared_binding"]["values"], [value])
        effective, visibility, review = lib.effective_documents(facts, [])
        self.assertTrue(any(d["kind"] == "declared_enum_binding" for d in review["decisions"]))
        self.assertFalse(any(v.get("status") == "approved" for e in effective["enums"] for v in e["values"]))
        with self.assertRaisesRegex(ValueError, "config_drift"):
            lib.require_complete_facts(facts, tables, dict(config, max_rows=499))
        bad = copy.deepcopy(facts)
        bad["declared_enum_observation"]["bindings"][0]["values"][0]["label"] = "forged"
        bad["facts_sha256"] = lib.digest({k: v for k, v in bad.items() if k != "facts_sha256"})
        with self.assertRaisesRegex(ValueError, "observation_binding"):
            lib.require_complete_facts(bad)

    def test_product_entry_observes_configured_adapter(self):
        tables, config, _, _ = fixture()
        with patch.object(lib.declared_enums, "discover", return_value={"bindings": []}) as discover, \
                patch.object(lib, "build_facts", return_value={}) as build:
            lib.observe_facts(Mock(), tables, {"declared_enum_adapter": config})
        discover.assert_called_once()
        self.assertEqual(build.call_args.kwargs["declared"], {"bindings": []})

    def test_retained_evidence_requires_exact_inventory_and_candidate_set(self):
        tables, config, declaration, value = fixture()
        evidence = adapter.discover(Mock(query=Mock(side_effect=[[declaration], [value]])), tables, config)
        facts = lib.build_facts(tables, lambda *args: {}, declared=evidence)
        for mutation, error in [
            (lambda f: f.pop("declared_enum_observation"), "observation_required"),
            (lambda f: f["enum_candidates"].append(copy.deepcopy(f["enum_candidates"][0])),
             "candidate_binding"),
            (lambda f: f["declared_enum_observation"].update(bindings=[]), "candidate_binding"),
            (lambda f: f["tables"][0].update(label_candidate="different observation"),
             "observation_binding"),
        ]:
            bad = copy.deepcopy(facts)
            mutation(bad)
            if "declared_enum_observation" in bad:
                observation = bad["declared_enum_observation"]
                observation["evidence_sha256"] = adapter.digest(
                    {k: v for k, v in observation.items() if k != "evidence_sha256"})
            bad["facts_sha256"] = lib.digest({k: v for k, v in bad.items() if k != "facts_sha256"})
            with self.subTest(error=error), self.assertRaisesRegex(ValueError, error):
                lib.require_complete_facts(bad)
        reversed_tables = copy.deepcopy(tables)
        reversed_tables.reverse()
        for table in reversed_tables:
            table["columns"].reverse()
        self.assertEqual(adapter.inventory_digest(tables), adapter.inventory_digest(reversed_tables))


if __name__ == "__main__":
    unittest.main()
