#!/usr/bin/env python3
"""Scores an induced structure (`structure.json` of the corpus tests or the live run) against the frozen gold (D93).

    python3 -B tools/super-ontology/structure/score.py --run DIR [--gold FILE] [--out FILE]

Reports: class recall (a gold class is found by its kind and shape), file recall (members of each gold class), property precision and recall
(by concept slug), property type accuracy (text and long_text count as one), relation recall and precision (the target class kind),
structure-layer coverage (share of record files whose class is confirmed), and the noise files that wrongly joined a class.
Exit 1 when a hard gate fails: every gold class found, type accuracy >= 0.90, relation recall >= 0.80.
"""
import argparse, json, sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]


def fam(t):
    return "text" if t in ("text", "long_text") else t


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--run", required=True)
    ap.add_argument("--gold", default=str(ROOT / "testing/super-ontology/structure/gold/structure.json"))
    ap.add_argument("--out")
    a = ap.parse_args()
    gold = json.loads(Path(a.gold).read_text())
    run = json.loads((Path(a.run) / "structure.json").read_text())
    classes = {c["kind"]: c for c in run["classes"] if c["kind"] != "other"}
    key_of_kind = {k: c["class_key"] for k, c in classes.items()}
    rep = {"classes_found": 0, "classes_gold": len(gold["classes"]), "properties": [], "wrong": [], "missing": [], "extra": [], "relations": {"tp": 0, "fn": 0, "fp": 0}}
    typed_ok = typed_all = 0
    props_gold = props_found = 0
    for kind, g in sorted(gold["classes"].items()):
        c = classes.get(kind)
        if not c or c["shape"] != g["shape"]:
            rep["missing"].append(kind)
            continue
        rep["classes_found"] += 1
        got = {p["slug"]: p for p in c["properties"] if p["status"] != "rejected"}
        for slug, gp in sorted(g["properties"].items()):
            props_gold += 1
            p = got.get(slug)
            if p is None:
                rep["missing"].append(f"{kind}.{slug}")
                if gp.get("relation"):
                    rep["relations"]["fn"] += 1
                continue
            props_found += 1
            typed_all += 1
            ok = fam(p["value_type"]) == fam(gp["type"])
            typed_ok += ok
            if not ok:
                rep["wrong"].append(f"{kind}.{slug}: {p['value_type']} (gold {gp['type']})")
            rel = (p.get("relation") or {})
            want = gp.get("relation")
            got_t = rel.get("target") if rel.get("target_kind") == "class" else None
            if want:
                if got_t and got_t == key_of_kind.get(want, want):
                    rep["relations"]["tp"] += 1
                else:
                    rep["relations"]["fn"] += 1
                    rep["wrong"].append(f"{kind}.{slug}: relation {got_t} (gold {want})")
            elif got_t:
                rep["relations"]["fp"] += 1
                rep["wrong"].append(f"{kind}.{slug}: relation {got_t} not in gold")
        for slug in sorted(set(got) - set(g["properties"])):
            rep["extra"].append(f"{kind}.{slug}")
    n_props_found_total = sum(1 for k, c in classes.items() if k in gold["classes"] for p in c["properties"] if p["status"] != "rejected")
    r = rep["relations"]
    rep["type_accuracy"] = round(typed_ok / typed_all, 4) if typed_all else 0.0
    rep["property_recall"] = round(props_found / props_gold, 4) if props_gold else 0.0
    rep["property_precision"] = round(props_found / n_props_found_total, 4) if n_props_found_total else 0.0
    rep["relation_recall"] = round(r["tp"] / (r["tp"] + r["fn"]), 4) if (r["tp"] + r["fn"]) else 1.0
    rep["relation_precision"] = round(r["tp"] / (r["tp"] + r["fp"]), 4) if (r["tp"] + r["fp"]) else 1.0
    files_in = sum(c["files"] for k, c in classes.items() if k in gold["classes"])
    files_gold = sum(g["files"] for g in gold["classes"].values())
    rep["file_recall"] = round(min(files_in, files_gold) / files_gold, 4)
    confirmed = sum(c["files"] for c in run["classes"] if c["status"] == "confirmed")
    rep["coverage_confirmed"] = round(confirmed / gold["record_files"], 4)
    rep["coverage_induced"] = round(sum(c["files"] for c in run["classes"]) / gold["record_files"], 4)
    rep["unclustered_ok"] = sorted(run.get("unclustered", [])) == sorted(gold["unclustered"])
    ok = rep["classes_found"] == rep["classes_gold"] and rep["type_accuracy"] >= 0.90 and rep["relation_recall"] >= 0.80
    rep["pass"] = bool(ok)
    print(json.dumps(rep, ensure_ascii=False, indent=1))
    if a.out:
        Path(a.out).write_text(json.dumps(rep, ensure_ascii=False, indent=1) + "\n")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
