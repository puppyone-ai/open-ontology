#!/bin/bash
# Real pg-builder, dialogue, query-pg, and answer-citations phase.
# Runs only after the 100k chain exits 0. Does not print the bearer.
set -u
echo "oo_task=sp12-product-benchmark-bench"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a389"
echo "phase=bench"
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TERM_COLOR=never
export HOME=/tmp
export TMPDIR=/cargo-home/tmp
export CARGO_BUILD_JOBS=1
unset HTTP_PROXY HTTPS_PROXY ALL_PROXY NO_PROXY http_proxy https_proxy all_proxy no_proxy OPENAI_API_KEY OPENAI_BASE_URL OPENAI_API_BASE || true
mkdir -p /evidence/bench/phases /evidence/score-input /evidence/live/builder/cache /evidence/report \
  /cargo-home/tmp /cargo-home/bin /cargo-home/relay

record() {
  printf '%s\n' "$2" > "/evidence/bench/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
}

run_log() {
  name=$1
  shift
  set +e
  "$@" > "/evidence/bench/${name}.log" 2>&1
  code=$?
  set +e
  record "$name" "$code"
  return 0
}

if [ -d /cargo-home/a274-clang-root/usr/lib ]; then
  libdir=$(find /cargo-home/a274-clang-root/usr/lib \( -name 'libclang.so' -o -name 'libclang-*.so' \) -print -quit)
  if [ -n "$libdir" ]; then
    libdir=$(dirname "$libdir")
    export LIBCLANG_PATH="$libdir"
    export LD_LIBRARY_PATH="${libdir}:/cargo-home/a274-clang-root/usr/lib/aarch64-linux-gnu"
    export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I/cargo-home/a274-clang-root/usr/lib/llvm-19/lib/clang/19/include -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
    export PATH="/cargo-home/bin:/cargo-home/a274-clang-root/usr/bin:${PATH}"
  fi
fi

RUN=/evidence/live/run
BINS=/cargo-home/a317-bins
BENCH=/evidence/live/builder
SCORE=/evidence/score-input
BEARER=/run/llm-token
QUESTION="Count the rows in shop.entry. The only statement that matches this question is SELECT count(*) FROM shop.entry."

copy_chain_queries() {
  cp "$RUN/import-result.json" "$SCORE/import-result.json"
  cp "$RUN/apply-outcome.json" "$SCORE/apply-outcome.json"
  for name in wide-count wide-null wide-qty wide-fk wide-phone narrow-party wide-hidden; do
    cp "$RUN/steps/query-${name}.stdout" "$SCORE/query-${name}.stdout"
    cp "$RUN/steps/query-${name}.exit" "$SCORE/query-${name}.exit"
  done
}
set +e
copy_chain_queries > /evidence/bench/copy-chain.log 2>&1
copy_exit=$?
set +e
record copy-chain "$copy_exit"

run_log prep-build rustc --edition 2024 -D warnings /crate/linux/bench/prep.rs -o /cargo-home/bin/oo-a389-prep
run_log forward-build rustc --edition 2024 -D warnings /crate/linux/bench/forward.rs -o /cargo-home/bin/oo-a389-forward
if [ "$(cat /evidence/bench/phases/forward-build.exit)" = "0" ]; then
  /cargo-home/bin/oo-a389-forward > /evidence/bench/forward.log 2>&1 &
  echo $! > /evidence/bench/forward.pid
  sleep 1
  record forward-start 0
else
  record forward-start 1
fi

run_log fetch-pg-builder cargo fetch --manifest-path /pg-builder-src/tools/pg-builder/Cargo.toml --locked
run_log build-pg-builder env CARGO_TARGET_DIR=/target/pg-builder cargo build --manifest-path /pg-builder-src/tools/pg-builder/Cargo.toml --locked --offline --bin pg-builder
if [ "$(cat /evidence/bench/phases/build-pg-builder.exit)" = "0" ]; then
  cp /target/pg-builder/debug/pg-builder /cargo-home/bin/pg-builder
fi

run_log fetch-dialogue cargo fetch --manifest-path /dialogue-src/tools/ontology-dialogue/Cargo.toml --locked
run_log build-dialogue env CARGO_TARGET_DIR=/target/dialogue cargo build --manifest-path /dialogue-src/tools/ontology-dialogue/Cargo.toml --locked --offline --bin ontology-dialogue
if [ "$(cat /evidence/bench/phases/build-dialogue.exit)" = "0" ]; then
  cp /target/dialogue/debug/ontology-dialogue /cargo-home/bin/ontology-dialogue
