"""Unit tests and the P2-T reproducible replay integration test."""

import copy
from decimal import Decimal
import json
import os
from pathlib import Path
import tempfile
import unittest

import run as ab


FIXTURE = ab.ROOT / "benchmarks/ab/fixtures/demo.json"


class ScorerTests(unittest.TestCase):
    def test_demo_amount_is_permission_not_accuracy(self):
        dataset = ab.load(FIXTURE)
        question = next(q for q in dataset["questions"] if q["id"] == "q02")
        self.assertEqual(question["track"], "permission")
        self.assertEqual(question["expected_kind"], "denied")
        self.assertEqual(question["gold"]["rows"][0][0]["coefficient"], "35175000")
        pack = ab.load(ab.pinned(FIXTURE.parent, dataset["inputs"]["pack"]))
        ab.validate_role(dataset, pack)

    def test_accuracy_role_visibility_is_validated_before_trials(self):
        dataset = ab.load(FIXTURE)
        pack = ab.load(ab.pinned(FIXTURE.parent, dataset["inputs"]["pack"]))
        ab.validate_role(dataset, pack)
        dataset["questions"][0]["required_columns"] = {"demo.customers": ["phone_synthetic"]}
        with self.assertRaisesRegex(ab.Invalid, "accuracy_not_answerable"):
            ab.validate_role(dataset, pack)
        dataset["questions"][0]["role"] = "ops"
        with self.assertRaisesRegex(ab.Invalid, "accuracy_role_contract"):
            ab.validate_dataset(dataset)

    def test_row_order_only_matters_when_explicitly_required(self):
        gold = ab.load(FIXTURE)["questions"][0]["gold"]
        gold["rows"].append([{"presence": "value", "value": "61"}])
        actual = copy.deepcopy(gold)
        actual["rows"].reverse()
        question = {"gold": gold}
        self.assertTrue(ab.compare_rows(question, actual)["pass"])
        question["order_required"] = True
        self.assertFalse(ab.compare_rows(question, actual)["pass"])
        actual["rows"] = actual["rows"][:1]
        question["order_required"] = False
        self.assertFalse(ab.compare_rows(question, actual)["pass"])

    def row(self, qid, left, right, track="accuracy", missing=False):
        return {"id": qid, "track": track,
                "c0": {"pass": left, "no_output": missing},
                "c3": {"pass": right, "no_output": False}}

    def test_fixed_broken_and_separate_security_tracks(self):
        rows = [self.row("fixed", False, True), self.row("broken", True, False),
                self.row("deny", False, True, "permission"),
                self.row("injection", True, True, "injection")]
        result = ab.paired(rows)
        self.assertEqual(result["accuracy"]["fixed"], ["fixed"])
        self.assertEqual(result["accuracy"]["broken"], ["broken"])
        self.assertEqual(result["permission"]["fixed"], ["deny"])
        self.assertEqual(result["injection"]["question_count"], 1)
        self.assertTrue(all(t["relative_change"] is None for t in result.values()))

    def test_threshold_exactly_five_percent_is_allowed(self):
        self.assertTrue(ab.health(1, 20)["passed"])
        self.assertFalse(ab.health(1, 19)["passed"])
        self.assertFalse(ab.health(0, 0)["passed"])
        with self.assertRaises(ab.Invalid):
            ab.health(21, 20)

    def test_one_arm_failure_cannot_be_hidden_by_pooling(self):
        rows = [self.row(str(i), i != 0, True, missing=i == 0) for i in range(19)]
        report = ab.paired(rows, recorded=False)["accuracy"]
        self.assertIsNone(report["relative_change"])
        self.assertEqual(report["headline_status"], "no_output_above_5_percent")
        self.assertEqual(report["question_count"], 19)

    def test_zero_baseline_and_empty_track(self):
        report = ab.paired([self.row("a", False, True)], recorded=False)
        self.assertEqual(report["accuracy"]["headline_status"], "zero_baseline")
        self.assertEqual(report["permission"]["headline_status"], "no_questions")

    def test_relative_change_not_absolute_accuracy(self):
        rows = [self.row("a", True, True), self.row("b", False, True)]
        self.assertEqual(ab.paired(rows, recorded=False)["accuracy"]["relative_change"],
                         {"numerator": 1, "denominator": 1})

    def test_prompt_has_no_gold_or_cross_arm_knowledge(self):
        dataset = ab.load(FIXTURE)
        q = copy.deepcopy(dataset["questions"][0])
        q["gold"] = "gold-canary"
        left = ab.prompt(q, "c0", {"t": ["id"]}, {"knowledge": "pack-canary"})
        right = ab.prompt(q, "c3", {"t": ["id"]}, {"knowledge": "pack-canary"})
        self.assertNotIn("gold-canary", ab.canonical(left) + ab.canonical(right))
        self.assertNotIn("pack-canary", ab.canonical(left))
        self.assertIn("pack-canary", ab.canonical(right))

    def test_duplicate_questions_and_noninteger_seed_rejected(self):
        dataset = ab.load(FIXTURE)
        dataset["questions"].append(dataset["questions"][0])
        with self.assertRaisesRegex(ab.Invalid, "question_ids"):
            ab.validate_dataset(dataset)
        dataset = ab.load(FIXTURE)
        dataset["seed"] = True
        with self.assertRaisesRegex(ab.Invalid, "model_seed"):
            ab.validate_dataset(dataset)

    def test_frozen_gold_is_exactly_the_shipped_demo_gold(self):
        dataset = ab.load(FIXTURE)
        gold = ab.load(ab.pinned(FIXTURE.parent, dataset["inputs"]["demo_gold"]))
        for q in dataset["questions"]:
            if q["track"] != "accuracy":
                continue
            expected = next(g for g in gold if g["id"] == q["source_id"])
            cell = q["gold"]["rows"][0][0]
            value = (Decimal(cell["coefficient"]).scaleb(-cell["scale"])
                     if "coefficient" in cell else Decimal(cell["value"]))
            self.assertEqual(value, Decimal(expected["expected"]["analyst"][0][0]))

    def test_refuses_duplicate_json_keys(self):
        with self.assertRaises(ab.Invalid):
            json.loads('{"id":1,"id":2}', object_pairs_hook=ab.object_pairs)


