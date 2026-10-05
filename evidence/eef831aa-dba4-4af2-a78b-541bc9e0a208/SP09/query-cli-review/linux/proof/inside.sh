#!/bin/bash
# Proof on the fixed a661dcdf tree plus the review test file only.
# Linux arm64 Rust 1.94. No cargo update. No apt. No product edits.
set -u
echo "oo_task=sp09-query-cli-review-proof"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a208"
echo "oo_fixed=a661dcdf72ff91561ae6b7cb79916e582884d08a"
echo "oo_platform=linux/arm64"
echo "phase=proof"
uname -a
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=4
mkdir -p /evidence/linux/proof/phases /evidence/live /cargo-home/bin
cd /src
fail=0

note() {
  mkdir -p /evidence/linux/proof/phases
  printf '%s\n' "$2" > "/evidence/linux/proof/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ] && [ "$fail" -eq 0 ]; then
    fail=$2
  fi
}

stop() {
  printf '%s\n' "$1" > /evidence/linux/proof/proof.exit
  exit "$1"
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/linux/proof/rustc-version.txt
case "$got" in
  "rustc 1.94.0 "*) note toolchain 0 ;;
  *) echo "refusing toolchain" >&2; note toolchain 2; stop 2 ;;
esac

if [ ! -x /cargo-home/bin/sha256 ]; then
  rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256
  note sha256-tool $?
else
  note sha256-tool 0
fi
rustc --edition 2021 /tools/secret-scan.rs -o /cargo-home/bin/secret-scan
note secret-scan-tool $?
export PATH="/cargo-home/bin:${PATH}"

if [ ! -f crates/oo-gateway/tests/query_pg_cli_a208_review.rs ]; then
  echo "review overlay is missing" >&2
  note overlay-present 2
  stop 2
fi
note overlay-present 0
/cargo-home/bin/sha256 file crates/oo-gateway/tests/query_pg_cli_a208_review.rs \
  > /evidence/linux/proof/overlay.sha256

/cargo-home/bin/sha256 file \
  crates/oo-gateway/src/bin/query-pg.rs \
  crates/oo-gateway/tests/query_pg_cli.rs \
  contracts/gateway/query-pg-cli-v1.md \
  crates/oo-gateway/Cargo.toml \
  Cargo.lock \
  crates/oo-gateway/src/query_executor.rs \
  crates/oo-gateway/src/policy.rs \
  crates/oo-gateway/src/policy_catalog.rs \
  crates/oo-gateway/tests/policy_catalog_pg_live.rs \
  contracts/gateway/policy-catalog-v1.md \
  > /evidence/linux/proof/product-sha256.txt
if ! cmp -s /evidence/linux/proof/product-sha256.txt /evidence/linux/smoke/owned-sha256.txt; then
  echo "product bytes differ from the pre-overlay smoke" >&2
  note product-hash 2
  stop 2
fi
note product-hash 0

if [ ! -d /cargo-home/a208-clang-root/usr/lib ]; then
  echo "libclang root from smoke is missing" >&2
  note libclang-reuse 2
  stop 2
fi
note libclang-reuse 0
libdir=$(cat /evidence/linux/smoke/libclang-dir.txt)
multiarch=/cargo-home/a208-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a208-clang-root/usr/lib/llvm-19/lib/clang/19/include
real=$(find /cargo-home/a208-clang-root/usr/lib -name 'libclang-19.so.19' -print -quit)
devso=$(find /cargo-home/a208-clang-root/usr/lib -name 'libclang.so' -print -quit)
export LIBCLANG_PATH="${devso:+$(dirname "$devso")}"
if [ -z "${LIBCLANG_PATH}" ]; then
  export LIBCLANG_PATH="$libdir"
fi
export LD_LIBRARY_PATH="${LIBCLANG_PATH}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
printf '%s\n' "no" > /evidence/linux/proof/cargo-update-invoked.txt

echo "=== OO_A190_LIVE=1 policy_catalog_pg_live ==="
set +e
env OO_A190_LIVE=1 cargo test --manifest-path Cargo.toml --locked --offline \
  -p oo-gateway --test policy_catalog_pg_live -- --test-threads=1 --nocapture \
  > /evidence/linux/proof/a190-live.log 2>&1
