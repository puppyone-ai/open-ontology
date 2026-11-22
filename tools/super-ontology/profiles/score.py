#!/usr/bin/env python3
"""Scores the admission test set (D94, 30 section 4 admission gate) against its frozen gold.

    python3 -B tools/super-ontology/profiles/score.py --run DIR --gold testing/super-ontology/profiles/gold [--out score.json]

DIR holds what `profiles_live` wrote: files.jsonl ({"file","profile","class","reason","state"}), entities.jsonl ({"file","type","label","name"}),
relations.jsonl ({"file","subject","predicate","object"}) and brief.md (the BRIEF of the whole Drive). Reported:
  * class accuracy (and profile accuracy), with the files that were wrong and the reason the engine gave;
  * admission: entities that instruction, code and other files put into the ontology (0 required), what a reference file puts in besides Reference
    instances and its authors (0 required), and how many of the gold record entities the records produced;
  * the reference ontology: precision and recall of Reference and Person entities and of cites and authored relations;
  * the BRIEF: every instruction file of the gold is in the 工作约定 section, and the gold quotes are in it.
Exit status 1 when a hard gate fails (admission violations, BRIEF content).
"""
import argparse, json, re, sys, unicodedata
from pathlib import Path


def norm(s):
    s = unicodedata.normalize("NFKC", s).replace("`", "")
    return re.sub(r"\s+", " ", s).strip().lower().rstrip(".。;；:：")


def load(path):
    p = Path(path)
    return [json.loads(l) for l in p.read_text().splitlines() if l.strip()] if p.exists() else []


def prf(gold, pred):
    tp = len(gold & pred)
    p = tp / len(pred) if pred else 1.0
    r = tp / len(gold) if gold else 1.0
    return {"gold": len(gold), "pred": len(pred), "tp": tp, "precision": round(p, 3), "recall": round(r, 3)}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--run", required=True)
    ap.add_argument("--gold", required=True)
    ap.add_argument("--out")
    a = ap.parse_args()
    run, gold = Path(a.run), Path(a.gold)
    classes = {g["file"]: g for g in load(gold / "classes.jsonl")}
    files = {f["file"]: f for f in load(run / "files.jsonl")}
    ents = load(run / "entities.jsonl")
    rels = load(run / "relations.jsonl")
    res = {}

    # classification
    ok_c = ok_p = 0
    wrong = []
    per_class = {}
    for f, g in classes.items():
        got = files.get(f, {})
        c_ok = got.get("class") == g["class"]
        p_ok = g["profile"] is None or got.get("profile") == g["profile"]
        ok_c += c_ok
        ok_p += c_ok and p_ok
        d = per_class.setdefault(g["class"], [0, 0])
        d[1] += 1
        d[0] += c_ok
        if not c_ok:
            wrong.append({"file": f, "gold": g["class"], "got": got.get("class"), "profile": got.get("profile"), "reason": got.get("reason")})
    res["classification"] = {"files": len(classes), "class_accuracy": round(ok_c / len(classes), 3), "profile_accuracy": round(ok_p / len(classes), 3),
                              "by_class": {k: f"{c}/{n}" for k, (c, n) in per_class.items()}, "wrong": wrong}

    # admission
    def non_doc(file):
        # "own": the file put the entity in (evidence of the entity itself); the rest are links of the file to entities that exist (a project it is attached to).
        return [e for e in ents if e["file"] == file and e["type"] != "Document" and e.get("own", True)]
    linked = [{"file": e["file"], "entity": f'{e.get("label") or e["type"]}:{e["name"]}'} for e in ents
              if classes.get(e["file"], {}).get("class") in ("instruction", "code", "other") and e["type"] != "Document" and not e.get("own", True)]
    viol = []
    for f, g in classes.items():
        if g["class"] in ("instruction", "code", "other"):
            viol += [{"file": f, "entity": f'{e.get("label") or e["type"]}:{e["name"]}'} for e in non_doc(f)]
        elif g["class"] == "reference":
            viol += [{"file": f, "entity": f'{e.get("label") or e["type"]}:{e["name"]}'} for e in non_doc(f) if (e.get("label") or e["type"]) not in ("Reference", "Person")]
    rec = {(r["file"], norm(r["name"])) for r in load(gold / "records.jsonl")}
    got_rec = {(e["file"], norm(e["name"])) for e in ents}
    res["admission"] = {"entities_from_instruction_code_other_and_foreign_to_reference": len(viol), "violations": viol[:40], "linked_to_existing_entities": linked[:40],
                        "record_entities": {"gold": len(rec), "found": len(rec & got_rec), "recall": round(len(rec & got_rec) / len(rec), 3) if rec else None}}

    # reference ontology
    g_ent = {(r["file"], r["label"], norm(r["name"])) for r in load(gold / "reference.jsonl")}
    p_ent = {(e["file"], e.get("label") or e["type"], norm(e["name"])) for e in ents
             if classes.get(e["file"], {}).get("class") == "reference" and (e.get("label") or e["type"]) in ("Reference", "Person")}
    g_rel = {(r["file"], norm(r["subject"]), r["predicate"], norm(r["object"])) for r in load(gold / "reference-relations.jsonl")}
    p_rel = {(r["file"], norm(r["subject"]), r["predicate"], norm(r["object"])) for r in rels if r["predicate"] in ("cites", "authored")}
    res["reference_ontology"] = {"entities": prf(g_ent, p_ent), "relations": prf(g_rel, p_rel),
                                 "by_type": {t: prf({x for x in g_ent if x[1] == t}, {x for x in p_ent if x[1] == t}) for t in ("Reference", "Person")},
                                 "by_relation": {t: prf({x for x in g_rel if x[2] == t}, {x for x in p_rel if x[2] == t}) for t in ("cites", "authored")},
                                 "missed": sorted(map(list, (g_ent | g_rel) - (p_ent | p_rel)))[:20], "extra": sorted(map(list, (p_ent | p_rel) - (g_ent | g_rel)))[:20]}

    # BRIEF
    spec = json.loads((gold / "brief.json").read_text())
    brief = (run / "brief.md").read_text() if (run / "brief.md").exists() else ""
    sec = brief.split("## 工作约定", 1)[1].split("\n## ", 1)[0] if "## 工作约定" in brief else ""
    missing = [f for f in spec["must_include_files"] if f"### {f}" not in sec]
    quotes = [q for q in spec["must_quote"] if norm(q) not in norm(sec)]
    res["brief"] = {"has_section": bool(sec), "missing_files": missing, "missing_quotes": quotes, "lines_without_source": sum(1 for l in sec.splitlines() if l.startswith("- ") and "[src:" not in l)}

    hard = res["admission"]["entities_from_instruction_code_other_and_foreign_to_reference"] == 0 and not missing and not quotes and bool(sec)
    res["hard_gates_pass"] = hard
    print(json.dumps({k: v for k, v in res.items() if k != "classification"} | {"classification": {k: v for k, v in res["classification"].items() if k != "wrong"}}, ensure_ascii=False, indent=1))
    if res["classification"]["wrong"]:
        print("wrong classes:", json.dumps(res["classification"]["wrong"], ensure_ascii=False))
    if a.out:
        Path(a.out).write_text(json.dumps(res, ensure_ascii=False, indent=1))
    sys.exit(0 if hard else 1)


if __name__ == "__main__":
    main()
