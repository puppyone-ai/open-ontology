#!/bin/bash
# Host loopback chain. The bearer is read from its file by pg-builder. It is not an argument.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a328/SP07/pg-builder
LIVE=${EVIDENCE}/live
LLM=${LIVE}/llm
BIN=/tmp/oo-a328-eef831aa-dba4-4af2-a78b-541bc9e0a328/host-target/debug/pg-builder
CACHE=/tmp/oo-a328-eef831aa-dba4-4af2-a78b-541bc9e0a328/llm-cache
BEARER=/tmp/oo-customer-demo-20261007/llm-token
mkdir -p "$LLM" "$CACHE"
if [ ! -x "$BIN" ]; then
  echo "host pg-builder binary is absent" >&2
  exit 2
fi

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
  local name="$1" config="$2" pass="$3"
  set +e
  "$BIN" generate \
    --config "$config" \
    --seal "$LIVE/$pass/seal.json" \
    --cards "$LIVE/$pass/cards.json" \
    --approval "$LIVE/$pass/approval.json" \
    --cache "$CACHE" \
    --out "$LLM/${name}.json" > "$LLM/${name}.stdout" 2> "$LLM/${name}.stderr"
  local code=$?
  set -e
  printf '%s\n' "$code" > "$LLM/${name}.exit"
  echo "GEN $name=$code"
}

write_config "$LLM/config-timeout.json" generate 8317 100000 1
write_config "$LLM/config-truncated.json" generate 8317 1 30000
write_config "$LLM/config-unavailable.json" fault_injection 9 100000 30000
write_config "$LLM/config-success.json" generate 8317 100000 30000

shasum -a 256 "$LIVE/pass1/curated.json" > "$LLM/curated-before.sha256"
rm -f "$LLM/pack-hidden-should-be-absent.json"

run_gen timeout "$LLM/config-timeout.json" pass1
run_gen truncated "$LLM/config-truncated.json" pass1
run_gen unavailable "$LLM/config-unavailable.json" pass1
run_gen success "$LLM/config-success.json" pass1
run_gen cache-hit "$LLM/config-success.json" pass1
run_gen injection "$LLM/config-success.json" pass2

set +e
"$BIN" pack \
  --generation "$LLM/success.json" \
  --curated "$LIVE/pass1/curated.json" \
  --operator "$LIVE/operator-review.json" \
  --settings "$LIVE/settings.json" \
  --classifications "$LIVE/classifications.json" \
  --roles "$LIVE/roles-with-hidden.json" \
  --out "$LLM/pack-hidden-should-be-absent.json" > "$LLM/pack-hidden.stdout" 2> "$LLM/pack-hidden.stderr"
printf '%s\n' "$?" > "$LLM/pack-hidden.exit"
"$BIN" review \
  --generation "$LLM/success.json" \
  --curated "$LIVE/pass1/curated.json" \
  --operator "$LIVE/operator-review.json" \
  --out "$LLM/review.json" > "$LLM/review.stdout" 2> "$LLM/review.stderr"
printf '%s\n' "$?" > "$LLM/review.exit"
"$BIN" pack \
  --generation "$LLM/success.json" \
  --curated "$LIVE/pass1/curated.json" \
  --operator "$LIVE/operator-review.json" \
  --settings "$LIVE/settings.json" \
  --classifications "$LIVE/classifications.json" \
  --roles "$LIVE/roles.json" \
  --hidden national_id \
  --out "$LLM/pack.json" > "$LLM/pack.stdout" 2> "$LLM/pack.stderr"
printf '%s\n' "$?" > "$LLM/pack.exit"
# The pass2 comment is an injection attempt. Do not apply review when the
# generation validated: a validated draft is still not a grant or a publication.
# When it did not validate, one refusal probe must write no review file.
inject_outcome=$(python3 -c 'import json,sys; from pathlib import Path; p=Path(sys.argv[1]); print(json.loads(p.read_text()).get("outcome") if p.is_file() else "missing")' "$LLM/injection.json")
rm -f "$LLM/injection-review-should-be-absent.json"
if [ "$inject_outcome" = "validated" ]; then
  printf '%s\n' "injection_review=not_invoked outcome=validated" | tee "$LLM/injection-review.skipped"
  printf '%s\n' "not_invoked" > "$LLM/injection-review.exit"
else
  "$BIN" review \
    --generation "$LLM/injection.json" \
    --curated "$LIVE/pass2/curated.json" \
    --operator "$LIVE/operator-review.json" \
    --out "$LLM/injection-review-should-be-absent.json" > "$LLM/injection-review.stdout" 2> "$LLM/injection-review.stderr"
  printf '%s\n' "$?" > "$LLM/injection-review.exit"
fi
set -e

shasum -a 256 "$LIVE/pass1/curated.json" > "$LLM/curated-after.sha256"
if cmp -s "$LLM/curated-before.sha256" "$LLM/curated-after.sha256"; then
  echo "curated_unchanged=yes" | tee "$LLM/curated-unchanged.txt"
else
  echo "curated_unchanged=no" | tee "$LLM/curated-unchanged.txt"
fi

python3 - <<'PY'
import json
from pathlib import Path
llm = Path("/Users/redux/puppy-one/open-ontology/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a328/SP07/pg-builder/live/llm")
names = ["timeout", "truncated", "unavailable", "success", "cache-hit", "injection"]
lines = []
for name in names:
    path = llm / f"{name}.json"
    if not path.exists():
        lines.append(f"{name} missing")
        continue
    doc = json.loads(path.read_text())
    transport = doc.get("transport", {})
    cost = transport.get("declared_cost", {})
    lines.append(
        " ".join([
            name,
            f"outcome={doc.get('outcome')}",
            f"field={doc.get('field_outcome')}",
            f"cache={doc.get('cache')}",
            f"store={doc.get('cache_store')}",
            f"socket={transport.get('socket')}",
            f"attempt={transport.get('attempt')}",
            f"status={transport.get('status')}",
            f"finish={transport.get('finish_reason')}",
            f"refusal={transport.get('refusal')}",
            f"http={transport.get('http_status')}",
            f"tokens={transport.get('usage')}",
            f"latency_ms={transport.get('latency_ms')}",
            f"cost_status={cost.get('status')}",
            f"amount={cost.get('amount')}",
            f"invoice={cost.get('invoice')}",
            f"billing={cost.get('billing_certificate')}",
            f"model={transport.get('model_id')}",
            f"request={transport.get('request_input_sha256')}",
            f"response={transport.get('response_sha256')}",
            f"repair={doc.get('repair')}",
        ])
    )
success = json.loads((llm / "success.json").read_text()) if (llm / "success.json").exists() else {}
hit = json.loads((llm / "cache-hit.json").read_text()) if (llm / "cache-hit.json").exists() else {}
same = success.get("candidate_sha256") == hit.get("candidate_sha256") and success.get("candidates") == hit.get("candidates")
lines.append(f"cache_hit_same_candidates={str(same).lower()}")
(llm / "summary.txt").write_text("\n".join(lines) + "\n")
print("\n".join(lines))
PY

"$BIN" scan --root "$EVIDENCE" --bearer-file "$BEARER" > "$LLM/bearer-scan.txt" 2> "$LLM/bearer-scan.stderr" || true
echo "llm_chain_recorded"
