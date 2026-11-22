#!/usr/bin/env python3
"""Scores the entities the engine found against the frozen gold of the profile test set.

    python3 -B tools/super-ontology/profiles/score.py --pred entities.jsonl --gold testing/super-ontology/profiles/gold [--out score.json]

--pred: JSON lines {"file", "type", "label", "name"} (one per entity and file that holds it; label is the Thing label or null). The gold files
are instruction.jsonl, code.jsonl and paper.jsonl ({"file","label","name"}). A prediction counts only when its label is one the profile extractors own
(Thing labels, plus Person for papers): other entities (the core rules of 10 section 5.1) are not this measurement. Names are compared after
normalisation (case, backticks, trailing punctuation, spaces, a leading ./ and a trailing / of a path). Precision and recall per profile and label.
"""
import argparse, json, re, unicodedata
from pathlib import Path

LABELS = {"instruction": ["Rule", "Command", "Tool", "Path"], "code": ["Function", "Class", "Type", "Dependency", "Package"], "paper": ["Paper", "Person", "Venue", "Method", "Dataset", "Metric"]}


def norm(label, name):
    s = unicodedata.normalize("NFKC", name).replace("`", "")
    s = re.sub(r"\s+", " ", s).strip().lower()
    if label == "Path":
        s = s.removeprefix("./").rstrip("/")
    return s.rstrip(".。;；:：")


def load(path):
    return [json.loads(l) for l in Path(path).read_text().splitlines() if l.strip()]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--pred", required=True)
    ap.add_argument("--gold", required=True)
    ap.add_argument("--out")
    a = ap.parse_args()
    pred = load(a.pred)
    result, detail = {}, {}
    for profile, labels in LABELS.items():
        gold = load(Path(a.gold) / f"{profile}.jsonl")
        files = {g["file"] for g in gold}
        g_set = {(g["file"], g["label"], norm(g["label"], g["name"])) for g in gold if g["label"] in labels}
        # the files this profile is measured on, and the labels it owns (Person only counts on paper files)
        p_set = set()
        for p in pred:
            if p["file"] not in files:
                continue
            label = p.get("label") or p["type"]
            if label not in labels:
                continue
            p_set.add((p["file"], label, norm(label, p["name"])))
        tp = g_set & p_set
        per = {}
        for label in labels:
            g = {x for x in g_set if x[1] == label}
            pr = {x for x in p_set if x[1] == label}
            t = g & pr
            per[label] = {"gold": len(g), "pred": len(pr), "tp": len(t), "precision": round(len(t) / len(pr), 3) if pr else None, "recall": round(len(t) / len(g), 3) if g else None}
        p_all = len(tp) / len(p_set) if p_set else 0
        r_all = len(tp) / len(g_set) if g_set else 0
        f1 = 2 * p_all * r_all / (p_all + r_all) if p_all + r_all else 0
        result[profile] = {"gold": len(g_set), "pred": len(p_set), "tp": len(tp), "precision": round(p_all, 3), "recall": round(r_all, 3), "f1": round(f1, 3), "per_label": per}
        detail[profile] = {"missed": sorted(g_set - p_set), "extra": sorted(p_set - g_set)}
    for profile, r in result.items():
        print(f"{profile:12s} gold {r['gold']:3d} pred {r['pred']:3d} P {r['precision']:.3f} R {r['recall']:.3f} F1 {r['f1']:.3f}")
        for label, m in r["per_label"].items():
            print(f"    {label:11s} gold {m['gold']:3d} pred {m['pred']:3d} tp {m['tp']:3d} P {m['precision']} R {m['recall']}")
    if a.out:
        Path(a.out).write_text(json.dumps({"summary": result, "detail": detail}, ensure_ascii=False, indent=1))
    return detail


if __name__ == "__main__":
    d = main()
    import sys
    if "--verbose" in sys.argv:
        print(json.dumps(d, ensure_ascii=False, indent=1))