fi
run_log fetch-citations cargo fetch --manifest-path /dialogue-src/tools/answer-citations/Cargo.toml --locked
run_log build-citations env CARGO_TARGET_DIR=/target/citations cargo build --manifest-path /dialogue-src/tools/answer-citations/Cargo.toml --locked --offline --bin answer-citations
if [ "$(cat /evidence/bench/phases/build-citations.exit)" = "0" ]; then
  cp /target/citations/debug/answer-citations /cargo-home/bin/answer-citations
fi

run_log fetch-relay cargo fetch --manifest-path /llm-src/testing/real-db/local-llm/Cargo.toml --locked
run_log build-relay env CARGO_TARGET_DIR=/target/relay cargo build --manifest-path /llm-src/testing/real-db/local-llm/Cargo.toml --locked --offline --bin oo-local-llm-relay
if [ "$(cat /evidence/bench/phases/build-relay.exit)" = "0" ]; then
  cp /target/relay/debug/oo-local-llm-relay /cargo-home/bin/oo-local-llm-relay
fi

run_log build-score env CARGO_TARGET_DIR=/target/product-benchmark cargo build --manifest-path /crate/Cargo.toml --locked --offline --bin product-benchmark
if [ "$(cat /evidence/bench/phases/build-score.exit)" = "0" ]; then
  cp /target/product-benchmark/debug/product-benchmark /cargo-home/bin/product-benchmark
fi

# Pinned relay bde5e477 accepts only --listen, --cert, and --key. It forwards
# the caller bearer unchanged and has no client-token mint.
if [ -x /cargo-home/bin/oo-local-llm-relay ] && [ -f /relay/cert.pem ] && [ -f /relay/key.pem ]; then
  set +e
  /cargo-home/bin/oo-local-llm-relay \
    --listen 127.0.0.1:8443 \
    --cert /relay/cert.pem \
    --key /relay/key.pem > /evidence/bench/relay.log 2>&1 &
  relay_pid=$!
  echo "$relay_pid" > /evidence/bench/relay.pid
  sleep 1
  if kill -0 "$relay_pid" 2>/dev/null; then
    record relay-start 0
  else
    record relay-start 1
  fi
  set +e
else
  record relay-start 1
fi

cat > "$BENCH/pg-config.json" <<'EOF'
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
  "secret": {"bearer_file": "/run/llm-token"},
  "declared_price": {"status": "unknown"},
  "budget": {
    "deadline_ms": 30000,
    "max_request_bytes": 262144,
    "max_response_bytes": 262144,
    "max_prompt_tokens": 100000,
    "max_completion_tokens": 8192,
    "max_total_tokens": 200000,
    "max_tables": 16,
    "max_in_flight": 1
  },
  "postgres": {
    "host": "oo-a317-pg",
    "port": 5432,
    "database": "oo_a317_src",
    "username": "oo_a317_reader",
    "password_file": "/secrets/reader.pass",
    "ca_file": "/secrets/ca.crt",
    "catalog_bin": "/cargo-home/a317-bins/oo-pipeline",
    "tables": [
      {"schema": "shop", "table": "party"},
      {"schema": "shop", "table": "account"},
      {"schema": "shop", "table": "entry"}
    ]
  }
}
EOF
cat > "$BENCH/generate-config.json" <<'EOF'
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
  "secret": {"bearer_file": "/run/llm-token"},
  "declared_price": {"status": "unknown"},
  "budget": {
    "deadline_ms": 180000,
    "max_request_bytes": 262144,
    "max_response_bytes": 262144,
    "max_prompt_tokens": 100000,
    "max_completion_tokens": 8192,
    "max_total_tokens": 200000,
    "max_tables": 16,
    "max_in_flight": 1
  }
}
EOF
sed 's/"model_id": "grok-4.7"/"model_id": "grok-4.3"/' "$BENCH/generate-config.json" > "$SCORE/wrong-model-config.json"

