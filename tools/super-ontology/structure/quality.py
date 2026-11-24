#!/usr/bin/env python3
"""The ontology-quality gate of 30 section 4 (D95, D96) on the output of an L4 run.

    python3 -B tools/super-ontology/structure/quality.py --predictions DIR --golden DIR [--out FILE]

Reads `<predictions>/entities.jsonl` (what `eval_live` exports) and `<golden>/labels/entities.jsonl`.
  * entity precision: the share of predicted entities that name a gold entity (by name or alias, NFKC, case folded), whatever the type;
  * type precision: of the predicted entities that name a gold entity, the share that carry the gold type;
  * single-word Decisions: predicted Decision entities whose name is one word (no blank, at most 4 characters; or one Latin word). Gate: 0;
  * Decision names shorter than 8 characters, for the eyes of the person reading the report.
Gates: entity precision >= 0.90, type precision >= 0.90, 0 single-word Decisions. Exit 1 when one fails. Thresholds are never changed to pass.
"""
import argparse, json, sys, unicodedata
from pathlib import Path


def norm(s):
    return " ".join(unicodedata.normalize("NFKC", s).lower().split())


def load(path):
    return [json.loads(l) for l in Path(path).read_text().splitlines() if l.strip()]


def single_word(name):
    n = name.strip()
    if not n:
        return True
    if any(c.isspace() for c in n):
        return False
    latin = all(ord(c) < 128 for c in n)
    return latin or len(n) <= 4


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--predictions", required=True)
    ap.add_argument("--golden", required=True)
    ap.add_argument("--out")
    a = ap.parse_args()
    pred = [e for e in load(Path(a.predictions) / "entities.jsonl") if e["type"] != "Document"]
    gold = load(Path(a.golden) / "labels" / "entities.jsonl")
    by_name = {}
    for g in gold:
        for n in [g["name"], *g.get("aliases", [])]:
            by_name.setdefault(norm(n), set()).add(g["type"])
    matched = [e for e in pred if any(norm(n) in by_name for n in [e["name"], *e.get("aliases", [])])]
    typed = [e for e in matched if e["type"] in by_name.get(norm(e["name"]), set()) or any(e["type"] in by_name.get(norm(n), set()) for n in e.get("aliases", []))]
    decisions = [e for e in pred if e["type"] == "Decision"]
    one = [e["name"] for e in decisions if single_word(e["name"])]
    rep = {
        "predicted_entities": len(pred), "matching_a_gold_entity": len(matched),
        "entity_precision": round(len(matched) / len(pred), 4) if pred else 1.0,
        "type_precision": round(len(typed) / len(matched), 4) if matched else 1.0,
        "decisions": len(decisions), "single_word_decisions": len(one), "single_word_decision_names": one,
        "short_decisions_under_8": sum(1 for e in decisions if len(e["name"].strip()) < 8),
    }
    rep["pass"] = rep["entity_precision"] >= 0.90 and rep["type_precision"] >= 0.90 and rep["single_word_decisions"] == 0
    print(json.dumps(rep, ensure_ascii=False, indent=1))
    if a.out:
        Path(a.out).write_text(json.dumps(rep, ensure_ascii=False, indent=1) + "\n")
    sys.exit(0 if rep["pass"] else 1)


if __name__ == "__main__":
    main()
