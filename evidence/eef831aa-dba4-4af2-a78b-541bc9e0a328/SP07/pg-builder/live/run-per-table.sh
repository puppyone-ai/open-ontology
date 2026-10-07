#!/bin/bash
# Per-table loopback chain. Does not rewrite live/llm or the historical timeout documents.
# The bearer stays in its file. This script never prints it and never passes it as an argument.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a328/SP07/pg-builder
LIVE=${EVIDENCE}/live
OUT=${LIVE}/per-table
BIN=/tmp/oo-a328-eef831aa-dba4-4af2-a78b-541bc9e0a328/per-table-host/target/debug/pg-builder
CACHE=/tmp/oo-a328-eef831aa-dba4-4af2-a78b-541bc9e0a328/per-table-cache
FAIL_CACHE=/tmp/oo-a328-eef831aa-dba4-4af2-a78b-541bc9e0a328/per-table-fail-cache
BEARER=/tmp/oo-customer-demo-20261007/llm-token
HIST=${LIVE}/llm/pass1-success.json
if [ ! -f "$HIST" ]; then
  HIST=${LIVE}/llm/success.json
fi
mkdir -p "$OUT" "$CACHE" "$FAIL_CACHE"
if [ ! -x "$BIN" ]; then
  echo "host pg-builder binary is absent" >&2
  exit 2
fi
if [ ! -f "$BEARER" ]; then
  echo "bearer file is absent" >&2
  exit 2
fi
bearer_bytes=$(wc -c < "$BEARER" | tr -d ' ')
printf 'bearer_bytes=%s\n' "$bearer_bytes" > "$OUT/bearer-bytes.txt"

write_config() {
  local path="$1" purpose="$2" port="$3" completion="$4" deadline="$5"
  cat > "$path" <<EOF
{
  "protocol": "oo.pg-builder.config/v1",
  "adapter": "loopback_http_local_dev",
  "namespace": "pg-builder",
  "purpose": "${purpose}",
  "sample_policy": "none",
  "temperature": 0,
  "endpoint": {
    "origin": "http://127.0.0.1:${port}",
    "path": "/v1/chat/completions",
    "model_id": "grok-4.7"
  },
  "secret": {"bearer_file": "${BEARER}"},
  "declared_price": {"status": "unknown"},
  "budget": {
    "deadline_ms": ${deadline},
    "max_request_bytes": 262144,
    "max_response_bytes": 262144,
    "max_prompt_tokens": 100000,
    "max_completion_tokens": ${completion},
    "max_total_tokens": 200000,
    "max_tables": 16,
    "max_in_flight": 1
  }
}
EOF
}

run_gen() {
  local name="$1" config="$2" pass="$3" cache="$4"
  local start end
  start=$(date +%s)
  set +e
  "$BIN" generate \
    --config "$config" \
    --seal "$LIVE/$pass/seal.json" \
    --cards "$LIVE/$pass/cards.json" \
    --approval "$LIVE/$pass/approval.json" \
    --cache "$cache" \
    --out "$OUT/${name}.json" > "$OUT/${name}.stdout" 2> "$OUT/${name}.stderr"
  local code=$?
  set -e
  end=$(date +%s)
  printf '%s\n' "$code" > "$OUT/${name}.exit"
  printf '%s\n' $((end - start)) > "$OUT/${name}.elapsed"
  echo "GEN $name=$code elapsed=$((end - start))"
}

split_calls() {
  python3 - "$1" "$2" <<'PY'
import json, sys
from pathlib import Path
doc = json.loads(Path(sys.argv[1]).read_text())
calls = doc.get("calls")
if not isinstance(calls, list):
    calls = [doc]
out = Path(sys.argv[2])
out.mkdir(parents=True, exist_ok=True)
for old in out.glob("*.json"):
    old.unlink()
for index, call in enumerate(calls):
    dataset = str(call.get("dataset") or f"call{index}")
    batch = int((call.get("batch") or {}).get("index") or 0)
    path = out / f"{dataset}-batch{batch}.json"
    path.write_text(json.dumps(call, indent=2, sort_keys=True) + "\n")
    transport = call.get("transport") or {}
    cost = transport.get("declared_cost") or {}
    print(
        f"{path.name} outcome={call.get('outcome')} cache={call.get('cache')} "
        f"socket={transport.get('socket')} attempt={transport.get('attempt')} "
        f"status={transport.get('status')} refusal={transport.get('refusal')} "
        f"usage={transport.get('usage')} cost_status={cost.get('status')} "
        f"cost_amount={cost.get('amount')} invoice={cost.get('invoice')}"
    )
PY
}