if [ -x /cargo-home/bin/pg-builder ]; then
  run_log catalog /cargo-home/bin/pg-builder catalog --config "$BENCH/pg-config.json" --source-id pubfix --secret-ref secret://pg/a317/reader --stderr "$BENCH/catalog.stderr" --out "$BENCH/catalog.json"
  run_log profile /cargo-home/bin/pg-builder profile --config "$BENCH/pg-config.json" --out "$BENCH/profile.json"
  run_log comments /cargo-home/bin/pg-builder comments --config "$BENCH/pg-config.json" --out "$BENCH/comments.json"
  run_log seal /cargo-home/bin/pg-builder seal --catalog "$BENCH/catalog.json" --out "$BENCH/seal.json"
  run_log cards /cargo-home/bin/pg-builder cards --config "$BENCH/pg-config.json" --catalog "$BENCH/catalog.json" --profile "$BENCH/profile.json" --comments "$BENCH/comments.json" --seal "$BENCH/seal.json" --out "$BENCH/cards.json"
  run_log curated /cargo-home/bin/pg-builder curated --catalog "$BENCH/catalog.json" --out "$BENCH/curated.json"
  run_log approve /cargo-home/bin/pg-builder approve --cards "$BENCH/cards.json" --reviewer-label local-operator --reviewer-at 2026-10-07 --out "$BENCH/approval.json"
  if [ "$(cat /evidence/bench/phases/approve.exit)" = "0" ]; then
    run_log generate-first /cargo-home/bin/pg-builder generate --config "$BENCH/generate-config.json" --seal "$BENCH/seal.json" --cards "$BENCH/cards.json" --approval "$BENCH/approval.json" --cache "$BENCH/cache" --out "$BENCH/first.json"
    run_log generate-hit /cargo-home/bin/pg-builder generate --config "$BENCH/generate-config.json" --seal "$BENCH/seal.json" --cards "$BENCH/cards.json" --approval "$BENCH/approval.json" --cache "$BENCH/cache" --out "$BENCH/hit.json"
    [ -s "$BENCH/first.json" ] && cp "$BENCH/first.json" "$SCORE/builder-first.json"
    [ -s "$BENCH/hit.json" ] && cp "$BENCH/hit.json" "$SCORE/builder-hit.json"
  else
    record generate-first 1
    record generate-hit 1
  fi
  set +e
  /cargo-home/bin/pg-builder generate --config "$SCORE/wrong-model-config.json" --out "$BENCH/wrong-model.out" > "$BENCH/wrong-model.stdout" 2> "$BENCH/wrong-model.stderr"
  printf '%s\n' "$?" > "$SCORE/wrong-model.exit"
  set +e
  record wrong-model "$(tr -d '[:space:]' < "$SCORE/wrong-model.exit")"
  run_log bearer-scan /cargo-home/bin/pg-builder scan --root "$BENCH" --bearer-file "$BEARER"
else
  record catalog 1
  record wrong-model 1
fi

if [ -x "$BINS/query-pg" ] && [ -x /cargo-home/bin/oo-a389-prep ] && [ -f "$RUN/queries/wide-count.request.json" ]; then
  run_log party-request /cargo-home/bin/oo-a389-prep request "$RUN/queries/wide-count.request.json" - "SELECT count(*) FROM shop.party" "$RUN/audit/party-count.jsonl" "$BENCH/party-count.request.json"
  if [ "$(cat /evidence/bench/phases/party-request.exit)" = "0" ]; then
    set +e
    "$BINS/query-pg" --request "$BENCH/party-count.request.json" > "$SCORE/query-party-count.stdout" 2> "$BENCH/party-count.stderr"
    printf '%s\n' "$?" > "$SCORE/query-party-count.exit"
    set +e
    record party-count "$(tr -d '[:space:]' < "$SCORE/query-party-count.exit")"
  else
    record party-count 1
  fi
else
  record party-count 1
fi