class PipelineTests(unittest.TestCase):
    def setUp(self):
        # All artifacts stay in this worktree, never in a system temp directory.
        self.tmp = tempfile.TemporaryDirectory(prefix="ab-test-", dir=ab.ROOT / "evidence")
        self.addCleanup(self.tmp.cleanup)
        self.base = Path(self.tmp.name)

    def modified_recordings(self, edit):
        dataset = ab.load(FIXTURE)
        recordings = ab.load(ab.pinned(FIXTURE.parent, dataset["inputs"]["responses"]))
        edit(recordings)
        ab.write(self.base / "responses.json", recordings)
        for ref in dataset["inputs"].values():
            ref["path"] = os.path.relpath((FIXTURE.parent / ref["path"]).resolve(), self.base)
        dataset["inputs"]["responses"] = {
            "path": "responses.json",
            "sha256": ab.digest((self.base / "responses.json").read_bytes()),
        }
        ab.write(self.base / "dataset.json", dataset)
        return self.base / "dataset.json"

    def test_p2_t_reproducible_replay(self):
        first = ab.run(FIXTURE, self.base / "first")
        second = ab.run(FIXTURE, self.base / "second")
        self.assertEqual(first, second)
        self.assertEqual((self.base / "first/report.json").read_bytes(),
                         (self.base / "second/report.json").read_bytes())
        self.assertEqual(first["tracks"]["accuracy"]["fixed"], [])
        self.assertEqual(first["tracks"]["permission"]["question_count"], 2)
        self.assertEqual(first["tracks"]["accuracy"]["broken"], ["q16"])
        self.assertFalse(first["model_measured"])
        self.assertTrue(all(t["relative_change"] is None for t in first["tracks"].values()))
        self.assertTrue((self.base / "first/c0-q01/execution/report.json").is_file())
        with self.assertRaises(FileExistsError):
            ab.run(FIXTURE, self.base / "first")

    def test_comparator_preserves_duplicate_rows_nulls_and_decimal_precision(self):
        expected = ab.load(FIXTURE)["questions"][1]["gold"]
        def compare(actual):
            return ab.invoke("result-comparator", {
                "version": "oo.benchmark.result-comparison/v1",
                "row_comparator": "unordered", "expected": expected, "actual": actual,
            })[1]
        self.assertEqual(compare(expected), 0)
        duplicate = copy.deepcopy(expected)
        duplicate["rows"] *= 2
        self.assertEqual(compare(duplicate), 1)
        null = copy.deepcopy(expected)
        null["rows"][0][0] = {"presence": "null"}
        self.assertEqual(compare(null), 1)
        changed = copy.deepcopy(expected)
        changed["rows"][0][0]["coefficient"] = "35175001"
        self.assertEqual(compare(changed), 1)

    def test_no_output_and_policy_denial_are_distinct(self):
        q = ab.load(FIXTURE)["questions"][-1]
        absent = ab.execute_recording(q, {"kind": "no_output"}, self.base / "absent", {})
        denied = ab.execute_recording(q, {"kind": "denied", "code": "POLICY_DENIED"},
                                     self.base / "denied", {})
        failure = ab.execute_recording(q, {"kind": "denied", "code": "ERROR"},
                                      self.base / "error", {})
        self.assertEqual(absent, {"pass": False, "no_output": True, "reason": "no_output"})
        self.assertTrue(denied["pass"])
        self.assertFalse(denied["no_output"])
        self.assertFalse(failure["pass"])

    def test_truncated_matching_prefix_is_not_a_pass(self):
        dataset = ab.load(FIXTURE)
        response = ab.load(ab.pinned(FIXTURE.parent, dataset["inputs"]["responses"]))[
            "responses"]["c0:q01"]
        response["transcript"][-1]["truncated"] = True
        frozen = dict(project="public-ab", snapshot="fixture", as_of=dataset["as_of"],
                      dataset=dataset["id"], suite="ab", role="analyst", budget="one")
        result = ab.execute_recording(dataset["questions"][0], response,
                                      self.base / "truncated", frozen)
        self.assertFalse(result["pass"])
        self.assertEqual(result["reason"], "truncated")

    def test_hash_drift_is_rejected(self):
        ref = dict(ab.load(FIXTURE)["inputs"]["pack"], sha256="0" * 64)
        with self.assertRaisesRegex(ab.Invalid, "binding_drift"):
            ab.pinned(FIXTURE.parent, ref)

    def test_missing_response_stays_in_denominator(self):
        path = self.modified_recordings(lambda r: r["responses"].pop("c0:q01"))
        report = ab.run(path, self.base / "missing")
        track = report["tracks"]["accuracy"]
        self.assertEqual(track["question_count"], 2)
        self.assertEqual(track["health"]["c0"]["no_output"], 1)
        self.assertFalse(track["health"]["c0"]["passed"])
        self.assertIsNone(track["relative_change"])

    def test_prompt_drift_is_rejected_before_execution(self):
        path = self.modified_recordings(
            lambda r: r["responses"]["c0:q01"].update(prompt_sha256="0" * 64))
        with self.assertRaisesRegex(ab.Invalid, "prompt_binding"):
            ab.run(path, self.base / "drift")
        self.assertFalse((self.base / "drift").exists())

    def test_model_identity_mismatch_is_rejected(self):
        path = self.modified_recordings(lambda r: r.update(model="other-model"))
        with self.assertRaisesRegex(ab.Invalid, "recording_binding"):
            ab.run(path, self.base / "model")


if __name__ == "__main__":
    unittest.main()
