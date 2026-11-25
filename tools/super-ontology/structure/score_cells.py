#!/usr/bin/env python3
"""Scores the slot filling of the table classes (D93, lane AT) against the frozen cell gold.

    python3 -B tools/super-ontology/structure/score_cells.py --instances DIR/instances-after.json --gold testing/super-ontology/<corpus>/gold/cells.json [--out FILE]

`instances-after.json` is what `structure_live` writes: per table class the instances the engine filled (one per row, with the cell range of the row,
the typed values and the relations). The gold (`gen_cells.py`) lists, per table file and sheet row, the cells the row should fill: a property is
matched by any of its labels (or its slug), a cell is right when its kind (`amount`, `date`, `ratio`, `number`, `text`) and canonical value are both
the gold's. Per property: precision = right cells / cells written for the property, recall = right cells / gold cells. Instances: a gold row is found
when an instance sits on its row of its file; it is named right when the name is the gold's. Relations: a gold (row, name) is found when the instance of
the row has a relation to an entity of that name; precision is over the relations written for gold rows.
"""
import argparse, json, sys
from collections import defaultdict
from pathlib import Path


def kind_of(value):
    for k in ("amount", "date", "ratio", "number"):
        if k in value:
            return k, str(value[k])
    for k in ("text", "enum"):
        if k in value:
            return "text", str(value[k])
    return None, None


def norm(s):
    return " ".join(str(s).casefold().split())


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--instances", required=True)
    ap.add_argument("--gold", required=True)
    ap.add_argument("--out")
    a = ap.parse_args()
    gold = json.loads(Path(a.gold).read_text())
    inst = json.loads(Path(a.instances).read_text())
    by_row = {}
    for cls in inst.values():
        for it in cls["items"]:
            by_row[(it["file"]["path"], it["row"])] = it
    per = defaultdict(lambda: {"gold": 0, "written": 0, "right": 0, "typed": 0})
    rows_gold = rows_found = rows_named = 0
    rel_gold = rel_found = rel_written = rel_right = 0
    wrong = []
    for t in gold["tables"]:
        labels = {k: {norm(x) for x in v} | {k} for k, v in t["properties"].items()}
        for r in t["rows"]:
            rows_gold += 1
            it = by_row.get((t["file"], r["row"]))
            if it is None:
                for k in r["cells"]:
                    per[k]["gold"] += 1
                rel_gold += len(r["relations"])
                continue
            rows_found += 1
            rows_named += norm(it["entity"]["name"]) == norm(r["name"])
            written = {}
            for c in it["cells"]:
                for k, ls in labels.items():
                    if norm(c.get("name") or "") in ls or c["slug"] == k:
                        written.setdefault(k, c)
            for k, g in r["cells"].items():
                p = per[k]
                p["gold"] += 1
                c = written.get(k)
                if c is None:
                    continue
                kd, v = kind_of(c["value"])
                p["written"] += 1
                p["typed"] += kd == g["kind"]
                if kd == g["kind"] and v == g["value"]:
                    p["right"] += 1
                else:
                    wrong.append({"file": t["file"], "row": r["row"], "property": k, "gold": g, "got": c["value"]})
            for k in written:
                if k not in r["cells"]:
                    per[k]["written"] += 1
            names = {norm(x["object"]["name"]) for x in it["relations"]}
            rel_gold += len(r["relations"])
            rel_written += len(names)
            for n in r["relations"]:
                if norm(n) in names:
                    rel_found += 1
                    rel_right += 1
    props = {}
    for k, p in sorted(per.items()):
        props[k] = {**p, "precision": round(p["right"] / p["written"], 3) if p["written"] else None, "recall": round(p["right"] / p["gold"], 3) if p["gold"] else None,
                    "type_accuracy": round(p["typed"] / p["written"], 3) if p["written"] else None}
    tg = sum(p["gold"] for p in per.values())
    tw = sum(p["written"] for p in per.values())
    tr = sum(p["right"] for p in per.values())
    rep = {"corpus": a.gold, "properties": props, "cells": {"gold": tg, "written": tw, "right": tr, "precision": round(tr / tw, 3) if tw else None, "recall": round(tr / tg, 3) if tg else None},
           "instances": {"gold_rows": rows_gold, "found": rows_found, "named_right": rows_named, "recall": round(rows_found / rows_gold, 3) if rows_gold else None},
           "relations": {"gold": rel_gold, "found": rel_found, "recall": round(rel_found / rel_gold, 3) if rel_gold else None},
           "wrong_cells": wrong[:80]}
    if a.out:
        Path(a.out).write_text(json.dumps(rep, ensure_ascii=False, indent=1))
    print(json.dumps({k: rep[k] for k in ("cells", "instances", "relations")}, ensure_ascii=False))
    for k, p in props.items():
        print(f"  {k:14s} gold {p['gold']:3d} written {p['written']:3d} right {p['right']:3d}  P {p['precision']}  R {p['recall']}  typed {p['type_accuracy']}")
    sys.exit(0)


if __name__ == "__main__":
    main()
