#!/bin/bash
# One host macOS grok-4.7 call for shop.account after the Linux builder is gone.
# The stored three-table seal is narrowed by generate() to the one card.
# The bearer file is referenced and never copied or printed.
set -u
set -e
set -o pipefail
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a357/SP07/pg-builder-review
OUT=${REVIEW}/live-one
PROOF=${REVIEW}/linux/proof-4
ROOT=/tmp/oo-a357-eef831aa-dba4-4af2-a78b-541bc9e0a357/host-live
RECEIPTS=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a328/SP07/pg-builder/live/pass1
FIXED=a13b1cc80ed66c07199a41d47b31fd42530f2be9
BEARER=/tmp/oo-customer-demo-20261007/llm-token
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5

if [ ! -f "$PROOF/container-removed.txt" ]; then
  echo "linux proof container is not recorded as removed" >&2
  exit 3
fi
if docker ps -q --filter label=oo.kind=rust-builder | grep -q .; then
  echo "a rust builder is still running" >&2
  exit 3
fi
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
if [ "$host_cargo" -ge 1 ]; then
  echo "host rustc or cargo is already running" >&2
  exit 3
fi
if ! lsof -nP -iTCP:8317 -sTCP:LISTEN >/dev/null 2>&1; then
  echo "127.0.0.1:8317 is not listening" >&2
  exit 4
fi
if [ ! -f "$BEARER" ]; then
  echo "bearer file is missing" >&2
  exit 4
fi
bearer_bytes=$(stat -f %z "$BEARER")
if [ "$bearer_bytes" -ne 6 ]; then
  echo "bearer file length is not the expected local token" >&2
  exit 4
fi
printf 'bearer_bytes=%s bearer_printed=no\n' "$bearer_bytes" > "$OUT/bearer-posture.txt"

mkdir -p "$ROOT/archive" "$ROOT/home" "$ROOT/cargo" "$ROOT/target" "$OUT/work"
rm -rf "$ROOT/archive"
mkdir -p "$ROOT/archive"
git -C "$SRC" archive --format=tar "$FIXED" \
  Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates \
  tools/description-review tools/description-pack \
  tools/pg-builder testing/real-db/pg-builder \
  | tar -x -C "$ROOT/archive"
rm -f "$ROOT/archive/rust-toolchain.toml"
export HOME="$ROOT/home"
export CARGO_HOME="$ROOT/cargo"
export CARGO_TARGET_DIR="$ROOT/target"
export CARGO_BUILD_JOBS=1
export CARGO_TERM_COLOR=never
unset HTTP_PROXY HTTPS_PROXY ALL_PROXY http_proxy https_proxy all_proxy OPENAI_API_KEY OPENAI_BASE_URL || true
/opt/homebrew/bin/rustc -vV > "$OUT/rustc-version.txt"
/opt/homebrew/bin/cargo -V > "$OUT/cargo-version.txt"
set +e
/opt/homebrew/bin/cargo fetch --manifest-path "$ROOT/archive/tools/pg-builder/Cargo.toml" --locked \
  > "$OUT/fetch.log" 2>&1
fetch_exit=$?
set -e
printf '%s\n' "$fetch_exit" > "$OUT/fetch.exit"
if [ "$fetch_exit" -ne 0 ]; then
  exit "$fetch_exit"
fi
set +e
/opt/homebrew/bin/cargo fetch --manifest-path "$ROOT/archive/tools/pg-builder/Cargo.toml" --locked --offline \
  > "$OUT/fetch-offline.log" 2>&1
offline_exit=$?
set -e
printf '%s\n' "$offline_exit" > "$OUT/fetch-offline.exit"
if [ "$offline_exit" -ne 0 ]; then
  exit "$offline_exit"
fi
set +e
/opt/homebrew/bin/cargo build --manifest-path "$ROOT/archive/tools/pg-builder/Cargo.toml" --locked --offline --bin pg-builder \
  > "$OUT/build.log" 2>&1
build_exit=$?
set -e
printf '%s\n' "$build_exit" > "$OUT/build.exit"
if [ "$build_exit" -ne 0 ]; then
  exit "$build_exit"
fi
BIN="$CARGO_TARGET_DIR/debug/pg-builder"
jq ' .tables |= map(select(.dataset == "shop.account"))' "$RECEIPTS/cards.json" > "$OUT/work/cards.json"
count=$(jq '.tables | length' "$OUT/work/cards.json")
if [ "$count" != "1" ]; then
  echo "account card subset was not one table" >&2
  exit 5
fi
jq -e '.tables[0].dataset == "shop.account"' "$OUT/work/cards.json" >/dev/null
cat > "$OUT/work/config.json" <<EOF
{
  "protocol": "oo.pg-builder.config/v1",
  "adapter": "loopback_http_local_dev",
  "namespace": "pg-builder",
  "purpose": "generate",
  "sample_policy": "none",
  "temperature": 0,
  "endpoint": {
    "origin": "http://127.0.0.1:8317",
    "path": "/v1/chat/completions",
    "model_id": "grok-4.7"
  },
  "secret": {"bearer_file": "${BEARER}"},
  "declared_price": {"status": "unknown"},
  "budget": {
    "deadline_ms": 180000,
    "max_request_bytes": 262144,
    "max_response_bytes": 262144,
    "max_prompt_tokens": 100000,
    "max_completion_tokens": 100000,
    "max_total_tokens": 200000,
    "max_tables": 16,
    "max_in_flight": 1
  }
}
EOF
token=$(<"$BEARER")
config_text=$(<"$OUT/work/config.json")
case "$config_text" in
  *"$token"*)
    echo "config contains the bearer bytes" >&2
    exit 6
    ;;
esac
unset token config_text
"$BIN" approve \
  --cards "$OUT/work/cards.json" \
  --reviewer-label local-operator \
  --reviewer-at 2026-10-08 \
  --out "$OUT/work/approval.json" > "$OUT/approve.log" 2>&1
printf '%s\n' 0 > "$OUT/approve.exit"
set +e
"$BIN" generate \
  --config "$OUT/work/config.json" \
  --seal "$RECEIPTS/seal.json" \
  --cards "$OUT/work/cards.json" \
  --approval "$OUT/work/approval.json" \
  --cache "$OUT/work/cache" \
  --out "$OUT/work/generation.json" > "$OUT/generate.log" 2>&1
gen_exit=$?
set -e
printf '%s\n' "$gen_exit" > "$OUT/generate.exit"
if [ ! -f "$OUT/work/generation.json" ]; then
  printf 'model_calls=attempted dataset=shop.account fallback=no result=no-document\n' > "$OUT/call-posture.txt"
  exit "$gen_exit"
fi
jq '{
  protocol, outcome, cache, cache_store, dataset,
  input_sha256, candidate_sha256,
  socket: .transport.socket,
  attempt: .transport.attempt,
  http_status: .transport.http_status,
  status: .transport.status,
  finish_reason: .transport.finish_reason,
  usage: .transport.usage,
  declared_cost: .transport.declared_cost,
  model_id: .transport.model_id,
  endpoint_origin: .transport.endpoint_origin
}' "$OUT/work/generation.json" > "$OUT/summary.json"
shasum -a 256 "$OUT/work/generation.json" "$OUT/work/cards.json" "$OUT/work/approval.json" "$RECEIPTS/seal.json" > "$OUT/inputs.sha256"
printf 'model_calls=1 dataset=shop.account fallback=no\n' > "$OUT/call-posture.txt"
exit "$gen_exit"
