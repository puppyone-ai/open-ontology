import copy
import json
import os
import unittest
from unittest.mock import Mock

import event_reconciliation as events


def fixture():
    tables = []
    for name in ("header", "detail"):
        tables.append({"dataset": "app." + name, "physical": '"_oo"."' + name + '"',
                       "columns": [{"name": n, "physical_name": "p_" + n, "data_type": t}
                                   for n, t in [("tenant", "text"), ("company", "text"),
                                                ("event", "text"), ("amount", "numeric(12,2)"),
                                                ("paid_time", "timestamp without time zone")]]})
    base = {"keys": ["tenant", "company", "event"], "amount": "amount", "time": "paid_time",
            "scope": {"tenant": "101", "company": "1001"}}
    config = {"left": dict(base, dataset="app.header"), "right": dict(base, dataset="app.detail"),
              "period": {"start": "2026-09-01", "end": "2026-10-01"}}
    return tables, copy.deepcopy(config)


class EventReconciliationTest(unittest.TestCase):
    def test_aggregate_before_join_and_amounts_stay_exact(self):
        tables, config = fixture()
        sql = events.statement(tables, config)
        self.assertTrue(sql.startswith("select "))
        self.assertEqual(sql.count("group by"), 2)
        self.assertIn('sum("p_amount"::numeric)', sql)
        self.assertIn("full join r on l.k0=r.k0 and l.k1=r.k1 and l.k2=r.k2", sql)
        self.assertIn('\"p_paid_time\"<TIMESTAMP \'2026-10-01\'', sql)
        self.assertEqual(sql.count('"p_company"=\'1001\''), 2)
        self.assertIn("sum(l.amount)::text", sql)
        self.assertIn("is distinct from r.amount", sql)
        self.assertNotIn("coalesce(sum(l.amount),0)", sql)

    def test_invalid_scope_keys_types_or_period_fail_before_query(self):
        tables, config = fixture()
        for mutation in [
            lambda c: c["right"].update(scope={"tenant": "101"}),
            lambda c: c["left"].update(scope={}),
            lambda c: c["left"].update(keys=["event"]),
            lambda c: c["left"].update(keys=["tenant", "company", "company"]),
            lambda c: c["period"].update(end="2026-08-31"),
            lambda c: c["left"].update(amount="not_present"),
        ]:
            bad = copy.deepcopy(config)
            mutation(bad)
            db = Mock()
            with self.assertRaises(ValueError):
                events.reconcile(db, tables, bad)
            db.query.assert_not_called()
        for name, kind in [("amount", "double precision"), ("paid_time", "timestamp with time zone")]:
            bad = copy.deepcopy(tables)
            next(c for c in bad[0]["columns"] if c["name"] == name)["data_type"] = kind
            with self.assertRaises(ValueError):
                events.statement(bad, config)

    def test_observation_never_approves_semantics_and_rejects_incomplete_results(self):
        tables, config = fixture()
        observed = {"snapshot": "1:2:", "observed_at": "2026-10-22",
                    "matched_events": 2, "left_only_events": 0, "right_only_events": 0,
                    "unequal_amount_events": 0}
        for side in ("l", "r"):
            observed.update({side + "_" + k: 0 for k in (
                "rows", "events", "multirow_events", "null_key_rows", "null_amount_rows")})
            observed[side + "_rows"] = observed[side + "_events"] = 2
            observed[side + "_amount"] = "9007199254740993.01"
        result = events.reconcile(Mock(query=Mock(return_value=observed)), tables, config)
        self.assertFalse(result["business_accepted"])
        self.assertEqual(result["review"], "pending")
        self.assertEqual(result["observation"]["l_amount"], "9007199254740993.01")
        for bad in [{}, dict(observed, l_amount=1.2), dict(observed, matched_events=True),
                    dict(observed, left_only_events=1), dict(observed, unequal_amount_events=3)]:
            with self.assertRaises(ValueError):
                events.reconcile(Mock(query=Mock(return_value=bad)), tables, config)

    @unittest.skipUnless(os.environ.get("OO_EVENT_TEST_PSQL"), "requires local PostgreSQL test connection")
    def test_real_postgres_multirow_null_empty_and_boundary_cases(self):
        tables, config = fixture()
        data = [
            [("A", "10"), ("A", "15"), ("B", "3"), (None, "2")],
            [("A", "12"), ("A", "13"), ("C", "7"), (None, None)],
        ]
        for table, rows in zip(tables, data):
            values = [
                "('101','1001'," + ("NULL" if key is None else "'" + key + "'") + ","
                + ("NULL" if value is None else value) + "::numeric,TIMESTAMP '2026-09-10')"
                for key, value in rows]
            values.extend([
                "('101','1002','X',999::numeric,TIMESTAMP '2026-09-10')",
                "('101','1001','Y',999::numeric,TIMESTAMP '2026-10-01')",
            ])
            table["physical"] = "(select * from (values " + ",".join(values) + (
                ') v("p_tenant","p_company","p_event","p_amount","p_paid_time")) fixture')
        db = events.lib.ReadOnlyPostgres(json.loads(os.environ["OO_EVENT_TEST_PSQL"]), timeout_ms=8000)
        result = events.reconcile(db, tables, config)["observation"]
        for key, expected in {
            "l_rows": 4, "r_rows": 4, "l_events": 3, "r_events": 3,
            "l_multirow_events": 1, "r_multirow_events": 1,
            "matched_events": 1, "left_only_events": 2, "right_only_events": 2,
            "l_null_key_rows": 1, "r_null_key_rows": 1, "r_null_amount_rows": 1,
            "unequal_amount_events": 0, "l_amount": "30", "r_amount": "32",
        }.items():
            self.assertEqual(result[key], expected, key)
        config["period"] = {"start": "2027-01-01", "end": "2027-02-01"}
        empty = events.reconcile(db, tables, config)["observation"]
        self.assertEqual(empty["l_rows"], 0)
        self.assertEqual(empty["matched_events"], 0)
        self.assertIsNone(empty["l_amount"])
        self.assertIsNone(empty["r_amount"])


if __name__ == "__main__":
    unittest.main()
