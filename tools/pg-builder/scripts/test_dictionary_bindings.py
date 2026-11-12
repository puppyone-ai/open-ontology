import copy
import json
import sqlite3
import unittest
import hashlib
from pathlib import Path
import tempfile

import dictionary_bindings as bindings


class Replica:
    def __init__(self):
        self.db = sqlite3.connect(":memory:")
        self.db.executescript("""
            create table events(code text, happened text);
            create table dictionary(kind text, code text, label text, lo text, hi text);
            insert into dictionary values
              ('state','A','old','2020-01-01','2022-01-01'),
              ('state','A','new','2022-01-01',null),
              ('other','A','unrelated','2020-01-01',null);
            insert into events values ('A','2021-12-31'),('A','2022-01-01'),
              ('A','2030-01-01');
        """)

    def query(self, sql):
        # Exercise the generated relational logic independently; this adapter
        # is not evidence of PostgreSQL parsing or a live retained publication.
        sql = sql.replace("json_build_object", "json_object").replace("::text", "")
        sql = sql.replace("(hi + interval '1 day')", "date(hi, '+1 day')")
        return json.loads(self.db.execute(sql).fetchone()[0])


def fixture():
    inventory = {
        "app.events": {"physical": '"events"', "columns": [
            {"name": n, "physical_name": n} for n in ("code", "happened")]},
        "app.dictionary": {"physical": '"dictionary"', "columns": [
            {"name": n, "physical_name": n, "data_type": "date" if n in ("lo", "hi") else "text"}
            for n in ("kind", "code", "label", "lo", "hi")]},
    }
    spec = {
        "id": "state", "concept": "Event state",
        "from": {"dataset": "app.events", "code": "code", "at": "happened"},
        "dictionary": {"dataset": "app.dictionary", "discriminator": "kind",
                       "value": "state", "code": "code", "label": "label",
                       "valid_from": "lo", "valid_to": "hi"},
        "interval": "closed_open_null_end_unbounded",
        "provenance": {"origin": "operator_reviewed", "reviewer": "fixture-owner",
                       "source_sha256": "a" * 64, "reviewed_at": "2026-11-12"},
    }
    return inventory, spec


class DictionaryBindingTests(unittest.TestCase):
    def setUp(self):
        self.replica = Replica()
        self.addCleanup(self.replica.db.close)
        self.inventory, self.spec = fixture()

    def measure(self):
        return bindings.measure(self.replica, self.inventory, self.spec)

    def test_repeated_codes_history_boundary_and_unbounded_end(self):
        decision, evidence = self.measure()
        self.assertEqual(decision, "complete")
        self.assertEqual(evidence["matched_rows"], 3)
        self.assertEqual(evidence["overlap_pairs"], 0)
        rows = self.replica.db.execute("""
            select label from events c join dictionary d on c.code=d.code
             and d.kind='state' and c.happened>=d.lo and (d.hi is null or c.happened<d.hi)
            order by c.happened
        """).fetchall()
        self.assertEqual(rows, [("old",), ("new",), ("new",)])

    def test_duplicate_interval_is_ambiguous_not_unique(self):
        self.replica.db.execute("insert into dictionary select * from dictionary where label='new'")
        decision, evidence = self.measure()
        self.assertEqual(decision, "incomplete")
        self.assertEqual(evidence["ambiguous_rows"], 2)
        self.assertEqual(evidence["overlap_pairs"], 1)

    def test_inclusive_date_boundaries_and_lookup_only_coverage(self):
        self.spec["interval"] = "closed_closed_date_null_end_unbounded"
        self.replica.db.execute("update dictionary set hi='2021-12-31' where label='old'")
        decision, evidence = self.measure()
        self.assertEqual(decision, "complete")
        self.assertEqual(evidence["gap_pairs"], 0)
        self.assertEqual(evidence["matched_rows"], 3)
        self.spec["from"] = None
        decision, evidence = self.measure()
        self.assertEqual(decision, "incomplete")
        self.assertEqual(evidence["rows_checked"], 0)
        self.assertEqual(evidence["overlap_pairs"], 0)

    def test_gap_and_uncovered_reference_are_retained(self):
        self.replica.db.execute("update dictionary set hi='2021-01-01' where label='old'")
        decision, evidence = self.measure()
        self.assertEqual(decision, "incomplete")
        self.assertEqual(evidence["gap_pairs"], 1)
        self.assertEqual(evidence["matched_rows"], 2)
        self.replica.db.execute("insert into events values ('missing','2023-01-01')")
        self.assertEqual(self.measure()[1]["rows_checked"], 4)
        self.assertEqual(self.measure()[1]["matched_rows"], 2)

    def test_invalid_interval_and_missing_event_time_do_not_certify(self):
        self.replica.db.execute("update dictionary set hi=lo where label='old'")
        self.assertEqual(self.measure()[1]["invalid_intervals"], 1)
        self.replica.db.execute("insert into events values ('A',null)")
        self.assertEqual(self.measure()[0], "incomplete")

    def test_no_review_or_unknown_column_fails_before_query(self):
        from unittest.mock import Mock
        for mutate in (
            lambda s: s["provenance"].update(origin="model"),
            lambda s: s.update(interval="inclusive"),
            lambda s: s["dictionary"].update(label="secret"),
        ):
            spec = copy.deepcopy(self.spec)
            mutate(spec)
            db = Mock()
            with self.assertRaises((ValueError, KeyError)):
                bindings.measure(db, self.inventory, spec)
            db.query.assert_not_called()

    def test_discriminator_is_a_literal_not_sql(self):
        self.spec["dictionary"]["value"] = "state' OR 1=1 --"
        self.assertEqual(self.measure()[1]["matched_rows"], 0)

    def test_retained_input_is_explicit_and_hash_pinned(self):
        from retained_build import reviewed_dictionary_bindings
        self.assertEqual(reviewed_dictionary_bindings({}), [])
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "bindings.json"
            path.write_text(json.dumps({
                "protocol": "oo.reviewed-dictionary-bindings/v1", "bindings": [self.spec]}))
            config = {"dictionary_bindings_input": {
                "path": str(path), "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}}
            self.assertEqual(reviewed_dictionary_bindings(config), [self.spec])
            path.write_text("{}")
            with self.assertRaisesRegex(ValueError, "digest"):
                reviewed_dictionary_bindings(config)

    def test_adjudication_graph_binds_reviewed_definition_list(self):
        from unittest.mock import patch
        import adjudication
        import retained_build
        from test_retained_build import Db, cut, facts
        tables = facts(cut(1))["tables"]
        command = retained_build.review_input(tables, {"graph": {"edges": []}}, "test")
        command["adjudication_specs"] = {"dictionary_bindings": [self.spec]}
        evidence = dict.fromkeys(bindings.COUNTS, 0)
        evidence["binding_sha256"] = bindings.lib.digest(self.spec)
        with patch.object(bindings, "measure", return_value=("incomplete", evidence)):
            result = adjudication.collect(Db(cut(1)), command, tables, "a" * 64)
        self.assertEqual(result["graph"]["dictionary_bindings_sha256"],
                         bindings.lib.digest([self.spec]))
        old_hash = result["summary_sha256"]
        result["graph"]["dictionary_bindings_sha256"] = "b" * 64
        self.assertNotEqual(adjudication.summary_hash(result), old_hash)