write_config "$OUT/config-unavailable.json" fault_injection 9 8192 30000
write_config "$OUT/config-timeout.json" generate 8317 8192 1000
write_config "$OUT/config-success.json" generate 8317 8192 180000

shasum -a 256 "$LIVE/pass1/curated.json" > "$OUT/curated-before.sha256"
if [ -f "$HIST" ]; then
  shasum -a 256 "$HIST" > "$OUT/historical-timeout-before.sha256"
fi
curl -sS -D "$OUT/endpoint.headers" -o "$OUT/endpoint.body" -m 5 -w '%{http_code}\n' \
  http://127.0.0.1:8317/v1/models > "$OUT/endpoint.status" || printf '%s\n' curl_failed > "$OUT/endpoint.status"

run_gen unavailable "$OUT/config-unavailable.json" pass1 "$FAIL_CACHE"
run_gen timeout "$OUT/config-timeout.json" pass1 "$FAIL_CACHE"
find "$FAIL_CACHE" -type f | wc -l | awk '{print "fail_cache_files="$1}' > "$OUT/fail-cache-count.txt"

run_gen pass1 "$OUT/config-success.json" pass1 "$CACHE"
split_calls "$OUT/pass1.json" "$OUT/pass1-calls" | tee "$OUT/pass1-calls.txt"
run_gen pass1-hit "$OUT/config-success.json" pass1 "$CACHE"
split_calls "$OUT/pass1-hit.json" "$OUT/pass1-hit-calls" | tee "$OUT/pass1-hit-calls.txt"
run_gen pass2 "$OUT/config-success.json" pass2 "$CACHE"
split_calls "$OUT/pass2.json" "$OUT/pass2-calls" | tee "$OUT/pass2-calls.txt"

python3 - "$OUT" "$LIVE" <<'PY'
import json, sys
from pathlib import Path
out = Path(sys.argv[1])
live = Path(sys.argv[2])
pass1 = json.loads((out / "pass1.json").read_text())
hit = json.loads((out / "pass1-hit.json").read_text())
same = pass1.get("candidate_sha256") == hit.get("candidate_sha256") and pass1.get("candidates") == hit.get("candidates")
lines = [
    f"pass1_outcome={pass1.get('outcome')}",
    f"pass1_cache={pass1.get('cache')}",
    f"hit_cache={hit.get('cache')}",
    f"hit_socket={(hit.get('transport') or {}).get('socket')}",
    f"hit_attempt={(hit.get('transport') or {}).get('attempt')}",
    f"candidate_bytes_same={str(same).lower()}",
]
(out / "cache-hit.txt").write_text("\n".join(lines) + "\n")
print("\n".join(lines))

def one(dataset, decisions):
    kept = [row for row in decisions if row.get("dataset") == dataset]
    return kept

review = json.loads((live / "operator-review.json").read_text())
classes = json.loads((live / "classifications.json").read_text())
roles = json.loads((live / "roles.json").read_text())
for path in sorted((out / "pass1-calls").glob("*.json")):
    call = json.loads(path.read_text())
    dataset = call.get("dataset")
    if not dataset or call.get("outcome") != "validated" or not isinstance(call.get("complete_command"), str):
        continue
    op = dict(review)
    op["decisions"] = one(dataset, review["decisions"])
    (out / f"operator-{dataset}.json").write_text(json.dumps(op, indent=2) + "\n")
    cl = dict(classes)
    cl["decisions"] = one(dataset, classes["decisions"])
    (out / f"class-{dataset}.json").write_text(json.dumps(cl, indent=2) + "\n")
    role = json.loads(json.dumps(roles))
    for item in role:
        item["datasets"] = [row for row in item["datasets"] if row.get("dataset") == dataset]
    (out / f"role-{dataset}.json").write_text(json.dumps(role, indent=2) + "\n")
    print(f"prepared {dataset}")
PY

