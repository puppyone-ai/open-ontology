#!/bin/bash
# Post-edit focused lib, clippy, gated return, and the live catalog fixture.
# Linux arm64 Rust 1.94. One builder. No cargo update.
set -u
echo "oo_task=sp06-catalog-test-alignment-green"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a206"
echo "oo_platform=linux/arm64"
echo "phase=postedit"
uname -a
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=4
mkdir -p /evidence/linux/green/phases /evidence/live /cargo-home/bin
cd /src

record() {
  mkdir -p /evidence/linux/green/phases
  printf '%s\n' "$2" > "/evidence/linux/green/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/green/green.exit
    exit "$2"
  fi
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/linux/green/rustc-version.txt
cargo -V | tee /evidence/linux/green/cargo-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac

if [ ! -x /cargo-home/bin/sha256 ]; then
  rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256
  record sha256-tool $?
else
  record sha256-tool 0
fi
export PATH="/cargo-home/bin:${PATH}"

if ! grep -q 'Some(self.clerk_role.as_str())' crates/oo-gateway/tests/policy_catalog_pg_live.rs; then
  echo "guard refusal does not assert the bound clerk role" >&2
  record assertion 2
fi
if ! grep -q 'ExecError::RoleReuse' crates/oo-gateway/tests/policy_catalog_pg_live.rs; then
  echo "RoleReuse assertion is missing" >&2
  record assertion 2
fi
if ! grep -q 'Some(self.auditor_role.as_str())' crates/oo-gateway/tests/policy_catalog_pg_live.rs; then
  echo "fresh session does not assert the other role" >&2
  record assertion 2
fi
if ! grep -q 'config digest' crates/oo-gateway/src/policy.rs; then
  echo "generation comment was not updated" >&2
  record assertion 2
fi
if ! grep -q 'does not prove the session is idle' contracts/gateway/policy-catalog-v1.md; then
  echo "xid weakness is missing from the contract" >&2
  record assertion 2
fi
record assertion 0

/cargo-home/bin/sha256 file \
  crates/oo-gateway/src/policy_catalog.rs \
  crates/oo-gateway/src/query_executor.rs \
  Cargo.lock \
  > /evidence/linux/green/unchanged-sha256.txt
record unchanged-hash $?
for rel in crates/oo-gateway/src/policy_catalog.rs crates/oo-gateway/src/query_executor.rs Cargo.lock; do
  want=$(awk -v rel="$rel" '$2==rel {print $1}' /evidence/linux/smoke/owned-sha256.txt)
  got=$(awk -v rel="$rel" '$2==rel {print $1}' /evidence/linux/green/unchanged-sha256.txt)
  if [ "$want" != "$got" ]; then
    echo "product file changed $rel smoke=$want green=$got" > /evidence/linux/green/unchanged.txt
    record unchanged 1
  fi
done
echo "executor catalog lock unchanged" > /evidence/linux/green/unchanged.txt
record unchanged 0

libdir=$(cat /evidence/linux/smoke/libclang-dir.txt)
multiarch=/cargo-home/a206-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a206-clang-root/usr/lib/llvm-19/lib/clang/19/include
devso=$(find /cargo-home/a206-clang-root/usr/lib -name 'libclang.so' -print -quit)
if [ -n "$devso" ]; then
  libdir=$(dirname "$devso")
fi
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a206-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a206-clang-root/usr/share/pkgconfig"
if [ ! -f "$llvm_inc/stdbool.h" ] || [ -z "$libdir" ]; then
  record libclang 1
fi
record libclang 0
printf '%s\n' "no" > /evidence/linux/green/cargo-update-invoked.txt

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/linux/green/clippy-component.log 2>&1
  clippy_add=$?
  set -e
  record clippy-component "$clippy_add"
else
  echo "clippy_component=already_installed" > /evidence/linux/green/clippy-component.log
  record clippy-component 0
fi
if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^rustfmt'; then
  set +e
  rustup component add rustfmt --toolchain 1.94.0 > /evidence/linux/green/rustfmt-component.log 2>&1
  rustfmt_add=$?
  set -e
  record rustfmt-component "$rustfmt_add"
else
  echo "rustfmt_component=already_installed" > /evidence/linux/green/rustfmt-component.log
  record rustfmt-component 0
fi

echo "=== rustfmt the edited test ==="
set +e
rustfmt --edition 2024 --check \
  crates/oo-gateway/tests/policy_catalog_pg_live.rs \
  > /evidence/linux/green/rustfmt.log 2>&1
fmt_exit=$?
set -e
record rustfmt "$fmt_exit"
printf '%s\n' 'policy.rs rustfmt --check rewrites pre-existing lines. Attempt 1 kept that log. This run checks the test file only.' \
  > /evidence/linux/green/rustfmt-scope.txt

echo "=== cargo test -p oo-gateway --lib ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib -- --test-threads=4 \
  > /evidence/linux/green/test.log 2>&1
test_exit=$?
set -e
record test "$test_exit"

echo "=== cargo clippy -p oo-gateway --lib ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib -- -D warnings \
  > /evidence/linux/green/clippy.log 2>&1
clippy_exit=$?
set -e
record clippy "$clippy_exit"

echo "=== cargo clippy catalog live test ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline \
  -p oo-gateway --test policy_catalog_pg_live -- -D warnings \
  > /evidence/linux/green/clippy-test.log 2>&1
clippy_test_exit=$?
set -e
record clippy-test "$clippy_test_exit"

echo "=== gated catalog test ==="
set +e
env -u OO_A190_LIVE cargo test --manifest-path Cargo.toml --locked --offline \
  -p oo-gateway --test policy_catalog_pg_live -- --test-threads=1 --nocapture \
  > /evidence/linux/green/gated.log 2>&1
gated_exit=$?
set -e
record gated "$gated_exit"
if ! grep -q 'OO_A190_LIVE is unset; the policy catalog comparison was not run' /evidence/linux/green/gated.log; then
  record gated-message 1
fi
if grep -Eq 'panicked at|query failed' /evidence/linux/green/gated.log; then
  record gated-message 1
fi
printf '%s\n' 'cargo_ok_means_gate_returned comparison_not_run=1' > /evidence/linux/green/gated-note.txt
record gated-message 0

echo "=== OO_A190_LIVE=1 ==="
set +e
OO_A190_LIVE=1 cargo test --manifest-path Cargo.toml --locked --offline \
  -p oo-gateway --test policy_catalog_pg_live -- --test-threads=1 --nocapture \
  > /evidence/linux/green/live.log 2>&1
live_exit=$?
set -e
record live "$live_exit"
if [ -f /evidence/live/oracle.txt ]; then
  cp /evidence/live/oracle.txt /evidence/live/green-oracle.txt
fi
if ! grep -q 'guard_bound_role=clerk' /evidence/live/green-oracle.txt; then
  record oracle 1
fi
if ! grep -q 'guard_audit_delta=0' /evidence/live/green-oracle.txt; then
  record oracle 1
fi
if ! grep -q 'role_reuse=refused' /evidence/live/green-oracle.txt; then
  record oracle 1
fi
if ! grep -q 'fresh_other_role=auditor' /evidence/live/green-oracle.txt; then
  record oracle 1
fi
if ! grep -q 'done=ok' /evidence/live/green-oracle.txt; then
  record oracle 1
fi
record oracle 0

/cargo-home/bin/sha256 file Cargo.lock | awk '{print $1}' > /evidence/linux/green/lock-after.sha256
before=$(awk '$2 ~ /Cargo.lock$/ {print $1}' /evidence/linux/smoke/owned-sha256.txt)
after=$(cat /evidence/linux/green/lock-after.sha256)
if [ "$before" != "$after" ]; then
  echo "lock changed before=$before after=$after" > /evidence/linux/green/lock-hash.txt
  record lock-hash 1
fi
echo "unchanged $after" > /evidence/linux/green/lock-hash.txt
record lock-hash 0

printf '%s\n' 0 > /evidence/linux/green/green.exit
echo "postedit=ok"
exit 0
