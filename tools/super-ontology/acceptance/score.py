#!/usr/bin/env python3
"""Scores (1) cross-file relations and (2) the unlinked files of an acceptance run.

    python3 -B score.py --state-dir <engine state dir> --token-file <read token> --run packaged-run.json \
        [--unlinked-ui unlinked-ui.json] --gold testing/super-ontology/acceptance/gold --out results/

The ontology is read through the engine API with a read-only Agent token (docs:read), the way an Agent would: GET /v1/docs/entities and
/v1/docs/entities/{key}. `--state-dir` only supplies the engine address (serve.json); the token is a separate file. Nothing is written to the engine.

(1) Relations. A gold relation (names + the files that must carry the same thing) is FOUND when one entity (not a Document) whose name or alias
matches a gold name cites at least 2 of the gold files and at least 60 percent of them. Coverage is the share of the gold files that entity cites
(the best entity per relation). Recall = found / gold. Precision is measured on what the ontology shows: every (entity, file) link of a non-Document
entity that cites 2 or more files is SUPPORTED when one of the entity's names or aliases really occurs in that file's text (the generator's own
source text of the file, so the check does not depend on the gold); precision = supported / shown. Links from a project entity into an unrelated
file are listed separately as leaks.

(2) Unlinked. For each file of gold/unlinked.jsonl the reason the app shows (UI scrape, run_packaged.mjs --unlinked) or, without it, the reason the
app's own rules give for the engine state (unsupported, parse_failed, no entities, entities without relations) is mapped to the gold vocabulary
(unsupported_format, corrupt_file, empty_file, unrelated_content) and compared. A project file wrongly listed as not connected is a false alarm.
"""
import argparse, json, re, sys, tempfile, urllib.request, urllib.parse
from pathlib import Path

HERE = Path(__file__).resolve().parent


class Api:
    def __init__(self, base, token):
        self.base, self.token = base.rstrip("/"), token

    def get(self, path, **query):
        q = urllib.parse.urlencode({k: v for k, v in query.items() if v is not None})
        req = urllib.request.Request(f"{self.base}{path}" + (f"?{q}" if q else ""), headers={"Authorization": f"Bearer {self.token}", "OO-Docs-Api-Min": "1.0", "Accept": "application/json"})
        with urllib.request.urlopen(req, timeout=60) as r:
            return json.loads(r.read())

    def entities(self):
        out, cursor = [], None
        while True:
            page = self.get("/v1/docs/entities", limit=200, cursor=cursor)
            out += page["items"]
            cursor = page.get("next_cursor")
            if not cursor:
                return out

    def detail(self, key):
        return self.get(f"/v1/docs/entities/{urllib.parse.quote(key, safe=':')}")


def norm(s):
    return re.sub(r"\s+", " ", s.casefold()).strip()


def load_world(api):
    """Every non-Document entity with its names and the project-relative paths it cites."""
    out = []
    for e in api.entities():
        if e["type"] == "Document":
            continue
        d = api.detail(e["key"])
        names = {norm(d["name"])} | {norm(a["alias"]) for a in d.get("aliases", []) if a.get("status", "active") != "rejected"}
        files = {p.split("project/", 1)[-1] for p in d.get("files", [])}
        blob = [d["name"]] + [a["alias"] for a in d.get("aliases", [])]
        by_file = {}
        for f in d.get("facts", []):
            text = " ".join([f.get("predicate") or "", json.dumps(f.get("value"), ensure_ascii=False) if f.get("value") else "", (f.get("object") or {}).get("name") or ""])
            for ev in f.get("evidence", []):
                path = ev["path"].split("project/", 1)[-1]
                files.add(path)
                by_file.setdefault(path, []).append(text + " " + (ev.get("quote") or ""))
            blob.append(text)
        out.append({"key": e["key"], "type": e["type"], "name": d["name"], "names": {n for n in names if n}, "files": files, "blob": norm(" ".join(blob)),
                    "blob_by_file": {k: norm(" ".join(v)) for k, v in by_file.items()}})
    return out


