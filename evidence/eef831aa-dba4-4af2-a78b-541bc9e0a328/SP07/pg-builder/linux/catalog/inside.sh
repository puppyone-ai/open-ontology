#!/bin/bash
# Build catalog-pg and pg-builder from the fixed archive, then read the private database.
# This script does not invoke cargo update and does not call the model endpoint.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target/pipeline
export CARGO_BUILD_JOBS=1
export CARGO_TERM_COLOR=never
export TMPDIR=/cargo-home/tmp
EV=/evidence/linux/catalog
mkdir -p /cargo-home/tmp /cargo-home/bin "$EV/phases" /evidence/live/pass1 /evidence/live/pass2 /evidence/live/flags
printf '%s\n' no > "$EV/cargo-update-invoked.txt"

record() {
  printf '%s\n' "$2" > "$EV/phases/$1.exit"
  echo "PHASE $1=$2"
}

echo "=== identity ==="
rustc -vV | tee "$EV/rustc-version.txt"
cargo -V | tee "$EV/cargo-version.txt"
uname -a | tee "$EV/uname.txt"

echo "=== copy archive ==="
rm -rf /work
mkdir -p /work
cp -a /src/. /work/
rm -f /work/rust-toolchain.toml
cd /work
if [ ! -f crates/oo-pipeline/Cargo.toml ] || [ ! -f tools/pg-builder/src/main.rs ]; then
  echo "archive or overlay is incomplete" >&2
  record overlay 2
  exit 2
fi
record overlay 0

echo "=== workspace fetch locked ==="
sha256sum Cargo.lock | awk '{print $1}' > "$EV/root-lock-before.sha256"
set +e
cargo fetch --manifest-path /work/Cargo.toml --locked > "$EV/fetch-workspace.log" 2>&1
fetch_exit=$?
set -e
record fetch-workspace "$fetch_exit"
sha256sum Cargo.lock | awk '{print $1}' > "$EV/root-lock-after.sha256"
if [ "$fetch_exit" -ne 0 ]; then
  exit "$fetch_exit"
fi
if ! cmp -s "$EV/root-lock-before.sha256" "$EV/root-lock-after.sha256"; then
  echo "root lock bytes changed" >&2
  record root-lock 1
  exit 1
fi
record root-lock 0

echo "=== oo-pipeline ==="
set +e
cargo build --manifest-path /work/Cargo.toml --locked --offline -p oo-pipeline --bin oo-pipeline \
  > "$EV/build-pipeline.log" 2>&1
pipe_exit=$?
set -e
record build-pipeline "$pipe_exit"
if [ "$pipe_exit" -ne 0 ]; then
  exit "$pipe_exit"
fi
cp /target/pipeline/debug/oo-pipeline /cargo-home/bin/oo-pipeline

echo "=== pg-builder ==="
set +e
CARGO_TARGET_DIR=/target/pg-builder cargo build --manifest-path /work/tools/pg-builder/Cargo.toml --locked --offline --bin pg-builder \
  > "$EV/build-builder.log" 2>&1
builder_exit=$?
set -e
record build-builder "$builder_exit"
if [ "$builder_exit" -ne 0 ]; then
  exit "$builder_exit"
fi
cp /target/pg-builder/debug/pg-builder /cargo-home/bin/pg-builder
sha256sum /cargo-home/bin/oo-pipeline /cargo-home/bin/pg-builder > "$EV/bin-sha256.txt"

printf '%s\n' 'local-catalog-placeholder' > /evidence/live/unused-bearer
cat > /evidence/live/pg-config.json <<'EOF'
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
  "secret": {"bearer_file": "/evidence/live/unused-bearer"},
  "declared_price": {"status": "unknown"},
  "budget": {
    "deadline_ms": 30000,
    "max_request_bytes": 262144,
    "max_response_bytes": 262144,
    "max_prompt_tokens": 100000,
    "max_completion_tokens": 100000,
    "max_total_tokens": 200000,
    "max_tables": 16,
    "max_in_flight": 1
  },
  "postgres": {
    "host": "oo-a328-pg",
    "port": 5432,
    "database": "oo_a328_src",
    "username": "oo_a328_reader",
    "password_file": "/secrets/reader.pass",
    "ca_file": "/secrets/ca.crt",
    "catalog_bin": "/cargo-home/bin/oo-pipeline",
    "tables": [
      {"schema": "shop", "table": "party"},
      {"schema": "shop", "table": "account"},
      {"schema": "shop", "table": "entry"}
    ]
  }
}
EOF

