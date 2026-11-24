#!/usr/bin/env python3
"""Assemble the STaRK submission drafts from finished official runs.

The upload file for a split that was run is `results/<kb>-<split>/predictions.csv`
as it stands. For every `results/<kb>-test/predictions.csv` this writes
`<out>/<kb>-test-0.1.csv`, the official 10 % subset of `test` taken from the
same run (not a new run), and `<out>/scores.md` with the metrics of every
upload file. Nothing is uploaded.

Usage: python3 -I package.py --results DIR --qa-root DIR --out DIR
"""
import argparse
import csv
import json
from pathlib import Path


def metrics(pred, gold):
    pred = pred[:100]
    gold = set(gold)
    first = next((r for r, p in enumerate(pred, 1) if p in gold), None)
    return {"hit@1": float(bool(pred) and pred[0] in gold), "hit@5": float(any(p in gold for p in pred[:5])),
            "recall@20": len(gold & set(pred[:20])) / max(len(gold), 1), "mrr": 1.0 / first if first else 0.0}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--results", required=True)
    ap.add_argument("--qa-root", required=True)
    ap.add_argument("--out", required=True)
    a = ap.parse_args()
    res, out = Path(a.results), Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    table = []
    for run in sorted(res.iterdir()):
        if not (run / "predictions.csv").exists():
            continue
        kb, split = run.name.split("-", 1)
        m = json.loads((run / "metrics.json").read_text())
        table.append((kb, split, m["n"], m["hit@1"], m["hit@5"], m["recall@20"], m["mrr"]))
        if split != "test":
            continue
        ids = {int(x) for x in (Path(a.qa_root) / kb / "split" / "test-0.1.index").read_text().split()}
        gold = {}
        with open(Path(a.qa_root) / kb / "stark_qa" / "stark_qa.csv", newline="") as f:
            for r in csv.DictReader(f):
                gold[int(r["id"])] = json.loads(r["answer_ids"])
        rows, scores = [], []
        with open(run / "predictions.csv", newline="") as f:
            for r in csv.DictReader(f):
                qid = int(r["query_id"])
                if qid in ids:
                    rows.append(r)
                    scores.append(metrics(json.loads(r["pred_rank"]), gold[qid]))
        assert len(rows) == len(ids), f"{kb}: test-0.1 ids missing from the test run"
        with open(out / f"{kb}-test-0.1.csv", "w", newline="") as f:
            w = csv.DictWriter(f, fieldnames=["query_id", "pred_rank"])
            w.writeheader()
            w.writerows(rows)
        agg = {k: round(100 * sum(s[k] for s in scores) / len(scores), 2) for k in scores[0]}
        table.append((kb, "test-0.1 (from the test run)", len(rows), agg["hit@1"], agg["hit@5"], agg["recall@20"], agg["mrr"]))
    lines = ["| KB | split | n | Hit@1 | Hit@5 | R@20 | MRR |", "|---|---|---|---|---|---|---|"]
    lines += [f"| {kb} | {sp} | {n} | {h1} | {h5} | {r20} | {mrr} |" for kb, sp, n, h1, h5, r20, mrr in table]
    (out / "scores.md").write_text("\n".join(lines) + "\n")
    print("\n".join(lines))


if __name__ == "__main__":
    main()