def source_texts():
    sys.path.insert(0, str(HERE))
    import gen
    gen.FILES.clear(); gen.TEXT.clear()
    with tempfile.TemporaryDirectory() as t:
        gen.build(t)
    return {k: norm(v) for k, v in gen.TEXT.items()}


def score_relations(world, gold, texts):
    rows, found, lenient = [], 0, 0
    cover_sum = 0.0
    for r in gold:
        want = set(r["files"])
        names = [norm(n) for n in r["names"] if len(n) >= 2]
        best = None
        for e in world:
            if not any(n == m or (len(n) >= 4 and (n in m or m in n)) for n in names for m in e["names"]):
                continue
            hit = e["files"] & want
            if best is None or len(hit) > len(best[1]):
                best = (e, hit)
        hit = best[1] if best else set()
        cov = len(hit) / len(want)
        ok = len(hit) >= 2 and cov >= 0.6
        found += ok
        cover_sum += cov
        # Lenient: the ontology states the thing (a name, alias, fact value or evidence quote carries it) in these files, whichever entities hold it.
        said = set()
        for e in world:
            if any(n in e["blob"] for n in names):
                said |= e["files"] & want
                for path, text in e["blob_by_file"].items():
                    if path in want and any(n in text for n in names):
                        said.add(path)
        lcov = len(said) / len(want)
        lok = len(said) >= 2 and lcov >= 0.6
        lenient += lok
        rows.append({"id": r["id"], "kind": r["kind"], "label": r["label"], "found": ok, "coverage": round(cov, 2), "entity": best[0]["name"] if best else None,
                     "cited": len(hit), "gold_files": len(want), "missing": sorted(want - hit), "lenient_found": lok, "lenient_coverage": round(lcov, 2)})
    shown = supported = 0
    leaks, unsupported = [], []
    for e in world:
        if len(e["files"]) < 2:
            continue
        for f in sorted(e["files"]):
            shown += 1
            blob = texts.get(f)
            if blob is not None and any(n in blob for n in e["names"]):
                supported += 1
            else:
                unsupported.append({"entity": e["name"], "file": f})
            if f.startswith("misc/") and any(g.startswith("misc/") is False for g in e["files"]):
                leaks.append({"entity": e["name"], "file": f})
    by_kind = {}
    for row in rows:
        k = by_kind.setdefault(row["kind"], [0, 0, 0])
        k[0] += row["found"]; k[1] += row["lenient_found"]; k[2] += 1
    return {"gold": len(gold), "found": found, "recall": round(found / len(gold), 3), "lenient_found": lenient, "lenient_recall": round(lenient / len(gold), 3),
            "by_kind": {k: f"strict {a}/{n}, lenient {b}/{n}" for k, (a, b, n) in by_kind.items()}, "mean_file_coverage": round(cover_sum / len(gold), 3),
            "shown_links": shown, "supported_links": supported, "precision": round(supported / shown, 3) if shown else None,
            "leaks_into_unrelated_files": leaks, "unsupported_links": unsupported[:60]}, rows


APP_TO_GOLD = {"unsupported": "unsupported_format", "parse_failed": "corrupt_file", "no_entities": "empty_file", "no_relations": "unrelated_content", "entity": "unrelated_content",
               "connected": "linked_to_project", "review": "waiting_review", "processing": "still_processing"}


def engine_reason(file_rec, world, path, size):
    """The reason the app's own rules give for one file (renderer model/unlinked.ts fileReason), from the engine state."""
    state = file_rec.get("state")
    if state == "unsupported":
        return "unsupported"
    if state == "parse_failed":
        return "parse_failed"
    if state not in ("published", "extract_partial"):
        return "processing"
    mine = [e for e in world if path in e["files"]]
    if not mine:
        return "no_entities"
    return "connected" if any(len(e["files"]) > 1 for e in mine) else "no_relations"


