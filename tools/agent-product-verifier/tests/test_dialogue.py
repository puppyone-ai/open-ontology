import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("dialogue", Path(__file__).parents[1] / "dialogue.py")
dialogue = importlib.util.module_from_spec(spec)
spec.loader.exec_module(dialogue)


class DialogueEvidence(unittest.TestCase):
    def test_denied_query_retains_error(self):
        body = {"class": "gateway", "exit": 2, "code": "FUNCTION_NOT_ALLOWED", "message": "refused", "query_id": "denied"}
        self.assertEqual(dialogue.model_view("query", body), body)

    def test_named_fact_requires_matching_successful_cell(self):
        body = {"columns": [{"name": "updated_count"}], "rows": [["1"]]}
        good = {"facts": {"updated_count": "1"}, "evidence": {"updated_count": {"query_id": "success", "column": "updated_count"}}}
        dialogue.verify_named_facts(json.dumps(good), {"updated_count": 1}, {"success": body})
        for value in ("0", True, 1.0):
            bad = {**good, "facts": {"updated_count": value}}
            with self.assertRaises(ValueError):
                dialogue.verify_named_facts(json.dumps(bad), {"updated_count": 1}, {"success": body})
        bad = {**good, "evidence": {"updated_count": {"query_id": "denied", "column": "updated_count"}}}
        with self.assertRaises(KeyError):
            dialogue.verify_named_facts(json.dumps(bad), {"updated_count": 1}, {"success": body})
        with self.assertRaises(ValueError):
            dialogue.verify_named_facts(json.dumps({**good, "answer": "update count is zero"}), {"updated_count": 1}, {"success": body})

    def test_rich_value_and_wrong_evidence_shape(self):
        result = {"columns": [{"name": "row_count"}], "rows": [["14"]]}
        report = {"facts": {"row_count": {"value": "14", "result_type": "int8"}}, "evidence": {"row_count": {"query_id": "real", "column": "row_count"}}}
        dialogue.verify_named_facts(json.dumps(report), {"row_count": 14}, {"real": result})
        with self.assertRaises(ValueError):
            dialogue.verify_named_facts(json.dumps({**report, "evidence": []}), {"row_count": 14}, {"real": result})


IDENTITY = {
    "source_id": "clickhouse_connection_whole",
    "source_generation": 11,
    "pack_sha256": "ab" * 32,
    "role": "oo_r_reader",
}


def bank():
    return [
        {"semantic_id": "filter.qty", "query_class": "filter", "prompt": "current qty for reading 105", "truth_key": "qty"},
        {"semantic_id": "aggregate.sum", "query_class": "aggregate", "prompt": "sum of current qty", "truth_key": "total"},
        {"semantic_id": "detail.row", "query_class": "detail", "prompt": "reading whose qty is 4", "truth_key": "row"},
        {"semantic_id": "filter.positive", "query_class": "filter", "prompt": "rows whose qty is above zero", "truth_key": "positive"},
    ]


def gold():
    return {
        "qty": {"expected_named_facts": {"qty": "5"}},
        "total": {"expected_named_facts": {"sum_qty": "17"}},
        "row": {"expected_named_facts": {"reading_id": "104"}},
        "positive": {"expected_named_facts": {"positive_rows": "3"}},
    }


