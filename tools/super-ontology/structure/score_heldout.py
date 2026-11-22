#!/usr/bin/env python3
"""Scores the held-out structure corpus (gold in testing/super-ontology/structure-heldout/gold).

    python3 -B score_heldout.py --run DIR/structure.json [--out FILE]

Class: the induced class holding most of the gold class's files, with purity (share of its members from that gold class) >= 0.8 and recall >= 0.8.
Property: matched by any gold label against the property's name or keys. Type: text and long_text are one type. Relation: the property's class target is the class
found for the gold prefix. Coverage: members of confirmed classes / record files. Thresholds fixed in advance: class found 5/5, type >= 0.90, relation recall >= 0.80.
"""
import argparse, json, sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
fam = lambda t: "text" if t in ("text", "long_text") else t


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--run", required=True)
    ap.add_argument("--gold", default=str(ROOT / "testing/super-ontology/structure-heldout/gold/structure.json"))
    ap.add_argument("--out")
    a = ap.parse_args()
    g = json.loads(Path(a.gold).read_text())
    r = json.loads(Path(a.run).read_text())
    members = r.get("members", {})
    classes = {c["class_key"]: c for c in r["classes"]}
    rep = {"classes": {}, "wrong": [], "missing": []}
    found_key = {}
    for name, gc in g["classes"].items():
        pre = gc["prefix"]
        best = None
        for k, paths in members.items():
            inside = sum(p.startswith(pre) for p in paths)
            if inside and (best is None or inside > best[1]):
                best = (k, inside, len(paths))
        total = sum(1 for k, ps in members.items() for p in ps if p.startswith(pre))
        n_gold = sum(1 for p in {q for ps in members.values() for q in ps} if p.startswith(pre))
        if best:
            k, inside, size = best
            gold_files = gc.get("files") or None
            rep["classes"][name] = {"class": k, "purity": round(inside / size, 3), "recall_of_members": inside}
            if inside / size >= 0.8:
                found_key[name] = k
    ty_ok = ty_all = rel_tp = rel_fn = rel_fp = prop_found = prop_gold = 0
    for name, gc in g["classes"].items():
        k = found_key.get(name)
        if not k:
            rep["missing"].append(name)
            for pn, gp in gc["properties"].items():
                prop_gold += 1
                if gp.get("relation"):
                    rel_fn += 1
            continue
        c = classes[k]
        for pn, gp in gc["properties"].items():
            prop_gold += 1
            labels = set(gp["labels"])
            p = next((p for p in c["properties"] if p["name"] in labels or set(p["keys"]) & {l.lower() for l in labels} or p["name"].lower() in {l.lower() for l in labels}), None)
            if not p:
                rep["missing"].append(f"{name}.{pn}")
                if gp.get("relation"):
                    rel_fn += 1
                continue
            prop_found += 1
            ty_all += 1
            ok = fam(p["value_type"]) == fam(gp["type"])
            ty_ok += ok
            if not ok:
                rep["wrong"].append(f"{name}.{pn}: {p['value_type']} (gold {gp['type']})")
            if gp.get("relation"):
                want = found_key.get(next((n for n, x in g["classes"].items() if x["prefix"] == gp["relation"]), ""))
                rel = p.get("relation") or {}
                if rel.get("target_kind") == "class" and rel.get("target") == want:
                    rel_tp += 1
                else:
                    rel_fn += 1
                    rep["wrong"].append(f"{name}.{pn}: relation {rel.get('target')} (gold {want})")
            elif (p.get("relation") or {}).get("target_kind") == "class":
                rel_fp += 1
    conf = sum(len(ps) for k, ps in members.items() if classes.get(k, {}).get("status") == "confirmed")
    rep.update({"classes_found": len(found_key), "classes_gold": len(g["classes"]), "property_recall": round(prop_found / prop_gold, 3),
                "type_accuracy": round(ty_ok / ty_all, 3) if ty_all else 0, "relation_recall": round(rel_tp / (rel_tp + rel_fn), 3) if rel_tp + rel_fn else 1,
                "relation_precision": round(rel_tp / (rel_tp + rel_fp), 3) if rel_tp + rel_fp else 1,
                "coverage_confirmed": round(conf / g["record_files"], 3), "coverage_in_class": round(sum(len(p) for p in members.values()) / g["record_files"], 3)})
    rep["pass"] = rep["classes_found"] == rep["classes_gold"] and rep["type_accuracy"] >= 0.9 and rep["relation_recall"] >= 0.8
    print(json.dumps(rep, ensure_ascii=False, indent=1))
    if a.out:
        Path(a.out).write_text(json.dumps(rep, ensure_ascii=False, indent=1) + "\n")
    sys.exit(0 if rep["pass"] else 1)


if __name__ == "__main__":
    main()
