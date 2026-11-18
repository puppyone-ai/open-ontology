#!/usr/bin/env python3
"""Generate testing/super-ontology/golden (Super Ontology card B-S1-04).

Everything is synthetic (a fictional 9-person company). Output is byte-deterministic:
no clocks, fixed seed for canaries, ZIP_STORED containers with a fixed timestamp.

  gen.py                 write into <repo>/testing/super-ontology/golden
  gen.py --out DIR       write into DIR
  gen.py --check         regenerate into a temp dir and compare bytes with the committed tree
  gen.py --variant holdout --seed N [--out DIR] [--check]
                         the held-out set (a different cast, same formats and label schema),
                         default <repo>/testing/super-ontology/holdout
"""
import argparse
import hashlib
import json
import os
import random
import sys
import tempfile
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))

import world  # noqa: E402
import world_holdout  # noqa: E402
import writers  # noqa: E402
from writers import WRITERS  # noqa: E402

DEFAULT_CREATOR = writers.CREATOR
SEED = 20261117
REPO = Path(__file__).resolve().parents[3]
DEFAULT_OUT = REPO / "testing" / "super-ontology" / "golden"
HOLDOUT_OUT = REPO / "testing" / "super-ontology" / "holdout"
MENTION_TYPES = {"Person", "Organization", "Project", "Topic", "Term", "Event"}

LABELS_README = """# Golden labels (oo.eval.docs-labels/v1)

Hand-curated labels for the synthetic golden set in `../docs/`. JSON Lines, UTF-8, one JSON object per
line, files `labels/*.jsonl` (the eval tool reads every `*.jsonl` in a directory). A predictions
directory uses exactly the same format, so `--predictions <this dir>` must score 1.0.

## Records

```json
{"kind":"entity","id":"E012","type":"Person","name":"陈子墨","aliases":["小陈"],"anchors":[{"file":"docs/People/陈子墨.md","frag":"#L5-L5"}]}
{"kind":"fact","id":"F007","subject":"E012","predicate":"owns","object":"E031","valid_from":"2026-01-01","valid_to":"2026-05-31","anchors":[...]}
{"kind":"supersede","old":"F007","new":"F008"}
{"kind":"conflict","id":"C1","constraint":"single_owner_per_project","facts":["F010","F011"]}
```

* `entity.type` is one of the D15 core types: `Person Organization Project Decision Document Topic Event Term`.
  A preference is `Decision` with `"subkind":"preference"`. `aliases` lists nicknames, English names, short names.
* `fact.predicate` is one of `owns member_of decides supersedes rejects mentions about tried_and_failed`.
  `subject` and `object` are entity ids of the same file set. `valid_from`/`valid_to` are `YYYY-MM-DD` (optional).
  A fact may carry `"value"` instead of `"object"` for literal values (unused in v1).
* `supersede`: fact `new` replaces fact `old` (the old one is no longer current).
* `conflict` (D57): the listed facts violate `constraint` at the same time; no engine should pick one.
* `anchors` (at least one on every entity and fact; predictions may omit them) point at evidence:
  `{"file": "<path relative to the golden root>", "frag": "<fragment>"}` plus `"bbox"` for pdf.

## Fragments (10 backend sec.3.5)

| format | fragment | meaning |
|---|---|---|
| md, txt | `#L12-L18` | 1-based closed line range of the file (frontmatter counted) |
| csv | `#R12` | 1-based row, header is row 1 |
| xlsx | `#'Sheet'!A1:C9` | sheet name is always single-quoted, A1 range |
| docx | `#¶12-14` | 0-based body paragraph range (table paragraphs not counted); `#T2!R3C1:R3C4` = table 2 (1-based), row 3 |
| pptx | `#s3` | 1-based slide |
| pdf | `#p5` | 1-based page; `"bbox":[x0,y0,x1,y1]` integers in 1/100 pt, origin top-left, page 595x842 pt |

Anchors overlap (provenance coverage) when: text, same file and line ranges intersect; csv/xlsx, same file
(and sheet) and ranges intersect; docx, paragraph ranges intersect or same table row; pptx, same slide;
pdf, same page and bbox IoU >= 0.5 (same page when either side has no bbox).

## Matching (10 backend sec.12.6)

Entities match on (type, normalised name or any normalised alias); normalisation is NFKC, lower-case,
trimmed, inner whitespace collapsed. Facts match on (matched subject, predicate, matched object). Matching is
one-to-one. Predictions are never required to reuse the gold ids.
"""

