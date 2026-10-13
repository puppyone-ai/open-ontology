#!/bin/bash
# Run the Linux 1.94 binary against the recorded C5 cycle. No import and no Mongo start.
set -euo pipefail

ROOT="/Users/redux/puppy-one/open-ontology"
EVIDENCE="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/c5-cycle-79"
BUILD="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/c5-linux-194-presence"
IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
BIN_VOLUME="oo-dl01w1-41-c5target194"

[[ "$(tr -d '[:space:]' < "$BUILD/docker.exit")" == "0" ]]
test -f /tmp/oo-dl01w1-41-secrets/reader
test -f /tmp/oo-dl01w1-41-secrets/dba
test -f /tmp/oo-dl01w1-41-secrets/audit

python3 - "$EVIDENCE" << 'PY'
import json, pathlib, sys
root = pathlib.Path(sys.argv[1])
keys = json.loads((root / "keys.json").read_text())
cycle_a = json.loads((root / "cycle-a.json").read_text())
cycle_b = json.loads((root / "cycle-b.json").read_text())
baseline = json.loads((root / "baseline.json").read_text())
approved = [
    "_id", "code", "flag", "units", "price", "created", "uid", "blob", "blob_presence",
    "weight", "warehouse", "whCode", "lane", "memo", "memo_presence", "marks",
    "marks_presence", "pack", "pack_presence",
]

def plan(method, every_ms, row_count):
    c1 = method == "C1"
    return {
        "protocol": "oo.mongodb-preview.sync-plan/v1",
        "method": method,
        "every_ms": every_ms,
        "delete_mode": "native" if c1 else "reconcile",
        "consistency": "T" if c1 else "B",
        "token_order": "Total" if c1 else "None",
        "quality": [] if c1 else ["DELETES_LAGGED"],
        "budget_rows": 16,
        "approved_fields": approved,
        "projected_fields": approved,
        "secret_ref": "secret://oo-dl01w1-41/reader",
        "secret_path_present": True,
        "topology": "standalone",
        "change_stream_privilege": False,
        "change_stream_server_code": 40573,
        "resume_supported": False,
        "oplog_window_known": False,
        "delete_event_observed": False,
        "delete_event_keys": [],
        "transaction_metadata": False,
        "write_class": "R1" if c1 else "R0",
        "fulldoc_requested": False,
        "fulldoc_contains_double": True,
        "fulldoc_claim_complete": False,
        "observed_fields": keys["fields"],
        "field_types": keys["field_types"],
        "row_count": row_count,
        "snapshot_token": None,
    }

def diff(before, after, tombstones):
    return {"before": before["rows"], "after": after["rows"], "tombstones": tombstones}

(root / "plan-c1-request.json").write_text(json.dumps(plan("C1", 3000, baseline["count"])))
(root / "plan-c5-a-request.json").write_text(json.dumps(plan("C5", 3000, cycle_a["count"])))
(root / "plan-c5-b-request.json").write_text(json.dumps(plan("C5", 8000, cycle_b["count"])))
deleted = "c141d04141eef0410000000f"
updated = "c141d04141eef04100000000"
(root / "diff-a-request.json").write_text(json.dumps(diff(baseline, cycle_a, [deleted])))
(root / "diff-b-request.json").write_text(json.dumps(diff(cycle_a, cycle_b, [deleted])))
(root / "diff-full-request.json").write_text(json.dumps(diff(baseline, cycle_b, [deleted])))
(root / "diff-resurrect-request.json").write_text(json.dumps(diff(baseline, cycle_b, [updated])))
PY

run_bin() {
  local name="$1"
  shift
  set +e
  docker run --rm --platform linux/arm64 --pull never \
    --memory 2g --memory-swap 2g \
    -v "$BIN_VOLUME:/target:ro" \
    -v "$EVIDENCE:/evidence" \
    "$IMAGE" \
    /target/debug/mongodb-preview "$@" \
    >"$EVIDENCE/${name}.stdout" 2>"$EVIDENCE/${name}.stderr"
  local status=$?
  set -e
  printf '%s\n' "$status" >"$EVIDENCE/${name}.exit"
  printf '%s\n' "$status"
}

