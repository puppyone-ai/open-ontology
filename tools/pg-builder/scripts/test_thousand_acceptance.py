import copy
import threading
import time
import unittest
from unittest.mock import patch

import live_thousand_acceptance as live


class ThousandAcceptanceTests(unittest.TestCase):
    def test_tls_entrypoint_forwards_new_fixture_server_arguments(self):
        fixture = object.__new__(live.Fixture)
        with patch.object(live.live.LaneRun, "command", return_value=(0, "")) as command:
            fixture.command(["docker", "create", "--name", "fixture",
                             "docker.io/library/postgres:17@sha256:fixture",
                             "-c", "logging_collector=on"])
        argv = command.call_args.args[0]
        image = argv.index("docker.io/library/postgres:17@sha256:fixture")
        self.assertEqual(argv[image + 1], "-c")
        self.assertIn('"$@"', argv[image + 2])
        self.assertEqual(argv[image + 3:], ["oo-postgres", "-c", "logging_collector=on"])

    def test_generator_retains_thousand_tables_and_shapes(self):
        sql = "".join(live.live.fixture.generator.generate("postgres", "17", 1000, 1000))
        self.assertEqual(sql.count("CREATE TABLE small_"), 1000)
        for shape in ("CREATE TABLE wide_rows", "c299 BIGINT", "CREATE TABLE log_202612",
                      "REFERENCES customers(id)", "CREATE TABLE events", "CREATE TABLE no_key"):
            self.assertIn(shape, sql)

    def test_agent_inventory_uses_target_types_and_bound_source_keys(self):
        source = {"tables": [{"dataset": "public.a", "primary_key": ["id"],
                             "foreign_keys": [], "columns": [{"name": "id", "data_type": "int"}]}]}
        target = {"tables": [{"dataset": "oo_load_data.t_a",
                             "columns": [{"name": "id", "data_type": "bigint"}]}]}
        receipts = [{"table": {"schema": "public", "name": "a"},
                     "target": "oo_load_data.t_a", "receipts": [{}]}]
        result = live.loaded_inventory(source, target, receipts)
        self.assertEqual(result["tables"][0]["columns"], target["tables"][0]["columns"])
        self.assertEqual(result["tables"][0]["primary_key"], ["id"])
        self.assertEqual(source["tables"][0]["columns"][0]["data_type"], "int")
        bad = copy.deepcopy(target)
        bad["tables"][0]["columns"][0]["name"] = "wrong"
        with self.assertRaisesRegex(ValueError, "acceptance_load_columns"):
            live.loaded_inventory(source, bad, receipts)
        with self.assertRaisesRegex(ValueError, "acceptance_load_count"):
            live.loaded_inventory(source, target, receipts * 2)

    def test_budget_reserves_before_upstream_and_never_refunds(self):
        egress = object.__new__(live.LimitedEgress)
        egress.limit_lock = threading.Lock()
        egress.admissions, egress.reserved_tokens = 0, 0
        egress.deadline = time.monotonic() + 60
        request = {"max_tokens": 100, "messages": []}
        with patch.object(live.live.egress_module.Egress, "call", return_value=iter([{}])) as call:
            self.assertEqual(list(egress.call(request)), [{}])
            self.assertGreater(egress.reserved_tokens, 10100)
            egress.admissions = 80
            with self.assertRaises(live.live.egress_module.Refusal):
                list(egress.call(request))
            self.assertEqual(call.call_count, 1)
            egress.admissions, egress.reserved_tokens = 0, 2_000_000
            with self.assertRaises(live.live.egress_module.Refusal):
                list(egress.call(request))
            egress.reserved_tokens, egress.deadline = 0, 0
            with self.assertRaises(live.live.egress_module.Refusal):
                list(egress.call(request))
            self.assertEqual(call.call_count, 1)

    def test_community_bound_rejects_either_overflow(self):
        batch = {"input_tokens": 100, "input_available": 100, "output_reserve": 20}
        result = {"graph": {"communities": [{"batches": [batch]}]}}
        self.assertTrue(live.bounded_communities(result, 20)["all_within_budget"])
        with self.assertRaisesRegex(ValueError, "acceptance_community_budget"):
            live.bounded_communities(result, 19)
        batch["input_tokens"] += 1
        with self.assertRaisesRegex(ValueError, "acceptance_community_budget"):
            live.bounded_communities(result, 20)


if __name__ == "__main__":
    unittest.main()
