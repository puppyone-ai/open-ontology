#!/bin/bash
# Run the acceptance card on the clean archive, then try one locked build.
# No cargo update. No customer call. No foreign database. Deadline 16 minutes.
set -u
export HOME=/tmp
export CARGO_HOME=/cargo
export CARGO_TARGET_DIR=/producer-target
export CARGO_BUILD_JOBS=1
export CARGO_TERM_COLOR=never
export TMPDIR=/producer-target/tmp
export RUSTUP_TOOLCHAIN=1.94.0
export RUSTUP_HOME=/usr/local/rustup
EV=/evidence
CARD=/target/debug/minimal-loop-acceptance
mkdir -p "$EV/phases" "$EV/card" "$TMPDIR" /tmp/work
deadline=$((SECONDS + 960))
rustc --version > "$EV/rustc-version.txt"
COMMIT=$(tr -d '[:space:]' < /archive/COMMIT)
printf '%s\n' "$COMMIT" > "$EV/phases/archive-commit.txt"

run_card() {
  out=$1
  shift
  mkdir -p "$out"
  "$CARD" card --repo /src --archive /archive --out "$out" --pinned-commit "$COMMIT" "$@" > "$out/card.stdout" 2> "$out/card.stderr"
  status=$?
  printf '%s\n' "$status" > "$out/card.exit"
  echo "PHASE card=$status out=$out"
  return "$status"
}

if [ ! -x "$CARD" ]; then
  echo "acceptance binary is absent" > "$EV/phases/card-missing.txt"
  printf '%s\n' 4 > "$EV/proof.exit"
  exit 4
fi
set +e
run_card "$EV/card"
card_exit=$?
set +e
printf '%s\n' "$card_exit" > "$EV/phases/card.exit"

build_exit=127
bin=/producer-target/debug/minimal-product-loop
remain=$((deadline - SECONDS - 240))
if [ "$remain" -lt 60 ]; then
  echo "no budget for the producer build" > "$EV/phases/producer-build.stderr"
  build_exit=124
else
  cd /archive/tools/minimal-product-loop
  timeout -k 20s "${remain}s" cargo build --locked --bin minimal-product-loop > "$EV/phases/producer-build.stdout" 2> "$EV/phases/producer-build.stderr"
  build_exit=$?
fi
printf '%s\n' "$build_exit" > "$EV/phases/producer-build.exit"
echo "PHASE producer-build=$build_exit"
if [ "$build_exit" -ne 0 ] || [ ! -f "$bin" ]; then
  printf '%s\n' "producer build did not produce a git-archive binary; execute skipped" > "$EV/failed-attempts.txt"
  printf '%s\n' "$card_exit" > "$EV/proof.exit"
  exit "$card_exit"
fi

sha=$(sha256sum "$bin" | awk 'NR==1 {print $1}')
printf '%s\n' "{\"commit\":\"${COMMIT}\",\"built_from\":\"git-archive\",\"binary_sha256\":\"${sha}\"}" > /tmp/work/manifest.json
install -m 0600 /dev/null /tmp/work/password
install -m 0600 /dev/null /tmp/work/bearer
install -m 0600 /dev/null /tmp/work/token
printf '%s\n' "fixture-not-a-customer-secret" > /tmp/work/password
printf '%s\n' "not-a-live-token" > /tmp/work/bearer
printf '%s\n' "not-a-live-token" > /tmp/work/token
chmod 0600 /tmp/work/password /tmp/work/bearer /tmp/work/token
mkdir -p /tmp/work/work /tmp/work/knowledge
pi_sha=$(sha256sum /bin/true | awk 'NR==1 {print $1}')
node_sha=$(sha256sum /bin/true | awk 'NR==1 {print $1}')
cat > /tmp/work/config.json <<EOF
{
  "protocol": "oo.minimal-product-loop.config/v1",
  "mode": "operator-development",
  "development_preview": true,
  "publication": "not_published",
  "llm_review": "unreviewed",
  "runtime_acknowledgement": "local-process-not-a-sandbox-certificate",
  "database": {
    "type": "postgresql",
    "host": "127.0.0.1",
    "port": 9,
    "name": "oo_a407_accept",
    "username": "reader",
    "tls": "require",
    "secret_ref": "secret://pg/a407/accept",
    "password_file": "/tmp/work/password",
    "ca_file": "/tmp/work/password"
  },
  "tables": [
    {"schema": "shop", "table": "party"},
    {"schema": "shop", "table": "account"},
    {"schema": "shop", "table": "entry"}
  ],
  "llm": {
    "model_id": "grok-4.7",
    "origin": "http://127.0.0.1:9",
    "path": "/v1/chat/completions",
    "bearer_file": "/tmp/work/bearer",
    "deadline_ms": 1000
  },
  "gateway": {
    "oo": "/bin/true",
    "query_pg": "/bin/true",
    "policy_file": "/tmp/work/config.json",
    "token_file": "/tmp/work/token"
  },
  "pi": {
    "executable": "/bin/true",
    "executable_sha256": "${pi_sha}",
    "runtime": "/bin/true",
    "runtime_sha256": "${node_sha}",
    "provider": "grok",
    "model": "grok-4.7"
  },
  "paths": {
    "catalog_bin": "/bin/true",
    "knowledge": "/tmp/work/knowledge",
    "work": "/tmp/work/work"
  },
  "operator": {
    "label": "a407",
    "at": "2026-10-08T00:00:00Z"
  }
}
EOF
chmod 0600 /tmp/work/config.json /tmp/work/manifest.json
set +e
run_card "$EV/card-execute" --binary "$bin" --manifest /tmp/work/manifest.json --config /tmp/work/config.json --execute
exec_exit=$?
set +e
printf '%s\n' "$exec_exit" > "$EV/phases/card-execute.exit"
rm -f /tmp/work/password /tmp/work/bearer /tmp/work/token
printf '%s\n' "$exec_exit" > "$EV/proof.exit"
exit "$exec_exit"
