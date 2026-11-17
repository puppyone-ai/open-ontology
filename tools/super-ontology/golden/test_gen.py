import json
import sys
from collections import Counter
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import gen  # noqa: E402

CORE = {"Person", "Organization", "Project", "Decision", "Document", "Topic", "Event", "Term"}
PRED = {"owns", "member_of", "decides", "supersedes", "rejects", "mentions", "about", "tried_and_failed"}


def lines(files, name):
    return [json.loads(x) for x in files[name].decode().splitlines()]


def test_golden_distribution_and_determinism():
    a, _ = gen.build()
    b, _ = gen.build()
    assert a == b
    fmt = Counter(k.rsplit(".", 1)[1] for k in a if k.startswith("docs/"))
    assert sum(fmt.values()) >= 50
    assert set(fmt) == {"md", "docx", "xlsx", "pdf", "pptx", "csv", "txt"}
    for d in ("People/", "Projects/", "Preferences/"):
        assert any(k.startswith("docs/" + d) for k in a)


def test_golden_labels_have_anchors_and_valid_types():
    files, _ = gen.build()
    ents = lines(files, "labels/entities.jsonl")
    ids = {e["id"] for e in ents}
    assert {e["type"] for e in ents} <= CORE
    assert all(e["anchors"] for e in ents)
    facts = lines(files, "labels/facts.jsonl")
    assert all(f["anchors"] and f["predicate"] in PRED and f["subject"] in ids and f["object"] in ids for f in facts)
    for r in ents + facts:
        for a in r["anchors"]:
            assert a["file"] in files and a["frag"].startswith("#")
    fids = {f["id"] for f in facts}
    for s in lines(files, "labels/supersedes.jsonl"):
        assert s["old"] in fids and s["new"] in fids
    conflicts = lines(files, "labels/conflicts.jsonl")
    assert conflicts and all(set(c["facts"]) <= fids for c in conflicts)
    assert any(e.get("subkind") == "preference" for e in ents)


def test_golden_redteam_canaries_only_in_redteam():
    files, _ = gen.build()
    canaries = files["canaries.txt"].decode().split()
    assert len(canaries) >= 4 and all(c.startswith("OOCANARY-") and len(c) == 9 + 32 for c in canaries)
    rt = [e for e in lines(files, "redteam/redteam-entities.jsonl") if e["redteam_only"]]
    names = [e["name"] for e in rt] + [a for e in rt for a in e["aliases"]]
    for rel, data in files.items():
        if rel.startswith("redteam/") or rel == "canaries.txt":
            continue
        text = data.decode("utf-8", "ignore")
        assert not any(c in text for c in canaries), rel
        if rel.endswith((".md", ".jsonl", ".csv", ".txt")):
            assert not any(n in text for n in names), rel


def test_golden_check_mode(tmp_path):
    assert gen.main(["--out", str(tmp_path / "g")]) == 0
    assert gen.main(["--out", str(tmp_path / "g"), "--check"]) == 0
    (tmp_path / "g" / "canaries.txt").write_text("x\n")
    assert gen.main(["--out", str(tmp_path / "g"), "--check"]) == 1