def score_unlinked(gold, run, world, ui):
    rows, right = [], 0
    for g in gold:
        path = g["file"]
        key = "project/" + path
        rec = run["files"].get(key, {})
        reason = None
        if ui is not None:
            reason = ui.get(path, "connected")  # a file the Not connected view does not list is shown as connected
        else:
            reason = engine_reason(rec, world, path, rec.get("detail", {}).get("size"))
        mapped = APP_TO_GOLD.get(reason)
        # An empty file and a file with no entities show the same app reason; the gold says which one the file is by its size.
        if reason == "no_entities" and rec.get("detail", {}).get("size", 1) != 0:
            mapped = "unrelated_content"
        ok = mapped == g["kind"]
        right += ok
        rows.append({"file": path, "gold": g["kind"], "app_reason": reason, "mapped": mapped, "correct": ok, "state": rec.get("state"), "size": rec.get("detail", {}).get("size")})
    return {"files": len(gold), "correct": right, "correct_pct": round(100 * right / len(gold), 1)}, rows


def false_alarms(gold_unlinked, run, world, ui):
    unrelated = {g["file"] for g in gold_unlinked}
    out = []
    for key, rec in sorted(run["files"].items()):
        path = key.split("project/", 1)[-1]
        if path in unrelated:
            continue
        reason = (ui or {}).get(path) if ui is not None else engine_reason(rec, world, path, None)
        if reason not in (None, "connected", "processing"):
            out.append({"file": path, "reason": reason})
    return out


def per_file(run, world):
    rows = []
    for key, rec in sorted(run["files"].items()):
        path = key.split("project/", 1)[-1]
        tl = rec.get("timeline", [])
        pub = next((t["ms"] for t in tl if t["state"] == "published"), None)
        rows.append({"file": path, "format": rec.get("detail", {}).get("format"), "state": rec.get("state"), "code": rec.get("detail", {}).get("state_code"),
                     "published_s": round(pub / 1000, 1) if pub is not None else None, "entities": sum(1 for e in world if path in e["files"])})
    return rows


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--state-dir")
    ap.add_argument("--endpoint")
    ap.add_argument("--token-file", required=True)
    ap.add_argument("--run", required=True)
    ap.add_argument("--unlinked-ui")
    ap.add_argument("--gold", required=True)
    ap.add_argument("--out", required=True)
    a = ap.parse_args()
    base = a.endpoint or f"http://127.0.0.1:{json.load(open(Path(a.state_dir) / 'serve.json'))['port']}"
    api = Api(base, Path(a.token_file).read_text().strip())
    gold = Path(a.gold)
    rel_gold = [json.loads(l) for l in (gold / "relations.jsonl").read_text().splitlines() if l]
    unl_gold = [json.loads(l) for l in (gold / "unlinked.jsonl").read_text().splitlines() if l]
    run = json.loads(Path(a.run).read_text())
    ui = json.loads(Path(a.unlinked_ui).read_text()) if a.unlinked_ui else None
    world = load_world(api)
    rel_sum, rel_rows = score_relations(world, rel_gold, source_texts())
    unl_sum, unl_rows = score_unlinked(unl_gold, run, world, ui)
    alarms = false_alarms(unl_gold, run, world, ui)
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    files = per_file(run, world)
    (out / "relations-score.json").write_text(json.dumps({"summary": rel_sum, "rows": rel_rows}, ensure_ascii=False, indent=1))
    (out / "unlinked-score.json").write_text(json.dumps({"summary": unl_sum, "rows": unl_rows, "false_alarms": alarms}, ensure_ascii=False, indent=1))
    (out / "per-file.json").write_text(json.dumps(files, ensure_ascii=False, indent=1))
    (out / "entities.json").write_text(json.dumps([{**e, "names": sorted(e["names"]), "files": sorted(e["files"])} for e in world], ensure_ascii=False, indent=1))
    print(json.dumps({"relations": {k: v for k, v in rel_sum.items() if k not in ("unsupported_links", "leaks_into_unrelated_files")}, "leaks": len(rel_sum["leaks_into_unrelated_files"]),
                      "unlinked": unl_sum, "false_alarms": len(alarms), "entities": len(world)}, ensure_ascii=False, indent=1))


if __name__ == "__main__":
    main()