GENERATOR_MD = """# Golden set generator

All content is synthetic: a fictional company with nine invented employees, customers and projects; no customer data.
`python3 -B tools/super-ontology/golden/gen.py` rewrites this tree; `--check` regenerates into a temp dir and compares bytes.

* Content lives in `tools/super-ontology/golden/world.py`; writers in `writers.py`; stdlib only.
* Determinism: no clocks; canaries come from `random.Random(%d)`; zip containers are `ZIP_STORED` with timestamp
  2026-01-01 so bytes do not depend on the zlib version; PDF has no Info/ID and uses a hand-written object layout.
* Fonts/licences: no font is embedded anywhere. The PDFs reference the standard non-embedded CJK font `STSong-Light`
  (Adobe-GB1, `UniGB-UCS2-H` encoding) plus an identity `ToUnicode` map; OOXML files only name `SimSun`/`Calibri`.
* Binary files (docx/xlsx/pptx/pdf) are committed; `MANIFEST.sha256` records the sha256 of every file.
* Anchors in labels are computed by the writers while rendering, so they always match the files.
* `redteam/` holds files that must never reach a scope that does not include them (canary strings, red-team-only names,
  one same-name-different-person decoy). `canaries.txt` lists the canaries; `scopes.json` the scope combinations.
* `token-calibration.json` records measured sizes per file; `tokens` stays null until a tokenizer calibration is run.
""" % SEED


def jl(rows):
    return "".join(json.dumps(r, ensure_ascii=False, separators=(",", ":")) + "\n" for r in rows)


def norm(s):
    return " ".join(s.lower().split())


def render(docs, base):
    """Return ({relpath: bytes}, {doc path: infos})."""
    files, infos = {}, {}
    for d in docs:
        fmt = d["path"].rsplit(".", 1)[1]
        data, inf = WRITERS[fmt](d)
        rel = f"{base}/{d['path']}"
        files[rel] = data
        for i in inf:
            i["anchor"] = {"file": rel, **i["anchor"]}
        infos[d["path"]] = inf
    return files, infos


def holdout_generator_md(seed):
    return f"""# Held-out set generator

A second synthetic company for checking that extraction rules and prompts do not depend on the golden set's names.
`python3 -B tools/super-ontology/golden/gen.py --variant holdout --seed {seed}` rewrites this tree; add `--check` to compare.

* Same formats, block structure and label schema as `../golden/`; a different cast: every person, organisation, project,
  topic, term, event and place has a new invented name (and nicknames, English names), the calendar is one year on,
  weekday words differ and five documents that no label needs are left out (`world_holdout.py`, seed {seed}).
* The labels are derived by the same code as the golden labels, so they stay consistent with the files.
* Never used for tuning: it is scored once at the end of a change.
* Red team files and canaries are regenerated from the seed; they are not part of the evaluated `docs/` tree.
"""


