#!/usr/bin/env python3
"""Score one run of run_quality.py: false propositional entities (D90) and review items (D91).

False Decision: an active Decision entity whose name is not one of the corpus' true decisions (matched by its object phrase or by the
actor and a verb phrase of the truth sentence). False Event and Person: a name that is one of truth.json `generic_words`.
Output: <run>/quality.json.
"""
import argparse
import json
import re
import unicodedata
from collections import Counter
from pathlib import Path


def norm(s):
    return re.sub(r"\s+", " ", re.sub(r"[^\w\s]", " ", unicodedata.normalize("NFKC", s).lower())).strip()


def is_true_decision(name, truth):
    n = norm(name)
    if len(n) < 8:
        return False
    for d in truth["decisions"]:
        t = norm(d["action"])
        if n in t or t in n or (norm(d["actor"]) in n and any(w in n for w in t.split()[:3])):
            return True
    return False


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--corpus")
    ap.add_argument("--run")
    a = ap.parse_args()
    run = Path(a.run)
    truth = json.loads((Path(a.corpus) / "truth.json").read_text())
    generic = {norm(w) for w in truth["generic_words"]}
    ents = [json.loads(l) for l in (run / "entities-all.jsonl").read_text().splitlines() if l.strip()]
    ents = [e for e in ents if e["status"] == "active"]
    by_type = Counter(e["type"] for e in ents)
    dec = [e for e in ents if e["type"] == "Decision"]
    false_dec = [e for e in dec if not is_true_decision(e["name"], truth)]
    true_found = {d["file"] for d in truth["decisions"] for e in dec if is_true_decision(e["name"], {"decisions": [d]})}
    out = {
        "entities_by_type": dict(by_type),
        "decision_total": len(dec),
        "decision_false": len(false_dec),
        "decision_false_by_origin": dict(Counter("+".join(e["origins"]) or "none" for e in false_dec)),
        "decision_false_names": sorted(e["name"] for e in false_dec),
        "decision_true_found": len(true_found),
        "decision_true_total": len(truth["decisions"]),
        "generic_named": {t: sorted(e["name"] for e in ents if e["type"] == t and norm(e["name"]) in generic)
                          for t in ("Person", "Event", "Topic", "Term", "Organization", "Project", "Thing")},
    }
    rev = [json.loads(l) for l in (run / "review-items.jsonl").read_text().splitlines() if l.strip()]
    pend = [r for r in rev if r["status"] == "pending"]
    out["review_pending"] = len(pend)
    out["review_pending_by_kind_reason"] = dict(Counter(f"{r['kind']}/{r['reason']}" for r in pend))
    out["review_all_by_kind_reason"] = dict(Counter(f"{r['kind']}/{r['reason']}/{r['status']}" for r in rev))
    (run / "quality.json").write_text(json.dumps(out, ensure_ascii=False, indent=1))
    print(json.dumps({k: v for k, v in out.items() if k != "decision_false_names"}, ensure_ascii=False, indent=1))
    print("false decisions:", out["decision_false_names"][:40])


main()