class SelectionFreeze(unittest.TestCase):
    def test_fixed_questions_stay_legacy(self):
        questions = [{"prompt": "fixed", "truth_key": "qty"}]
        manifest, chosen = dialogue.freeze_selection({"questions": questions}, gold(), b"{}")
        self.assertEqual(manifest["mode"], "legacy")
        self.assertFalse(manifest["counts_as_random"])
        self.assertEqual(chosen, questions)
        with self.assertRaisesRegex(ValueError, "legacy"):
            dialogue.freeze_selection({"questions": questions, "seed": 1}, gold(), b"{}")

    def test_same_seed_is_replay_and_omits_gold_values(self):
        cfg = {"question_bank": bank(), "seed": 7, "identity": IDENTITY}
        first, one = dialogue.freeze_selection(cfg, gold(), b'{"qty":1}')
        second, two = dialogue.freeze_selection(cfg, gold(), b'{"qty":1}')
        self.assertEqual(first["semantic_ids"], second["semantic_ids"])
        self.assertEqual(one, two)
        self.assertEqual(first["mode"], "replay")
        self.assertFalse(first["counts_as_random"])
        self.assertNotIn("expected_named_facts", json.dumps(first))
        self.assertGreaterEqual(len(set(item["query_class"] for item in first["selected"])), 2)

    def test_omitted_seed_counts_as_random(self):
        manifest, _ = dialogue.freeze_selection({"question_bank": bank(), "identity": IDENTITY}, gold(), b"{}")
        self.assertEqual(manifest["mode"], "random")
        self.assertTrue(manifest["counts_as_random"])
        self.assertIs(type(manifest["seed"]), int)

    def test_history_skips_a_used_sequence_and_replay_does_not(self):
        picked = dialogue.draw(bank(), gold(), 1, 2, [])
        used = [tuple(item["semantic_id"] for item in picked)]
        again = dialogue.draw(bank(), gold(), 1, 2, used)
        self.assertNotEqual(used[0], tuple(item["semantic_id"] for item in again))
        replay, _ = dialogue.freeze_selection(
            {"question_bank": bank(), "seed": 1, "identity": IDENTITY, "history": [list(used[0])]},
            gold(), b"{}")
        self.assertEqual(replay["mode"], "replay")
        self.assertEqual(replay["semantic_ids"], list(used[0]))
        prior = {"filter.qty", "aggregate.sum"}
        blocked = dialogue.draw(bank(), gold(), 1, 2, [], prior)
        self.assertTrue({item["semantic_id"] for item in blocked}.isdisjoint(prior))
        with self.assertRaisesRegex(ValueError, "exhausted"):
            dialogue.draw(bank(), gold(), 1, 2, [], {item["semantic_id"] for item in bank()})

    def test_exhausted_pool_is_explicit(self):
        seen = []
        for seed in range(20):
            try:
                picked = dialogue.draw(bank(), gold(), seed, 2, seen)
            except ValueError as exc:
                self.assertIn("exhausted", str(exc))
                break
            seen.append(tuple(item["semantic_id"] for item in picked))
        else:
            self.fail("pool did not exhaust")
        self.assertGreaterEqual(len(seen), 2)
        with self.assertRaisesRegex(ValueError, "exhausted"):
            dialogue.draw(bank(), gold(), 99, 2, seen)

    def test_rejects_bad_bank_before_draw(self):
        body = gold()
        cases = [
            ([{**bank()[0], "semantic_id": "Bad"}, bank()[1]], "illegal semantic id"),
            ([bank()[0], {**bank()[1], "semantic_id": "filter.qty"}], "duplicate semantic id"),
            ([bank()[0], {**bank()[1], "truth_key": "missing"}], "missing gold"),
            (bank()[:1], "no semantic difference"),
            ([
                {"semantic_id": "filter.a", "query_class": "filter", "prompt": "how many", "truth_key": "qty"},
                {"semantic_id": "aggregate.a", "query_class": "aggregate", "prompt": "what is the number", "truth_key": "qty"},
            ], "no semantic difference"),
        ]
        for items, message in cases:
            with self.assertRaisesRegex(ValueError, message):
                dialogue.freeze_selection({"question_bank": items, "identity": IDENTITY}, body, b"{}")
        with self.assertRaisesRegex(ValueError, "missing"):
            dialogue.freeze_selection({"question_bank": [], "identity": IDENTITY}, body, b"{}")
        wide, keys = [], ("qty", "total", "row", "positive")
        for i in range(9):
            wide.append({
                "semantic_id": f"item.n{i}",
                "query_class": "filter" if i % 2 == 0 else "aggregate",
                "prompt": f"prompt {i}",
                "truth_key": keys[i % 4],
            })
        with self.assertRaisesRegex(ValueError, "too large"):
            dialogue.freeze_selection({"question_bank": wide, "identity": IDENTITY}, body, b"{}")

    def test_resume_keeps_the_frozen_sequence(self):
        raw = b'{"frozen":true}\n'
        manifest, _ = dialogue.freeze_selection({"question_bank": bank(), "seed": 3, "identity": IDENTITY}, gold(), raw)
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        frozen = Path(directory.name) / "selection.json"
        frozen.write_text(json.dumps(manifest))
        def fail_draw(*_args, **_kwargs):
            raise AssertionError("resume redrew the sequence")
        original = dialogue.draw
        dialogue.draw = fail_draw
        try:
            resumed, questions = dialogue.freeze_selection(
                {"question_bank": bank(), "seed": 3, "identity": IDENTITY, "frozen_manifest": str(frozen), "resume_transcript": "x"},
                gold(), raw)
        finally:
            dialogue.draw = original
        self.assertEqual(resumed["semantic_ids"], manifest["semantic_ids"])
        self.assertEqual([item["truth_key"] for item in questions], [item["truth_key"] for item in manifest["questions"]])
        with self.assertRaisesRegex(ValueError, "truth digest"):
            dialogue.freeze_selection(
                {"question_bank": bank(), "identity": IDENTITY, "frozen_manifest": str(frozen)},
                gold(), b"other")
        changed = dict(IDENTITY, role="other")
        with self.assertRaisesRegex(ValueError, "identity"):
            dialogue.freeze_selection(
                {"question_bank": bank(), "identity": changed, "frozen_manifest": str(frozen)},
                gold(), raw)
        with self.assertRaisesRegex(ValueError, "seed"):
            dialogue.freeze_selection(
                {"question_bank": bank(), "seed": 4, "identity": IDENTITY, "frozen_manifest": str(frozen)},
                gold(), raw)
        edited = [dict(bank()[0], prompt="different words"), *bank()[1:]]
        with self.assertRaisesRegex(ValueError, "question mismatch"):
            dialogue.freeze_selection(
                {"question_bank": edited, "identity": IDENTITY, "frozen_manifest": str(frozen)},
                gold(), raw)

    def test_empty_pack_search_tells_the_model_to_narrow(self):
        view = dialogue.model_view("knowledge_search", {"hits": [], "knowledge_version": "1"})
        self.assertIn("short word", view["hint"])
        self.assertNotIn("hint", dialogue.model_view("knowledge_search", {"hits": [{"path": "INDEX.md"}]}))

    def test_unknown_knowledge_path_returns_to_the_model(self):
        body = {"class": "gateway", "code": "SCHEMA_INVALID", "message": "knowledge path is not in this pack"}
        self.assertTrue(dialogue.tool_error_returned("knowledge_cat", body))
        self.assertFalse(dialogue.tool_error_returned("query", body))
        self.assertFalse(dialogue.tool_error_returned("knowledge_cat", {"class": "gateway", "code": "UNAUTHENTICATED", "message": "authentication failed"}))

    def test_empty_search_returns_the_cli_usage_and_facts_stay_strict(self):
        body = {"class": "usage", "message": "search text is missing", "retryable": False}
        self.assertTrue(dialogue.tool_error_returned("knowledge_search", body))
        self.assertFalse(dialogue.tool_error_returned("knowledge_search", {"class": "usage", "message": "other"}))
        self.assertFalse(dialogue.tool_error_returned("query", body))
        search = next(item["function"] for item in dialogue.tool_schema() if item["function"]["name"] == "knowledge_search")
        self.assertEqual(search["parameters"]["properties"]["query"]["minLength"], 1)
        self.assertIn("non-empty", search["description"])
        with self.assertRaisesRegex(ValueError, "only facts and evidence"):
            dialogue.verify_named_facts(json.dumps({"evidence": {}, "payload": {}}), {"qty": "1"}, {})

    def test_coverage_nudge_stops_after_the_tools_exist(self):
        self.assertEqual(dialogue.coverage_nudge(set(), 0, 4), ["describe", "freshness", "knowledge_cat", "query"])
        self.assertEqual(dialogue.coverage_nudge({"query", "freshness", "describe", "knowledge_cat"}, 1, 4), [])
        self.assertEqual(dialogue.coverage_nudge(set(), 3, 4), [])
