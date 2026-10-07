#!/bin/bash
# Readiness per-table chain. Does not rewrite live/llm or live/per-table.
# S9, S10, and the role pack stay withheld until the field producer has a fixed commit.
# The bearer stays in its file. This script never prints it and never passes it as an argument.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a328/SP07/pg-builder
LIVE=${EVIDENCE}/live
OUT=${LIVE}/ready
BIN=/tmp/oo-a328-eef831aa-dba4-4af2-a78b-541bc9e0a328/ready-host/target/debug/pg-builder
CACHE=/tmp/oo-a328-eef831aa-dba4-4af2-a78b-541bc9e0a328/ready-cache
FAIL_CACHE=/tmp/oo-a328-eef831aa-dba4-4af2-a78b-541bc9e0a328/ready-fail-cache
BEARER=/tmp/oo-customer-demo-20261007/llm-token
HIST=${LIVE}/llm/success.json
OLD426=${LIVE}/per-table/timeout.json
mkdir -p "$OUT" "$CACHE" "$FAIL_CACHE"
if [ ! -x "$BIN" ]; then
  echo "host pg-builder binary is absent" >&2
  exit 2
fi
if [ ! -f "$BEARER" ]; then
  echo "bearer file is absent" >&2
  exit 2
fi
if [ -e "$OUT/pass1.json" ]; then
  echo "refusing to overwrite ready/pass1.json" >&2
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
        f"http={transport.get('http_status')} usage={transport.get('usage')} "
        f"cost_status={cost.get('status')} cost_amount={cost.get('amount')} "
        f"invoice={cost.get('invoice')}"
    )
PY
}

write_config "$OUT/config-unavailable.json" fault_injection 9 8192 30000
write_config "$OUT/config-timeout.json" generate 8317 8192 1000
write_config "$OUT/config-success.json" generate 8317 8192 180000
write_config "$OUT/config-wide.json" generate 8317 32768 180000

shasum -a 256 "$LIVE/pass1/curated.json" > "$OUT/curated-before.sha256"
shasum -a 256 "$HIST" > "$OUT/historical-timeout-before.sha256"
shasum -a 256 "$OLD426" > "$OUT/old426-before.sha256"

run_gen unavailable "$OUT/config-unavailable.json" pass1 "$FAIL_CACHE"
run_gen timeout "$OUT/config-timeout.json" pass1 "$FAIL_CACHE"
find "$FAIL_CACHE" -type f | wc -l | awk '{print "fail_cache_files="$1}' > "$OUT/fail-cache-count.txt"

set +e
"$BIN" review \
  --generation "$OUT/unavailable.json" \
  --curated "$LIVE/pass1/curated.json" \
  --operator "$LIVE/operator-review.json" \
  --out "$OUT/failure-review.json" > "$OUT/failure-review.stdout" 2> "$OUT/failure-review.stderr"
printf '%s\n' "$?" > "$OUT/failure-review.exit"
set -e
echo "REVIEW failure=$(cat "$OUT/failure-review.exit")"

run_gen pass1 "$OUT/config-success.json" pass1 "$CACHE"
split_calls "$OUT/pass1.json" "$OUT/pass1-calls" | tee "$OUT/pass1-calls.txt"

pass1_outcome=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("outcome"))' "$OUT/pass1.json")
if [ "$pass1_outcome" != "validated" ]; then
  run_gen pass1-wide "$OUT/config-wide.json" pass1 "$CACHE"
  split_calls "$OUT/pass1-wide.json" "$OUT/pass1-wide-calls" | tee "$OUT/pass1-wide-calls.txt"
  SUCCESS=pass1-wide
  CHAIN="$OUT/config-wide.json"
else
  SUCCESS=pass1
  CHAIN="$OUT/config-success.json"
fi
printf '%s\n' "$SUCCESS" > "$OUT/success-name.txt"

run_gen pass1-hit "$CHAIN" pass1 "$CACHE"
run_gen pass2 "$CHAIN" pass2 "$CACHE"

shasum -a 256 "$LIVE/pass1/curated.json" > "$OUT/curated-after.sha256"
if cmp -s "$OUT/curated-before.sha256" "$OUT/curated-after.sha256"; then
  printf '%s\n' yes > "$OUT/curated-unchanged.txt"
else
  printf '%s\n' no > "$OUT/curated-unchanged.txt"