if [ -x /cargo-home/bin/oo-a389-prep ] && [ -f "$RUN/bundles/wide.json" ] && [ -x /cargo-home/bin/ontology-dialogue ]; then
  run_log pack /cargo-home/bin/oo-a389-prep pack "$RUN/bundles/wide.json" "$BENCH/wide-pack.json"
  printf '%s\n' "{\"protocol\":\"oo.ontology-dialogue.question/v1\",\"text\":\"${QUESTION}\"}" > "$BENCH/question.json"
  run_log propose-config /cargo-home/bin/oo-a389-prep propose-config "$BENCH/wide-pack.json" "$BENCH/question.json" /relay/cert.pem "$BEARER" grok-4.7 https://127.0.0.1:8443 "$BENCH/dialogue-config.json"
  if [ -s "$BENCH/dialogue-config.json" ]; then
    cp "$BENCH/dialogue-config.json" "$SCORE/dialogue-config.json"
  fi
  if [ "$(cat /evidence/bench/phases/propose-config.exit)" = "0" ]; then
    set +e
    /cargo-home/bin/ontology-dialogue propose --config "$BENCH/dialogue-config.json" > "$SCORE/dialogue-proposal.json" 2> "$BENCH/dialogue-proposal.stderr"
    printf '%s\n' "$?" > "$SCORE/dialogue-proposal.exit"
    set +e
    record propose "$(tr -d '[:space:]' < "$SCORE/dialogue-proposal.exit")"
  else
    record propose 1
  fi
  if [ "$(cat "$SCORE/dialogue-proposal.exit" 2>/dev/null || echo 1)" = "0" ]; then
    run_log approval /cargo-home/bin/oo-a389-prep approval "$SCORE/dialogue-proposal.json" "$BENCH/approval-dialogue.json"
    run_log dialogue-request /cargo-home/bin/oo-a389-prep request "$RUN/queries/wide-count.request.json" "$SCORE/dialogue-proposal.json" - "$RUN/audit/dialogue-entry.jsonl" "$BENCH/dialogue.request.json"
    query_sha=$(/cargo-home/bin/sha256 file "$BINS/query-pg" | awk '{print $1}')
    run_log execute-config /cargo-home/bin/oo-a389-prep execute-config "$SCORE/dialogue-proposal.json" "$BENCH/approval-dialogue.json" "$BENCH/wide-pack.json" "$BENCH/dialogue.request.json" "$BINS/query-pg" "$query_sha" "$BENCH/execute-config.json"
    if [ "$(cat /evidence/bench/phases/execute-config.exit)" = "0" ]; then
      set +e
      /cargo-home/bin/ontology-dialogue execute --config "$BENCH/execute-config.json" > "$SCORE/dialogue-execution.json" 2> "$BENCH/dialogue-execution.stderr"
      printf '%s\n' "$?" > "$SCORE/dialogue-execution.exit"
      set +e
      record execute "$(tr -d '[:space:]' < "$SCORE/dialogue-execution.exit")"
    else
      record execute 1
    fi
  else
    record execute 1
  fi
else
  record propose 1
  record execute 1
fi

if [ -x /cargo-home/bin/answer-citations ] && [ -x /cargo-home/bin/oo-a389-prep ] && [ -f "$RUN/audit/wide-count.jsonl" ]; then
  run_log cite-input /cargo-home/bin/oo-a389-prep cite "$RUN/audit/wide-count.jsonl" oo_r_wide pubfix "$BENCH/cite-in.json"
  if [ "$(cat /evidence/bench/phases/cite-input.exit)" = "0" ]; then
    set +e
    /cargo-home/bin/answer-citations < "$BENCH/cite-in.json" > "$SCORE/citation-invalid.json" 2> "$BENCH/citation.stderr"
    printf '%s\n' "$?" > "$SCORE/citation-invalid.exit"
    set +e
    record citation "$(tr -d '[:space:]' < "$SCORE/citation-invalid.exit")"
  else
    record citation 1
  fi
else
  record citation 1
fi

if [ -x /cargo-home/bin/oo-a389-prep ]; then
  run_log whole /cargo-home/bin/oo-a389-prep whole "$SCORE/whole-product.json"
fi

if [ -x /cargo-home/bin/product-benchmark ]; then
  set +e
  /cargo-home/bin/product-benchmark score --freeze /crate/FREEZE.json --input "$SCORE" --out /evidence/report > /evidence/bench/score.log 2>&1
  score_exit=$?
  set +e
  record score "$score_exit"
else
  score_exit=1
  record score 1
fi

if [ -f /evidence/bench/forward.pid ]; then
  kill "$(cat /evidence/bench/forward.pid)" 2>/dev/null || true
fi
if [ -f /evidence/bench/relay.pid ]; then
  kill "$(cat /evidence/bench/relay.pid)" 2>/dev/null || true
fi
printf '%s\n' "$score_exit" > /evidence/bench/bench.exit
echo "bench_exit=$score_exit"
exit "$score_exit"
