"""Score frozen adjudication artifacts, never treat absent evidence as a pass."""
import argparse
import hashlib
import json
from pathlib import Path

POSITIVE = {"relation": "adopted", "sensitive": "hidden", "metric": "certified",
            "entity_key": "adopted", "code_table": "complete"}


NEGATIVE = {"relation": "rejected", "sensitive": "visible", "metric": "demoted",
            "entity_key": "rejected", "code_table": "incomplete"}

def score(document, gold, baseline=None):
    predictions = {i["id"]: i for i in document["items"]}
    if len(predictions) != len(document["items"]):
        raise ValueError("d28_duplicate_prediction")
    labels = {i["id"]: i for i in gold["items"]}
    if len(labels) != len(gold["items"]) or any(
            type(i["positive"]) is not bool or i["class"] not in POSITIVE
            or not i["id"].startswith(i["class"] + "/") for i in labels.values()):
        raise ValueError("d28_gold_shape")
    if any(i["class"] not in POSITIVE or
           i["decision"] not in (POSITIVE[i["class"]], NEGATIVE[i["class"]])
           or not i["id"].startswith(i["class"] + "/") for i in predictions.values()):
        raise ValueError("d28_prediction_shape")
    results = {}
    for cls, decision in POSITIVE.items():
        counts = dict(tp=0, fp=0, fn=0, tn=0, missing=0)
        for identity, label in labels.items():
            if label["class"] != cls:
                continue
            predicted = predictions.get(identity)
            if predicted is None:
                counts["missing"] += 1
            elif predicted["class"] != cls:
                raise ValueError("d28_prediction_class")
            positive = predicted is not None and predicted["decision"] == decision
            counts[("t" if positive == label["positive"] else "f") + ("p" if positive else "n")] += 1
        counts["precision"] = counts["tp"] / (counts["tp"] + counts["fp"]) if counts["tp"] + counts["fp"] else None
        counts["recall"] = counts["tp"] / (counts["tp"] + counts["fn"]) if counts["tp"] + counts["fn"] else None
        counts["gold_count"] = sum(counts[k] for k in ("tp", "fp", "fn", "tn"))
        results[cls] = counts
    result = dict(classes=results, unscored_predictions=len(set(predictions) - set(labels)),
                  baseline_status="missing", relation_recall_nonregression=None)
    if baseline is not None:
        if (baseline.get("protocol") != "oo.d28-human-baseline/v1"
                or baseline.get("gold_sha256") != digest(gold)
                or not baseline.get("evidence_run_id")
                or baseline.get("review_mode") != "per_item"):
            raise ValueError("d28_baseline_binding")
        adopted = baseline.get("adopted_relations")
        if (not isinstance(adopted, list) or any(not isinstance(i, str) for i in adopted)
                or len(set(adopted)) != len(adopted)):
            raise ValueError("d28_baseline_inventory")
        accepted = set(adopted)
        relations = {k: v for k, v in labels.items() if v["class"] == "relation"}
        if not accepted <= set(relations):
            raise ValueError("d28_baseline_inventory")
        positives = sum(i["positive"] for i in relations.values())
        recall = sum(relations[k]["positive"] for k in accepted) / positives if positives else None
        result.update(baseline_status="bound", baseline_relation_recall=recall,
                      relation_recall_nonregression=recall is not None and
                      results["relation"]["recall"] is not None and results["relation"]["recall"] >= recall)
    result["status"] = ("passed" if result["relation_recall_nonregression"] is True and
                        not result["unscored_predictions"] and
                        all(c["gold_count"] and not c["missing"] for c in results.values()) else "incomplete")
    return result


def digest(value):
    return hashlib.sha256(json.dumps(value, ensure_ascii=False, sort_keys=True,
                                     separators=(",", ":")).encode()).hexdigest()


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args(argv)
    manifest = json.loads(args.manifest.read_text())
    results = {}
    for case in manifest["datasets"]:
        if case["name"] in results:
            raise ValueError("d28_duplicate_dataset")
        def read(key):
            ref = case[key]
            path = args.manifest.parent / ref["path"]
            raw = path.read_bytes()
            if hashlib.sha256(raw).hexdigest() != ref["sha256"]:
                raise ValueError("d28_artifact_digest")
            return json.loads(raw)
        document, gold = read("adjudication"), read("gold")
        results[case["name"]] = score(document, gold, read("baseline") if "baseline" in case else None)
    passed = set(results) == {"T6", "OO-Logistics"} and all(r["status"] == "passed" for r in results.values())
    output = dict(protocol="oo.d28-result/v1", datasets=results, status="passed" if passed else "incomplete",
                  manifest_sha256=hashlib.sha256(args.manifest.read_bytes()).hexdigest())
    args.output.write_text(json.dumps(output, indent=2) + "\n")
    print(json.dumps(output, sort_keys=True))
    return 0 if passed else 2


if __name__ == "__main__":
    raise SystemExit(main())
