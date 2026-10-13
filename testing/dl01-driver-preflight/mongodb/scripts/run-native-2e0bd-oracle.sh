#!/bin/bash
# Independent cell oracle for the isolated native generations. Read-only on the platform database.
set -euo pipefail

ROOT="/Users/redux/puppy-one/open-ontology"
BASE="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product"
OUT="$BASE/raw/native-2e0bd/oracle"
PG_NAME="oo-dl01w1-41-pg-native"
if [[ -e "$OUT/cell-diff.json" ]]; then
  echo "oracle evidence already exists" >&2
  exit 2
fi
mkdir -p "$OUT"
docker exec oo-dl01w1-41-pg psql -v ON_ERROR_STOP=1 -U oo_w141_admin -d oo_w141 -At -c \
  "SELECT count(*) FROM _oo.pt_4d11431451ccacc3837c1cbd40cc4f0e28844c70b465650f63aef51b9898;" \
  >"$OUT/platform-gen2-count.txt"
docker exec oo-dl01w1-41-pg psql -v ON_ERROR_STOP=1 -U oo_w141_admin -d oo_w141 -At -c \
  "SELECT count(*) FROM _oo.pt_a1d0e63667e64ab1b7b9c250e46e7043a798de705406b292dd4b238c3254;" \
  >"$OUT/platform-gen1-count.txt"
python3 - <<'PY'
import json, subprocess
from pathlib import Path
base = Path("/Users/redux/puppy-one/open-ontology/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product")
out = base / "raw/native-2e0bd/oracle"
generations = {
    "gen1": {
        "physical": "pt_44103bb2e2cc9a13bbfe3077807b25900ca663bcd39106c1b7d511b31f01",
        "key": "pc_239fb02775dbaf237da937684a85c30d0e2c85a00716c0afc77e3b9880e7",
        "doc": "pc_cd4bd092a50bcdf0827141cedfdbe31257712b356c9c4f05544e257acba3",
        "seal": base / "raw/docseal-80/baseline.seal.json",
        "request": base / "raw/native-2e0bd/gen1/request.json",
    },
    "gen2": {
        "physical": "pt_d1eed4b241abd54a6a1749cb83000d6d0423cc3584f318db35cdb192774c",
        "key": "pc_700c406ada2011eaa7e3fca1985bfd04db583540daaf18c39f3fbcc202eb",
        "doc": "pc_3724855edb2f247566f5739ffcef479c10e1191e304132db0a72a5ccf8f5",
        "seal": base / "raw/docseal-80/cycle-b.seal.json",
        "request": base / "raw/native-2e0bd/gen2/request.json",
    },
}
absent = {
    "deleted": "c141d04141eef0410000000f",
    "updated_hidden": "c141d04141eef04100000000",
    "inserted_hidden": "c141d04141eef04100000010",
    "temporary": "c141d04141eef04100000011",
}

def psql(sql):
    result = subprocess.run(
        ["docker", "exec", "-i", "oo-dl01w1-41-pg-native", "psql", "-U", "postgres", "-d", "oo_w141", "-At", "-v", "ON_ERROR_STOP=1"],
        input=sql.encode(),
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=True,
    )
    return result.stdout.decode()

report = {"protocol": "oo.mongodb-preview.native-cell-oracle/v1", "source_acceptance": False, "generations": {}}
for name, spec in generations.items():
    seal = json.loads(spec["seal"].read_text())
    request = json.loads(spec["request"].read_text())
    stored_sql = (
        "SELECT COALESCE(json_agg(json_build_array({key}::text, {doc}::text) ORDER BY {key}::text), '[]'::json)\n"
        "FROM _oo.{physical};\n"
    ).format(**spec)
    stored = json.loads(psql(stored_sql))
    stored_map = {row[0]: row[1] for row in stored}
    pieces = []
    expected = {}
    for index, row in enumerate(seal["rows"]):
        if row.get("outcome") != "sealed":
            continue
        document = row["extended_json"]
        key = document["_id"]["$oid"]
        tag = f"m{index}"
        payload = json.dumps(document, separators=(",", ":"))
        pieces.append(f"SELECT '{key}', (${tag}${payload}${tag}$::jsonb)::text")
        expected[key] = {
            "code": document.get("code"),
            "weight": document.get("weight", {}).get("$numberDouble"),
            "seal_weight": row.get("weight"),
        }
    rendered = {}
    if pieces:
        text = psql("SELECT json_agg(json_build_array(k, t)) FROM (" + " UNION ALL ".join(pieces) + ") AS q(k, t);\n")
        for key, rendered_text in json.loads(text) or []:
            rendered[key] = rendered_text
    truth = {row[0]: row[1] for row in request["truth"]}
    mismatches = []
    for key, meta in expected.items():
        got = stored_map.get(key)
        if got != rendered.get(key):
            mismatches.append({"key": key, "kind": "live_pg_text"})
        if got != truth.get(key):
            mismatches.append({"key": key, "kind": "request_truth"})
        parsed = json.loads(got) if got is not None else None
        if not isinstance(parsed, dict):
            mismatches.append({"key": key, "kind": "stored_json"})
            continue
        if "hiddenNote" in parsed:
            mismatches.append({"key": key, "kind": "hidden_note"})
        if parsed.get("code") != meta["code"]:
            mismatches.append({"key": key, "kind": "code"})
        weight = parsed.get("weight")
        if not isinstance(weight, dict) or weight.get("$numberDouble") != meta["weight"] or meta["weight"] != meta["seal_weight"]:
            mismatches.append({"key": key, "kind": "weight", "got": weight, "want": meta["weight"]})
    extra = sorted(set(stored_map) - set(expected))
    missing = sorted(set(expected) - set(stored_map))
    report["generations"][name] = {
        "physical": spec["physical"],
        "stored_rows": len(stored_map),
        "sealed_rows": len(expected),
        "extra_keys": extra,
        "missing_keys": missing,
        "mismatches": mismatches,
        "cell_diff": len(mismatches) + len(extra) + len(missing),
        "absent_keys": {label: key not in stored_map for label, key in absent.items()},
        "jsonb_oid": 3802,
        "jsonb_typmod": -1,
    }
(out / "cell-diff.json").write_text(json.dumps(report, indent=2) + "\n")
print(json.dumps({name: {"rows": item["stored_rows"], "cell_diff": item["cell_diff"], "absent": item["absent_keys"]} for name, item in report["generations"].items()}, indent=2))
PY
echo "platform_gen2=$(cat "$OUT/platform-gen2-count.txt")"
echo "platform_gen1=$(cat "$OUT/platform-gen1-count.txt")"