run_pass() {
  local pass="$1"
  local dir="/evidence/live/${pass}"
  mkdir -p "$dir"
  set +e
  /cargo-home/bin/pg-builder catalog --config /evidence/live/pg-config.json \
    --source-id pubfix --secret-ref secret://pg/a328/reader \
    --stderr "$dir/catalog.stderr" --out "$dir/catalog.json" > "$dir/catalog.stdout" 2>&1
  local catalog_exit=$?
  /cargo-home/bin/pg-builder profile --config /evidence/live/pg-config.json --out "$dir/profile.json" > "$dir/profile.stdout" 2>&1
  local profile_exit=$?
  /cargo-home/bin/pg-builder comments --config /evidence/live/pg-config.json --out "$dir/comments.json" > "$dir/comments.stdout" 2>&1
  local comments_exit=$?
  /cargo-home/bin/pg-builder seal --catalog "$dir/catalog.json" --out "$dir/seal.json" > "$dir/seal.stdout" 2>&1
  local seal_exit=$?
  /cargo-home/bin/pg-builder cards --config /evidence/live/pg-config.json \
    --catalog "$dir/catalog.json" --profile "$dir/profile.json" --comments "$dir/comments.json" \
    --seal "$dir/seal.json" --out "$dir/cards.json" > "$dir/cards.stdout" 2>&1
  local cards_exit=$?
  /cargo-home/bin/pg-builder curated --catalog "$dir/catalog.json" --out "$dir/curated.json" > "$dir/curated.stdout" 2>&1
  local curated_exit=$?
  /cargo-home/bin/pg-builder approve --cards "$dir/cards.json" \
    --reviewer-label local-operator --reviewer-at 2026-10-07 --out "$dir/approval.json" > "$dir/approve.stdout" 2>&1
  local approve_exit=$?
  set -e
  printf '%s\n' "$catalog_exit" > "$dir/catalog.exit"
  printf '%s\n' "$profile_exit" > "$dir/profile.exit"
  printf '%s\n' "$comments_exit" > "$dir/comments.exit"
  printf '%s\n' "$seal_exit" > "$dir/seal.exit"
  printf '%s\n' "$cards_exit" > "$dir/cards.exit"
  printf '%s\n' "$curated_exit" > "$dir/curated.exit"
  printf '%s\n' "$approve_exit" > "$dir/approve.exit"
  echo "PASS $pass catalog=$catalog_exit profile=$profile_exit comments=$comments_exit seal=$seal_exit cards=$cards_exit curated=$curated_exit approve=$approve_exit"
  if [ "$catalog_exit" -ne 0 ] || [ "$profile_exit" -ne 0 ] || [ "$comments_exit" -ne 0 ] || [ "$seal_exit" -ne 0 ] || [ "$cards_exit" -ne 0 ] || [ "$curated_exit" -ne 0 ] || [ "$approve_exit" -ne 0 ]; then
    return 1
  fi
  return 0
}

echo "=== pass1 ==="
if ! run_pass pass1; then
  record pass1 1
  exit 1
fi
record pass1 0
date -u +%Y-%m-%dT%H:%M:%SZ > /evidence/live/flags/pass1.ready

echo "=== wait for comment replacement ==="
found=0
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31 32 33 34 35 36 37 38 39 40 41 42 43 44 45 46 47 48 49 50 51 52 53 54 55 56 57 58 59 60; do
  if [ -f /evidence/live/flags/comment.ready ]; then
    found=1
    break
  fi
  sleep 5
done
if [ "$found" -ne 1 ]; then
  echo "comment replacement did not arrive" >&2
  record comment-wait 2
  exit 2
fi
record comment-wait 0

echo "=== pass2 ==="
if ! run_pass pass2; then
  record pass2 1
  exit 1
fi
record pass2 0
echo "catalog_chain_done"
exit 0