run_bin plan-c1 --plan-sync --request /evidence/plan-c1-request.json --out /evidence/plan-c1-decision.json >/dev/null
run_bin plan-c5-a --plan-sync --request /evidence/plan-c5-a-request.json --out /evidence/plan-c5-a-decision.json >/dev/null
run_bin plan-c5-b --plan-sync --request /evidence/plan-c5-b-request.json --out /evidence/plan-c5-b-decision.json >/dev/null
run_bin diff-a --diff-sync --request /evidence/diff-a-request.json --out /evidence/diff-a-decision.json >/dev/null
run_bin diff-b --diff-sync --request /evidence/diff-b-request.json --out /evidence/diff-b-decision.json >/dev/null
run_bin diff-full --diff-sync --request /evidence/diff-full-request.json --out /evidence/diff-full-decision.json >/dev/null
run_bin diff-resurrect --diff-sync --request /evidence/diff-resurrect-request.json --out /evidence/diff-resurrect-decision.json >/dev/null

docker run --rm --platform linux/arm64 --pull never \
  -v "$BIN_VOLUME:/target:ro" \
  "$IMAGE" \
  bash -c 'od -An -tx1 -N 20 /target/debug/mongodb-preview' >"$EVIDENCE/mongodb-preview.elf-head.txt"

python3 - "$EVIDENCE" << 'PY'
import json, pathlib, sys
root = pathlib.Path(sys.argv[1])

def load(name):
    return json.loads((root / name).read_text())

def exit_of(name):
    return (root / f"{name}.exit").read_text().strip()

c1 = load("plan-c1-decision.json")
a = load("plan-c5-a-decision.json")
b = load("plan-c5-b-decision.json")
assert exit_of("plan-c1") == "3"
assert c1["outcome"] == "cdc_refused"
assert c1["capture_method"] is None
assert c1["every_ms"] == 3000
assert c1["cdc_claimed"] is False
assert [item["code"] for item in c1["blocks"]] == ["CDC_TOPOLOGY_UNSUPPORTED"]
assert c1["source_acceptance"] is False
assert c1["import_invoked"] is False
assert c1["generation_created"] is False
assert exit_of("plan-c5-a") == "0"
assert exit_of("plan-c5-b") == "0"
assert a["outcome"] == "c5_admitted" and b["outcome"] == "c5_admitted"
assert a["every_ms"] == 3000 and b["every_ms"] == 8000
assert a["consistency"] == "B" and a["delete_detection"] == "reconcile"
assert a["quality"] == ["DELETES_LAGGED"]
assert a["cdc_claimed"] is False
assert a["typed_weight_retained"] is True
assert a["approved_field_count"] == 19
hidden = a["unclassified"]
assert hidden and hidden[0]["field"] == "hiddenNote"
assert hidden[0]["visible"] is False and hidden[0]["projected"] is False
assert hidden[0]["raw_doc_authorized"] is False and hidden[0]["mixed"] is True
assert a["gaps"][0]["code"] == "JSONB_DOUBLE_UNENCODED"
assert a["gaps"][0]["owner"] == "DL01-W1-05"
assert a["gaps"][0]["also_owner"] == "DL01-W1-06"
assert a["fulldoc_produced"] is False and a["fulldoc_complete"] is False
diff_a = load("diff-a-decision.json")
diff_b = load("diff-b-decision.json")
full = load("diff-full-decision.json")
bad = load("diff-resurrect-decision.json")
assert exit_of("diff-a") == "0"
assert exit_of("diff-b") == "0"
assert exit_of("diff-full") == "0"
assert exit_of("diff-resurrect") == "3"
assert diff_a["updates"] == ["c141d04141eef04100000000"]
assert diff_a["deletes"] == ["c141d04141eef0410000000f"]
assert "c141d04141eef04100000010" not in diff_a["inserts"]
assert diff_a["import_invoked"] is False
assert diff_b["inserts"] == ["c141d04141eef04100000010"]
assert diff_b["deletes"] == []
assert "c141d04141eef0410000000f" not in diff_b["published"]
assert full["inserts"] == ["c141d04141eef04100000010"]
assert full["updates"] == ["c141d04141eef04100000000"]
assert full["deletes"] == ["c141d04141eef0410000000f"]
assert "c141d04141eef0410000000f" not in full["published"]
assert full["converged"] is True
assert full["generation_created"] is False
assert bad["converged"] is False
assert bad["reason"] == "resurrected"
assert bad["import_invoked"] is False
assert bad["generation_created"] is False
print("plan-diff-ok")
PY