fi
shasum -a 256 "$HIST" > "$OUT/historical-timeout-after.sha256"
if cmp -s "$OUT/historical-timeout-before.sha256" "$OUT/historical-timeout-after.sha256"; then
  printf '%s\n' yes > "$OUT/historical-timeout-unchanged.txt"
else
  printf '%s\n' no > "$OUT/historical-timeout-unchanged.txt"
fi
shasum -a 256 "$OLD426" > "$OUT/old426-after.sha256"
if cmp -s "$OUT/old426-before.sha256" "$OUT/old426-after.sha256"; then
  printf '%s\n' yes > "$OUT/old426-unchanged.txt"
else
  printf '%s\n' no > "$OUT/old426-unchanged.txt"
fi
"$BIN" scan --root "$OUT" --bearer-file "$BEARER" > "$OUT/bearer-scan.txt" 2> "$OUT/bearer-scan.stderr" || true
printf '%s\n' 'description-candidate.dataset_only: a339 field producer has no fixed commit. S9, S10, and the two-role pack are withheld. Model and cache results in this directory do not publish field text.' \
  > "$OUT/seams.txt"

set +e
python3 - "$OUT" <<'PY'
import json, sys
from pathlib import Path
out = Path(sys.argv[1])
name = (out / "success-name.txt").read_text().strip()
doc = json.loads((out / f"{name}.json").read_text())
hit = json.loads((out / "pass1-hit.json").read_text())
miss = json.loads((out / "pass2.json").read_text())
calls = doc.get("calls") or []
hit_calls = hit.get("calls") or []
miss_calls = miss.get("calls") or []

def honest(call):
    transport = call.get("transport") or {}
    cost = transport.get("declared_cost") or {}
    usage = transport.get("usage") or {}
    return (
        call.get("outcome") == "validated"
        and transport.get("model_id") == "grok-4.7"
        and isinstance(usage, dict)
        and usage.get("prompt_tokens", 0) + usage.get("completion_tokens", 0) == usage.get("total_tokens")
        and cost.get("status") == "unknown"
        and cost.get("amount") is None
        and cost.get("invoice") is False
        and cost.get("billing_certificate") is False
    )

ok = doc.get("outcome") == "validated" and calls and all(honest(call) for call in calls)
ok = ok and hit.get("cache") == "hit"
ok = ok and (hit.get("transport") or {}).get("socket") == "not_opened"
ok = ok and (hit.get("transport") or {}).get("attempt") == 0
ok = ok and doc.get("candidate_sha256") == hit.get("candidate_sha256")
ok = ok and miss.get("cache") == "miss"
ok = ok and (miss.get("transport") or {}).get("socket") == "opened"
inputs = {call.get("dataset"): call.get("input_sha256") for call in calls}
miss_inputs = {call.get("dataset"): call.get("input_sha256") for call in miss_calls}
ok = ok and inputs and inputs != miss_inputs
ok = ok and (out / "curated-unchanged.txt").read_text().strip() == "yes"
ok = ok and (out / "historical-timeout-unchanged.txt").read_text().strip() == "yes"
ok = ok and (out / "old426-unchanged.txt").read_text().strip() == "yes"
ok = ok and (out / "failure-review.exit").read_text().strip() == "1"
ok = ok and not (out / "failure-review.json").exists()
ok = ok and not list(out.glob("pack-*.json"))
timeout = json.loads((out / "timeout.json").read_text())
for call in timeout.get("calls") or []:
    transport = call.get("transport") or {}
    if transport.get("refusal") in {"timeout", "http_status", "unavailable"}:
        ok = ok and call.get("cache_store") == "not_stored"
lines = [
    f"success={name}",
    f"outcome={doc.get('outcome')}",
    f"hit_cache={hit.get('cache')}",
    f"hit_socket={(hit.get('transport') or {}).get('socket')}",
    f"hit_attempt={(hit.get('transport') or {}).get('attempt')}",
    f"candidate_bytes_same={str(doc.get('candidate_sha256') == hit.get('candidate_sha256')).lower()}",
    f"pass2_cache={miss.get('cache')}",
    f"pass2_socket={(miss.get('transport') or {}).get('socket')}",
]
(out / "cache-hit.txt").write_text("\n".join(lines) + "\n")
print("\n".join(lines))
print("model_cache_gate=" + ("pass" if ok else "fail"))
raise SystemExit(0 if ok else 1)
PY
gate=$?
set -e
printf '%s\n' "$gate" > "$OUT/gate.exit"
exit "$gate"
