#!/usr/bin/env python3
"""Compare public Gateway rows with the independent Doris source row bag."""

from collections import Counter
import hashlib
import json
from pathlib import Path
import subprocess
import sys


def sha(data):
    return hashlib.sha256(data).hexdigest()


def main():
    if len(sys.argv) != 8:
        raise SystemExit("usage: verify_gateway_gold.py SOURCE_AFTER COLLECTION BINDING CONTAINER TOKEN_FILE GATEWAY_URL OUTPUT_DIR")
    source_path, collection_path, binding_path, container, token_file, gateway, output_dir = sys.argv[1:]
    out = Path(output_dir)
    out.mkdir(parents=True, exist_ok=True)
    source = json.loads(Path(source_path).read_text())
    collection = json.loads(Path(collection_path).read_text())
    binding = json.loads(Path(binding_path).read_text())
    assert source["database"] == binding["schema"]
    assert collection["binding_vector"] == binding["binding_vector"]
    allowed = {(x["dataset"], x["column"]) for x in binding["allow"]}
    assert len(allowed) == 18
    source_tables = {x["name"]: x for x in source["tables"]}
    catalog_tables = {x["tables"][0]["logical_table"]: x["tables"][0] for x in collection["entries"]}
    results = []
    for name in sorted(source_tables):
        table = catalog_tables[name]
        schema = table["logical_schema"]
        dataset = schema + "." + name
        source_names = [x["logical_name"] for x in table["columns"]]
        indices = [i for i, col in enumerate(source_names) if (dataset, col) in allowed]
        columns = [source_names[i] for i in indices]
        assert columns
        gold = Counter(tuple(None if cells[i] == r"\N" else cells[i] for i in indices)
                       for row in source_tables[name]["rows"] for cells in [row.split("\x1f")])
        sql = "SELECT " + ", ".join('"' + c + '"' for c in columns) + ' FROM "' + schema + '"."' + name + '"'
        cmd = ["docker", "exec", container, "/bin/oo", "query", "--json", "--auth", "external", "--token-file", token_file, "--gateway", gateway, "--transport", "local-development", sql]
        run = subprocess.run(cmd, capture_output=True)
        (out / (name + ".stdout")).write_bytes(run.stdout)
        (out / (name + ".stderr")).write_bytes(run.stderr)
        case = {"table": dataset, "columns": columns, "expected_rows": sum(gold.values()), "exit": run.returncode, "stdout_sha256": sha(run.stdout), "stderr_sha256": sha(run.stderr)}
        if run.returncode == 0:
            body = json.loads(run.stdout)
            actual = Counter(tuple(None if cell is None else str(cell) for cell in row) for row in body["rows"])
            case.update(query_id=body.get("query_id"), actual_rows=sum(actual.values()), equal=actual == gold, missing=list((gold - actual).elements()), extra=list((actual - gold).elements()))
        else:
            try:
                body = json.loads(run.stdout)
                case.update(code=body.get("code"), message=body.get("message"), query_id=body.get("query_id"))
            except (ValueError, UnicodeDecodeError):
                case["code"] = "non_json_failure"
        results.append(case)
    result = {"source_sha256": sha(Path(source_path).read_bytes()), "collection_sha256": sha(Path(collection_path).read_bytes()), "binding_sha256": sha(Path(binding_path).read_bytes()), "cases": results, "all_equal": all(x.get("equal") is True for x in results)}
    (out / "gold-diff.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps({"all_equal": result["all_equal"], "tables": len(results), "gold_diff_sha256": sha((out / "gold-diff.json").read_bytes())}))
    if not result["all_equal"]:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