set +e
"$BIN" review \
  --generation "$OUT/pass1.json" \
  --curated "$LIVE/pass1/curated.json" \
  --operator "$LIVE/operator-review.json" \
  --out "$OUT/aggregate-review.json" > "$OUT/aggregate-review.stdout" 2> "$OUT/aggregate-review.stderr"
printf '%s\n' "$?" > "$OUT/aggregate-review.exit"
set -e
echo "REVIEW aggregate=$(cat "$OUT/aggregate-review.exit")"

for call in "$OUT"/pass1-calls/*.json; do
  dataset=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("dataset") or "")' "$call")
  [ -n "$dataset" ] || continue
  [ -f "$OUT/operator-${dataset}.json" ] || continue
  set +e
  "$BIN" review \
    --generation "$call" \
    --curated "$LIVE/pass1/curated.json" \
    --operator "$OUT/operator-${dataset}.json" \
    --out "$OUT/review-${dataset}.json" > "$OUT/review-${dataset}.stdout" 2> "$OUT/review-${dataset}.stderr"
  printf '%s\n' "$?" > "$OUT/review-${dataset}.exit"
  "$BIN" pack \
    --generation "$call" \
    --curated "$LIVE/pass1/curated.json" \
    --operator "$OUT/operator-${dataset}.json" \
    --settings "$LIVE/settings.json" \
    --classifications "$OUT/class-${dataset}.json" \
    --roles "$OUT/role-${dataset}.json" \
    --hidden national_id \
    --out "$OUT/pack-${dataset}.json" > "$OUT/pack-${dataset}.stdout" 2> "$OUT/pack-${dataset}.stderr"
  printf '%s\n' "$?" > "$OUT/pack-${dataset}.exit"
  set -e
  echo "TABLE $dataset review=$(cat "$OUT/review-${dataset}.exit") pack=$(cat "$OUT/pack-${dataset}.exit")"
done

shasum -a 256 "$LIVE/pass1/curated.json" > "$OUT/curated-after.sha256"
if cmp -s "$OUT/curated-before.sha256" "$OUT/curated-after.sha256"; then
  printf '%s\n' yes > "$OUT/curated-unchanged.txt"
else
  printf '%s\n' no > "$OUT/curated-unchanged.txt"
fi
if [ -f "$HIST" ]; then
  shasum -a 256 "$HIST" > "$OUT/historical-timeout-after.sha256"
  if cmp -s "$OUT/historical-timeout-before.sha256" "$OUT/historical-timeout-after.sha256"; then
    printf '%s\n' yes > "$OUT/historical-timeout-unchanged.txt"
  else
    printf '%s\n' no > "$OUT/historical-timeout-unchanged.txt"
  fi
fi
"$BIN" scan --root "$OUT" --bearer-file "$BEARER" > "$OUT/bearer-scan.txt" 2> "$OUT/bearer-scan.stderr" || true
printf '%s\n' 'description-review.single-request: the aggregate generation has no single complete command. Per-table review uses each call complete command. Field publication remains description-candidate.dataset_only.' \
  > "$OUT/seams.txt"

python3 - "$OUT" <<'PY'
import json, sys
from pathlib import Path
out = Path(sys.argv[1])
pass1 = json.loads((out / "pass1.json").read_text())
hit = json.loads((out / "pass1-hit.json").read_text())
ok = pass1.get("outcome") == "validated"
ok = ok and hit.get("cache") == "hit"
ok = ok and (hit.get("transport") or {}).get("socket") == "not_opened"
ok = ok and (hit.get("transport") or {}).get("attempt") == 0
ok = ok and pass1.get("candidate_sha256") == hit.get("candidate_sha256")
reviews = list(out.glob("review-shop.*.exit"))
packs = list(out.glob("pack-shop.*.exit"))
ok = ok and reviews and all(p.read_text().strip() == "0" for p in reviews)
ok = ok and packs and all(p.read_text().strip() == "0" for p in packs)
ok = ok and (out / "curated-unchanged.txt").read_text().strip() == "yes"
print("acceptance_gate=" + ("pass" if ok else "fail"))
raise SystemExit(0 if ok else 1)
PY
gate=$?
printf '%s\n' "$gate" > "$OUT/gate.exit"
exit "$gate"
