import unittest
import d28


class D28(unittest.TestCase):
    def fixture(self):
        gold = {"items":[{"id":c+"/positive", "class":c, "positive":True} for c in d28.POSITIVE]}
        document = {"items":[{"id":c+"/positive", "class":c, "decision":v}
                              for c, v in d28.POSITIVE.items()]}
        baseline = dict(protocol="oo.d28-human-baseline/v1", gold_sha256=d28.digest(gold),
                        evidence_run_id="test-only-baseline", review_mode="per_item",
                        adopted_relations=["relation/positive"])
        return document, gold, baseline

    def test_precision_recall_and_baseline_are_separate(self):
        document, gold, baseline = self.fixture()
        self.assertEqual(d28.score(document, gold)["status"], "incomplete")
        self.assertEqual(d28.score(document, gold, baseline)["status"], "passed")
        document["items"][0]["decision"] = "rejected"
        result = d28.score(document, gold, baseline)
        self.assertFalse(result["relation_recall_nonregression"])
        self.assertEqual(result["classes"]["relation"]["recall"], 0)

    def test_missing_prediction_is_false_negative_not_dropped(self):
        document, gold, baseline = self.fixture()
        document["items"].pop(0)
        result = d28.score(document, gold, baseline)
        self.assertEqual(result["classes"]["relation"]["fn"], 1)
        self.assertEqual(result["classes"]["relation"]["missing"], 1)

    def test_unbound_baseline_is_refused(self):
        document, gold, baseline = self.fixture()
        baseline["gold_sha256"] = "0" * 64
        with self.assertRaisesRegex(ValueError, "d28_baseline_binding"):
            d28.score(document, gold, baseline)

    def test_empty_gold_is_not_perfect_recall(self):
        result = d28.score({"items":[]}, {"items":[]})
        self.assertIsNone(result["classes"]["relation"]["recall"])
        self.assertEqual(result["status"], "incomplete")


    def test_duplicate_or_unknown_baseline_relations_are_refused(self):
        document, gold, baseline = self.fixture()
        for adopted in (["relation/positive"] * 2, ["relation/unknown"], "relation/positive"):
            with self.subTest(adopted=adopted), self.assertRaisesRegex(
                    ValueError, "d28_baseline_inventory"):
                d28.score(document, gold, dict(baseline, adopted_relations=adopted))

    def test_invalid_decision_or_gold_class_cannot_silently_score_as_negative(self):
        document, gold, baseline = self.fixture()
        document["items"][0]["decision"] = "typo"
        with self.assertRaisesRegex(ValueError, "d28_prediction_shape"):
            d28.score(document, gold, baseline)
        gold["items"][0]["class"] = "typo"
        with self.assertRaisesRegex(ValueError, "d28_gold_shape"):
            d28.score(document, gold)

    def test_unscored_prediction_prevents_acceptance(self):
        document, gold, baseline = self.fixture()
        document["items"].append(dict(id="relation/extra", **{
            "class": "relation", "decision": "adopted"}))
        result = d28.score(document, gold, baseline)
        self.assertEqual(result["unscored_predictions"], 1)
        self.assertEqual(result["status"], "incomplete")

if __name__ == "__main__":
    unittest.main()
