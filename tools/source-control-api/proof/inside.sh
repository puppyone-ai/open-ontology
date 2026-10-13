#!/bin/bash
# Build pinned producers and this child, then run the live proof. Writes only
# the child lock and this card's evidence directory.
set -eu
cd /src
export GIT_CONFIG_COUNT=1
export GIT_CONFIG_KEY_0=safe.directory
export GIT_CONFIG_VALUE_0=/src
EVIDENCE=/src/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a383/SP03/source-control-api
mkdir -p "$EVIDENCE/raw" "$EVIDENCE/failed-attempts"
export OO_A383_LIVE=1
export OO_A383_PG_HOST=oo-a383-pg
export OO_A383_PG_PORT=5432
export OO_A383_SECRETS=/secrets
export OO_A383_BINS=/cargo/target/debug
export OO_A383_WORK=/tmp/oo-a383-work
export OO_A383_EVIDENCE="$EVIDENCE"
export OO_A383_IDP_DER=/secrets/idp.der
export OO_A383_HEAD
OO_A383_HEAD=$(git rev-parse HEAD)
printf '%s\n' "$OO_A383_HEAD" > "$EVIDENCE/raw/head.txt"

redact() {
  local file=$1
  local secret name
  [ -f "$file" ] || return 0
  for name in admin.pass api.pass identity.pass reader.pass writer.pass runtime.pass client.secret; do
    secret=/secrets/$name
    [ -s "$secret" ] || continue
    if grep -F -q -f "$secret" "$file"; then
      printf '%s\n' redacted > "$file"
      return 1
    fi
  done
  if [ -s /tmp/oo-a383-work/bootstrap.token ] && grep -F -q -f /tmp/oo-a383-work/bootstrap.token "$file"; then
    printf '%s\n' redacted > "$file"
    return 1
  fi
}

finish() {
  if [ -n "${HOST_UID:-}" ] && [ -n "${HOST_GID:-}" ]; then
    chown -R "${HOST_UID}:${HOST_GID}" /src/tools/source-control-api/Cargo.lock "$EVIDENCE" 2>/dev/null || true
  fi
}
trap finish EXIT

{
  echo "head=$OO_A383_HEAD"
  if git diff --quiet b60f954897a6d305e72bd5deb82871474ddaae57 -- tools/file-import-pg; then echo file-import-pg=empty; else echo file-import-pg=differs; fi
  if git diff --quiet 50d4e705a2bb4ba014919ef1e071da065cf8e9b7 -- tools/pg-onboarding; then echo pg-onboarding=empty; else echo pg-onboarding=differs; fi
  if git diff --quiet a6f92da8a0bf872f8a30efb116c2c7b14463505b -- tools/control-identity; then echo control-identity-worktree=empty; else echo control-identity-worktree=differs; fi
  echo "control-identity-build=a6f92da8a0bf872f8a30efb116c2c7b14463505b"
} > "$EVIDENCE/raw/pin-diff.txt"

run_step() {
  local name=$1
  shift
  set +e
  "$@" > "$EVIDENCE/raw/${name}.log" 2>&1
  local code=$?
  set -e
  printf '%s\n' "$code" > "$EVIDENCE/raw/${name}.exit"
  if ! redact "$EVIDENCE/raw/${name}.log"; then
    printf '%s\n' "A383_FAILED step=$name redact" 
    exit 1
  fi
  if [ "$code" -ne 0 ]; then
    tail -n 200 "$EVIDENCE/raw/${name}.log" > "$EVIDENCE/failed-attempts/${name}.tail" || true
    printf '%s\n' "A383_FAILED step=$name exit=$code"
    exit "$code"
  fi
}

if [ ! -s /secrets/idp.der ]; then
  openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -out /tmp/idp.pem
  openssl pkcs8 -topk8 -nocrypt -outform DER -in /tmp/idp.pem -out /secrets/idp.der
  rm -f /tmp/idp.pem
fi

if [ ! -f /src/tools/source-control-api/Cargo.lock ]; then
  run_step lock cargo build --manifest-path /src/tools/source-control-api/Cargo.toml --bin source-control-api --tests
fi

run_step build-root cargo build --locked --manifest-path /src/Cargo.toml -p oo-pipeline --bin init-pg --bin oo-pipeline -p oo-builder --bin oo-builder -p oo-connectors --bin oo-file-parser-worker
run_step build-file-import cargo build --locked --manifest-path /src/tools/file-import-pg/Cargo.toml --bin file-import-pg
run_step build-pg-onboarding cargo build --locked --manifest-path /src/tools/pg-onboarding/Cargo.toml --bin pg-onboarding
rm -rf /tmp/oo-a383-pin
mkdir -p /tmp/oo-a383-pin
git archive a6f92da8a0bf872f8a30efb116c2c7b14463505b tools/control-identity | tar -x -C /tmp/oo-a383-pin
test -f /tmp/oo-a383-pin/tools/control-identity/Cargo.lock
run_step build-identity cargo build --locked --manifest-path /tmp/oo-a383-pin/tools/control-identity/Cargo.toml --bin control-identity
run_step build-api cargo build --locked --manifest-path /src/tools/source-control-api/Cargo.toml --bin source-control-api --tests
run_step clippy cargo clippy --locked --manifest-path /src/tools/source-control-api/Cargo.toml --all-targets -- -D warnings
run_step test cargo test --locked --manifest-path /src/tools/source-control-api/Cargo.toml --lib --test contract --test live -- --test-threads=1

if ! grep -F -q '"rows_100k":100000' "$EVIDENCE/live-summary.json"; then
  printf '%s\n' "A383_FAILED step=summary"
  exit 1
fi
printf '%s\n' "A383_DONE exit=0"
