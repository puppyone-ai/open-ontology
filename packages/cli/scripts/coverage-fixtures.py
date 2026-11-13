"""Offline evaluation inputs from unchanged T6 gold and the coordinator demo.

No answers or source data are generated. T6 metadata follows the existing seed
DDL, without question-derived labels. The fixture compiler is not publication.
"""
import copy
import importlib.util
import json
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[3]


def prepare(out):
    out.mkdir(parents=True, exist_ok=True)
    demo = lambda name: json.loads(subprocess.check_output(
        ["git", "show", f"v0.3/m:deploy/lite/demo/{name}.json"], cwd=ROOT))
    reviewed = demo("reviewed-analyst")
    artifact = demo("pack-analyst")
    effective, visibility = copy.deepcopy(reviewed["effective"]), copy.deepcopy(reviewed["visibility"])
    spec = importlib.util.spec_from_file_location("seed", ROOT / "testing/engines/seed-common/generate.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    names = {"regions", "areas", "sites", "codes", "customers", "orders", "shipments",
             "calendar", "order_lines", "events", "log_202601", "log_202602"}
    datasets = []
    for statement in module.generate("postgres", "17", 1, 0):
        match = re.fullmatch(r"CREATE TABLE (\w+) \((.*)\);\n", statement)
        if not match or match[1] not in names:
            continue
        # Split only top-level commas: numeric precision and composite keys
        # contain commas too.
        fields, start, depth = [], 0, 0
        for i, ch in enumerate(match[2]):
            depth += (ch == "(") - (ch == ")")
            if ch == "," and depth == 0:
                fields.append(match[2][start:i].strip())
                start = i + 1
        fields.append(match[2][start:].strip())
        columns = []
        for field in fields:
            if field.startswith(("PRIMARY ", "FOREIGN ")):
                continue
            name, kind = field.split(None, 1)
            if name in {"phone_synthetic", "national_id_synthetic"}:
                continue
            dtype = ("text" if kind.startswith("VARCHAR") else
                     "numeric" if kind.startswith("DECIMAL") else
                     "timestamp" if kind.startswith("TIMESTAMP") else
                     "date" if kind.startswith("DATE") else "bigint")
            columns.append({"name": name, "label": name, "data_type": dtype, "pii": "none",
                            "classification": {"state": "confirmed", "by": "fixture/test"},
                            "provenance": "declared"})
        datasets.append({"schema_version": "oos/v1", "dataset": "demo." + match[1],
                         "label": match[1], "tier": "raw", "primary_key": [], "columns": columns})
    assert len(datasets) == 12
    effective["datasets"] = datasets
    for key in ("entities", "relationships", "enums", "examples", "glossary", "metrics"):
        effective[key] = []
    visibility["datasets"] = [{"dataset": d["dataset"], "columns": [c["name"] for c in d["columns"]],
                              "row_security": "none", "global_stats": "denied"} for d in datasets]
    positives = json.loads((ROOT / "testing/engines/gold/questions.json").read_text())
    demo_gold = demo("gold")
    negatives = [
        "How many employees exist?", "What is total payroll?", "List hospital patients",
        "How many aircraft exist?", "Show university student grades", "Count insurance policies",
        "How many hotel reservations exist?", "List laboratory experiments",
        "Show customer phone numbers", "List customer national identifiers",
        "Show customer email addresses", "List customer birthdates",
        "Show order credit card numbers", "Show customer passwords",
        "Show shipment driver medical records", "Show customer bank accounts",
        "What is total order profit?", "What is order gross margin?",
        "What is customer lifetime value?", "What is shipment carbon emission?",
        "What is order refund rate?", "What is customer churn probability?",
        "What is shipment insurance liability?",
    ]
    assert len(negatives) == len(positives) + sum(not q.get("denied") for q in demo_gold)
    t6 = [dict(q, answerable=True) for q in positives]
    t6 += [{"id": f"N{i+1:02}", "question": q, "answerable": False} for i, q in enumerate(negatives[:20])]
    demo_questions = [dict(q, answerable=not q.get("denied")) for q in demo_gold]
    # Keep the existing demo deny as an additional negative, not a positive.
    demo_questions += [{"id": f"N{i+21:02}", "question": q, "answerable": False}
                       for i, q in enumerate(negatives[20:])]
    for name, value in [("t6-input", {"effective": effective, "visibility": visibility}),
                        ("demo-input", artifact), ("t6-questions", t6), ("demo-questions", demo_questions)]:
        (out / f"{name}.json").write_text(json.dumps(value, ensure_ascii=True) + "\n")
    (out / "demo-revision.txt").write_text(subprocess.check_output(
        ["git", "rev-parse", "v0.3/m"], cwd=ROOT).decode())


if __name__ == "__main__":
    prepare(Path(sys.argv[1]))