def build(world=world, seed=SEED, variant="golden"):
    writers.CREATOR = getattr(world, "CREATOR", DEFAULT_CREATOR)
    rng = random.Random(seed)
    canaries = ["OOCANARY-%032x" % rng.getrandbits(128) for _ in range(4)]
    subst = {f"{{C{i + 1}}}": c for i, c in enumerate(canaries)}

    def fix(blocks):
        out = []
        for b in blocks:
            b = list(b)
            if isinstance(b[1], list):
                b[1] = [subst.get(c, c) for c in b[1]]
            else:
                for k, v in subst.items():
                    b[1] = b[1].replace(k, v)
            out.append(tuple(b))
        return out

    rt_docs = [{**d, "blocks": fix(d["blocks"])} for d in world.RT_DOCS]
    files, infos = render(world.DOCS, "docs")
    rt_files, rt_infos = render(rt_docs, "redteam/docs")
    files.update(rt_files)
    assert len(world.DOCS) >= getattr(world, 'MIN_DOCS', 50)

    # ---- entities
    eid, ent_rows, kmap = {}, [], {}
    n = 0

    def new_id():
        nonlocal n
        n += 1
        return f"E{n:03d}"

    def first_anchors(names, limit=3):
        out, seen = [], set()
        for d in world.DOCS:
            for i in infos[d["path"]]:
                if any(nm in i["text"] for nm in names):
                    if d["path"] not in seen:
                        seen.add(d["path"])
                        out.append(i["anchor"])
                    break
            if len(out) >= limit:
                break
        return out

    for key, (typ, name, aliases, extra) in world.ENT.items():
        anchors = first_anchors([name] + aliases)
        assert anchors, f"entity {key} {name} has no anchor"
        row = {"kind": "entity", "id": new_id(), "type": typ, "name": name, "aliases": aliases}
        row.update(extra)
        row["anchors"] = anchors
        eid[key] = row["id"]
        ent_rows.append(row)
    for d in world.DOCS:
        stem = d["path"].rsplit("/", 1)[-1].rsplit(".", 1)[0]
        key = "doc:" + d["path"]
        eid[key] = new_id()
        ent_rows.append({"kind": "entity", "id": eid[key], "type": "Document", "name": d["title"],
                         "aliases": [stem] if stem != d["title"] else [],
                         "anchors": [infos[d["path"]][0]["anchor"]]})
    # unique normalised (type, name) keys
    seen = {}
    for r in ent_rows:
        for nm in [r["name"]] + r["aliases"]:
            k = (r["type"], norm(nm))
            assert seen.setdefault(k, r["id"]) == r["id"], f"ambiguous label {k}"

    # ---- facts
    tagmap = {d["path"]: {i["tag"]: i["anchor"] for i in infos[d["path"]] if i["tag"]} for d in world.DOCS}
    fact_rows, flabel, fn = [], {}, 0

    def add_fact(pred, s, o, vf, vt, anchors, label=None):
        nonlocal fn
        fn += 1
        row = {"kind": "fact", "id": f"F{fn:03d}", "subject": s, "predicate": pred, "object": o}
        if vf:
            row["valid_from"] = vf
        if vt:
            row["valid_to"] = vt
        assert anchors
        row["anchors"] = anchors
        fact_rows.append(row)
        if label:
            flabel[label] = row["id"]

    for pred, s, o, vf, vt, tags, label in world.FACTS:
        anchors = [tagmap[p][t] for p, t in tags]
        add_fact(pred, eid[s], eid[o], vf, vt, anchors, label)
    for d in world.DOCS:
        first = infos[d["path"]][0]["anchor"]
        for k in d["about"]:
            add_fact("about", eid["doc:" + d["path"]], eid[k], d["created"], None, [first])
        for key, (typ, name, aliases, _x) in world.ENT.items():
            if typ not in MENTION_TYPES:
                continue
            for i in infos[d["path"]]:
                if any(nm in i["text"] for nm in [name] + aliases):
                    add_fact("mentions", eid["doc:" + d["path"]], eid[key], d["created"], None, [i["anchor"]])
                    break
    sup_rows = [{"kind": "supersede", "old": flabel[a], "new": flabel[b]} for a, b in world.SUPERSEDE]
    con_rows = [{"kind": "conflict", "id": f"C{i}", "constraint": c, "facts": [flabel[x] for x in fl]}
                for i, (c, fl) in enumerate(world.CONFLICTS, 1)]

    # ---- red team labels
    rt_rows = []
    for i, (key, typ, name, aliases, only, note) in enumerate(world.RT_ENT, 1):
        anchors = []
        for d in rt_docs:
            for inf in rt_infos[d["path"]]:
                if name in inf["text"] or any(a in inf["text"] for a in aliases):
                    anchors.append(inf["anchor"])
                    break
        assert anchors, key
        row = {"kind": "entity", "id": f"R{i:03d}", "type": typ, "name": name, "aliases": aliases,
               "redteam_only": only, "anchors": anchors}
        if note:
            row["note"] = note
        rt_rows.append(row)

    files["labels/entities.jsonl"] = jl(ent_rows).encode()
    files["labels/facts.jsonl"] = jl(fact_rows).encode()
    files["labels/supersedes.jsonl"] = jl(sup_rows).encode()
    files["labels/conflicts.jsonl"] = jl(con_rows).encode()
    files["labels/README.md"] = LABELS_README.encode()
    if variant == "holdout":
        files["GENERATOR.md"] = holdout_generator_md(seed).encode()
    files["redteam/redteam-entities.jsonl"] = jl(rt_rows).encode()
    files["redteam/README.md"] = ("# Red team files\n\nFiles under `docs/` carry canary strings and red-team-only names (see `redteam-entities.jsonl`: "
                                  "`redteam_only:true` names and aliases must never appear in output for a scope without these files; the record with "
                                  "`redteam_only:false` is a same-name-different-person decoy and is not forbidden text). Canaries are listed in `../canaries.txt`.\n").encode()
    files["canaries.txt"] = ("\n".join(canaries) + "\n").encode()
    files["scopes.json"] = (json.dumps({"protocol": "oo.eval.scopes/v1", "scopes": world.SCOPES}, ensure_ascii=False, indent=2) + "\n").encode()
    calib = []
    for rel in sorted(k for k in files if k.startswith(("docs/", "redteam/docs/"))):
        b = files[rel]
        txt = b.decode("utf-8", "ignore") if rel.rsplit(".", 1)[1] in ("md", "txt", "csv") else ""
        calib.append({"path": rel, "bytes": len(b), "cjk_chars": sum(1 for c in txt if ord(c) >= 0x2E80),
                      "ascii_chars": sum(1 for c in txt if ord(c) < 128), "tokens": None})
    files["token-calibration.json"] = (json.dumps({"protocol": "oo.eval.token-calibration/v1", "note": "tokens is null until calibrated against a tokenizer; cjk/ascii counts are for text formats only", "files": calib}, ensure_ascii=False, indent=2) + "\n").encode()
    files.setdefault("GENERATOR.md", GENERATOR_MD.encode())
    man = "".join(f"{hashlib.sha256(files[k]).hexdigest()}  {k}\n" for k in sorted(files))
    files["MANIFEST.sha256"] = man.encode()
    stats = {"docs": len(world.DOCS), "entities": len(ent_rows), "facts": len(fact_rows), "supersedes": len(sup_rows), "conflicts": len(con_rows), "redteam_docs": len(rt_docs)}
    return files, stats