a190=$?
set -e
note a190-live "$a190"
if [ "$a190" -eq 0 ]; then
  missing=0
  for key in \
    guard_bound_role=clerk \
    guard_audit_delta=0 \
    role_reuse=refused \
    fresh_other_role=auditor \
    fresh_read_audit=1 \
    done=ok
  do
    if ! grep -q "^${key}$" /evidence/live/oracle.txt; then
      echo "missing oracle key ${key%%=*}" >> /evidence/linux/proof/a190-oracle.txt
      missing=1
    fi
  done
  if [ "$missing" -eq 0 ]; then
    printf '%s\n' "keys=present" > /evidence/linux/proof/a190-oracle.txt
  fi
  note a190-oracle "$missing"
else
  printf '%s\n' "test_failed" > /evidence/linux/proof/a190-oracle.txt
  note a190-oracle 1
fi

echo "=== OO_A208_LIVE unset ==="
set +e
env -u OO_A208_LIVE -u OO_A190_LIVE -u OO_A198_LIVE \
  cargo test --manifest-path Cargo.toml --locked --offline \
  -p oo-gateway --test query_pg_cli_a208_review -- --test-threads=1 --nocapture \
  > /evidence/linux/proof/a208-gate.log 2>&1
gate=$?
set -e
note a208-gate "$gate"
if grep -q 'OO_A208_LIVE is unset; the review comparison was not run' /evidence/linux/proof/a208-gate.log \
  && ! grep -Eq 'panicked at|query failed' /evidence/linux/proof/a208-gate.log
then
  note a208-gate-message 0
else
  note a208-gate-message 1
fi

echo "=== OO_A208_LIVE=1 fresh fixture ==="
set +e
env OO_A208_LIVE=1 \
  cargo test --manifest-path Cargo.toml --locked --offline \
  -p oo-gateway --test query_pg_cli_a208_review -- --test-threads=1 --nocapture \
  > /evidence/linux/proof/a208-live.log 2>&1
live=$?
set -e
note a208-live "$live"
if [ -f /evidence/live/cli-oracle.txt ] && grep -q '^done=ok$' /evidence/live/cli-oracle.txt; then
  printf '%s\n' "done=ok" > /evidence/linux/proof/a208-oracle.txt
  note a208-oracle 0
else
  printf '%s\n' "done_missing" > /evidence/linux/proof/a208-oracle.txt
  note a208-oracle 1
fi

echo "=== clippy component ==="
if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/linux/proof/clippy-component.log 2>&1
  clippy_add=$?
  set -e
  note clippy-component "$clippy_add"
else
  echo "clippy_component=already_installed" > /evidence/linux/proof/clippy-component.log
  note clippy-component 0
fi

echo "=== clippy lib bin and review test ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline \
  -p oo-gateway --lib --bin query-pg --test query_pg_cli_a208_review -- -D warnings \
  > /evidence/linux/proof/clippy.log 2>&1
clippy=$?
set -e
note clippy "$clippy"

/cargo-home/bin/sha256 file Cargo.lock | awk '{print $1}' > /evidence/linux/proof/lock-after.sha256
before=$(awk '$2 ~ /Cargo.lock$/ {print $1}' /evidence/linux/smoke/owned-sha256.txt)
after=$(cat /evidence/linux/proof/lock-after.sha256)
if [ "$before" = "$after" ]; then
  echo "unchanged $after" > /evidence/linux/proof/lock-hash.txt
  note lock-hash 0
else
  echo "changed" > /evidence/linux/proof/lock-hash.txt
  note lock-hash 1
fi

scan_files=()
for candidate in \
  /evidence/live/oracle.txt \
  /evidence/live/cli-oracle.txt \
  /evidence/linux/proof/a190-live.log \
  /evidence/linux/proof/a208-gate.log \
  /evidence/linux/proof/a208-live.log \
  /evidence/linux/proof/clippy.log
do
  if [ -f "$candidate" ]; then
    scan_files+=("$candidate")
  fi
done
set +e
/cargo-home/bin/secret-scan \
  /run/oo-a208/fixture/admin.pass \
  /run/oo-a208/fixture/login.pass \
  /evidence/linux/proof/secret-scan.txt \
  "${scan_files[@]}"
scan=$?
set -e
note secret-scan "$scan"

printf '%s\n' "$fail" > /evidence/linux/proof/proof.exit
echo "proof_fail=$fail"
exit "$fail"