def write(files, out):
    out = Path(out)
    for rel, data in files.items():
        p = out / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_bytes(data)


def disk_files(root):
    root = Path(root)
    return {str(p.relative_to(root)): p.read_bytes() for p in sorted(root.rglob("*")) if p.is_file()}


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--out")
    ap.add_argument("--check", action="store_true")
    ap.add_argument("--variant", choices=["golden", "holdout"], default="golden")
    ap.add_argument("--seed", type=int, default=None, help="holdout only")
    a = ap.parse_args(argv)
    if a.variant == "holdout":
        if a.seed is None:
            ap.error("--variant holdout needs --seed")
        files, stats = build(world_holdout.build(a.seed), seed=a.seed, variant="holdout")
        a.out = a.out or str(HOLDOUT_OUT)
    else:
        files, stats = build()
        a.out = a.out or str(DEFAULT_OUT)
    if a.check:
        with tempfile.TemporaryDirectory() as t:
            write(files, t)
            fresh = disk_files(t)
        if not Path(a.out).is_dir():
            print(f"golden tree missing: {a.out}", file=sys.stderr)
            return 1
        cur = disk_files(a.out)
        bad = [k for k in sorted(set(fresh) | set(cur)) if fresh.get(k) != cur.get(k)]
        for k in bad:
            print(f"MISMATCH {k}")
        print(f"golden check: {len(fresh)} files, {len(bad)} mismatches, {stats}")
        return 1 if bad else 0
    write(files, a.out)
    print(f"wrote {len(files)} files to {a.out} {stats}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
