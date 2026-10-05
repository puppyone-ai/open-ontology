#!/bin/bash
# Overlay proof: focused policy tests, the live two-table fixture, then clippy.
# Linux arm64 Rust 1.94. One builder. No cargo update. No apt.
# Origin: a200 proof inside.sh 64b70ee4050267d147128abbf254be93151effcb94b67051a5c0b01f878fadf0
# Changes: a218 paths, policy.rs may differ, live oracle required, clippy after, 1 job.
set -u
echo "oo_task=sp06-column-acl-set-proof"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a218"
echo "oo_fixed=c2db9c430b822a824a1062b6c3c7345924df7dd8"
echo "oo_platform=linux/arm64"
echo "phase=proof"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
export OO_A218_LIVE=1
mkdir -p /evidence/linux/proof/phases /evidence/live /cargo-home/bin
cd /src

record() {
  mkdir -p /evidence/linux/proof/phases
  printf '%s\n' "$2" > "/evidence/linux/proof/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/proof/proof.exit
    exit "$2"
  fi
}

if [ ! -f /evidence/linux/smoke/smoke.exit ] || [ "$(cat /evidence/linux/smoke/smoke.exit)" != "0" ]; then
  echo "pre-overlay smoke exit is not 0" >&2
  record smoke-gate 2
fi
record smoke-gate 0

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/linux/proof/rustc-version.txt
cargo -V | tee /evidence/linux/proof/cargo-version.txt
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

if [ ! -f crates/oo-gateway/tests/column_acl_set_pg_live.rs ]; then
  echo "live overlay is not mounted" >&2
  record source 2
fi
if [ ! -f contracts/gateway/column-acl-set-v1.md ]; then
  echo "set contract is not mounted" >&2
  record source 2
fi
if ! grep -q 'install_column_acl_set' crates/oo-gateway/src/policy.rs; then
  echo "policy.rs overlay is missing the set API" >&2
  record source 2
fi
printf '%s\n' "overlay_mounted" > /evidence/linux/proof/overlay-mounted.txt
record source 0

/cargo-home/bin/sha256 file \
  crates/oo-gateway/src/policy.rs \
  crates/oo-gateway/src/lib.rs \
  crates/oo-gateway/src/policy_catalog.rs \
  crates/oo-gateway/src/query_executor.rs \
  crates/oo-gateway/src/sql_guard.rs \
  crates/oo-gateway/Cargo.toml \
  Cargo.lock \
  contracts/gateway/pg-policy-v1.md \
  > /evidence/linux/proof/product-sha256.txt
record product-hash $?
policy_line=$(awk '$2=="crates/oo-gateway/src/policy.rs" {print $1}' /evidence/linux/smoke/owned-sha256.txt)
overlay_policy=$(awk '$2=="crates/oo-gateway/src/policy.rs" {print $1}' /evidence/linux/proof/product-sha256.txt)
if [ -z "$policy_line" ] || [ "$policy_line" = "$overlay_policy" ]; then
  echo "policy.rs does not differ from the pre-overlay smoke" >&2
  record overlay-diff 1
fi
record overlay-diff 0
while read -r hash path; do
  [ -n "$path" ] || continue
  if [ "$path" = "crates/oo-gateway/src/policy.rs" ]; then
    continue
  fi
  if ! awk -v h="$hash" -v p="$path" '$1==h && $2==p {found=1} END {exit !found}' /evidence/linux/proof/product-sha256.txt; then
    echo "frozen file changed $path" >&2
    record frozen-match 1
  fi
done < /evidence/linux/smoke/owned-sha256.txt
guard_got=$(/cargo-home/bin/sha256 file crates/oo-gateway/src/sql_guard.rs | awk '{print $1}')
guard_want=$(cat /evidence/linux/proof/sql-guard.sha256)
if [ "$guard_got" != "$guard_want" ]; then
  echo "sql_guard.rs changed" >&2
  record frozen-match 1
fi
record frozen-match 0

libdir=$(cat /evidence/linux/smoke/libclang-dir.txt)
multiarch=/cargo-home/a218-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a218-clang-root/usr/lib/llvm-19/lib/clang/19/include
devso=$(find /cargo-home/a218-clang-root/usr/lib -name 'libclang.so' -print -quit)
if [ -n "$devso" ]; then
  libdir=$(dirname "$devso")
fi
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a218-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a218-clang-root/usr/share/pkgconfig"
{
  echo "LIBCLANG_PATH=$LIBCLANG_PATH"
  test -f "$llvm_inc/stdbool.h" && echo "stdbool.h=present" || echo "stdbool.h=absent"
} > /evidence/linux/proof/libclang.txt
if [ ! -f "$llvm_inc/stdbool.h" ] || [ -z "$libdir" ]; then
  record libclang 1
fi
record libclang 0
printf '%s\n' "no" > /evidence/linux/proof/cargo-update-invoked.txt

echo "=== cargo test --lib policy::tests ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline \
  -p oo-gateway --lib policy::tests -- --test-threads=1 \
  > /evidence/linux/proof/lib-test.log 2>&1
lib_exit=$?
set -e
record lib-test "$lib_exit"
if ! grep -q 'test result: ok. 6 passed' /evidence/linux/proof/lib-test.log; then
  echo "expected 6 policy unit tests" >&2
  record lib-count 1
fi
record lib-count 0

rm -f /evidence/live/oracle.txt
echo "=== cargo test --test column_acl_set_pg_live ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline \
  -p oo-gateway --test column_acl_set_pg_live -- --test-threads=1 --nocapture \
  > /evidence/linux/proof/test.log 2>&1
test_exit=$?
set -e
record test "$test_exit"
if ! grep -q 'test result: ok. 1 passed' /evidence/linux/proof/test.log; then
  echo "expected the live test to pass" >&2
  record test-count 1
fi
record test-count 0

if [ ! -s /evidence/live/oracle.txt ]; then
  echo "live oracle is missing" >&2
  record oracle 1
fi
while IFS= read -r line; do
  [ -n "$line" ] || continue
  if ! grep -F -x -q "$line" /evidence/live/oracle.txt; then
    echo "oracle missing $line" >&2
    record oracle 1
  fi
done <<'EOF'
legacy_second_table=GrantMismatch
legacy_set_widen=GrantMismatch
legacy_party_columns=1
bad_second_mapping=MappingAbsent
bad_second_role=absent
bad_type=TypeRefused
bad_type_role=absent
digest_a_max=7
digest_b_max=9
stale_cross_digest=StaleGeneration
clerk_effect=Installed
clerk_replay=Replay
peek_effect=Installed
partial_refuse=GrantMismatch
changed_refuse=GrantMismatch
membership_refuse=MembershipMismatch
old_api_one_table_replay=Replay
ledger_grants_after_old_replay=unchanged
table_privileges=0
public_column_privileges=0
metadata_select=false
publication=0
reltuples_before=unknown
reltuples_after=known
query_clerk_exit=0
query_peek_exit=0
query_guard_exit=6
guard_audit=absent
peek_hidden=absent
clerk_rows=present
hidden_write=42501
raw_star=42501
same_backend_after_rollback=true
authentication=not_claimed
rls=open
grantor=oo_fixture_admin
role_shape=nologin
loader_mapping_grant=login_usage_select_execute
EOF
admin=$(tr -d '\n' < /run/oo-a218/fixture/admin.pass)
login=$(tr -d '\n' < /run/oo-a218/fixture/login.pass)
if grep -F -q "$admin" /evidence/live/oracle.txt || grep -F -q "$login" /evidence/live/oracle.txt; then
  echo "oracle contains a password" >&2
  record oracle 1
fi
unset admin login
record oracle 0

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/linux/proof/clippy-component.log 2>&1
  clippy_add=$?
  set -e
  record clippy-component "$clippy_add"
else
  echo "clippy_component=already_installed" > /evidence/linux/proof/clippy-component.log
  record clippy-component 0
fi
echo "=== cargo clippy -p oo-gateway --lib --test column_acl_set_pg_live ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline \
  -p oo-gateway --lib --test column_acl_set_pg_live -- -D warnings \
  > /evidence/linux/proof/clippy.log 2>&1
clippy_exit=$?
set -e
record clippy "$clippy_exit"

/cargo-home/bin/sha256 file Cargo.lock | awk '{print $1}' > /evidence/linux/proof/lock-after.sha256
before=$(awk '$2=="Cargo.lock" {print $1}' /evidence/linux/smoke/owned-sha256.txt)
after=$(cat /evidence/linux/proof/lock-after.sha256)
if [ "$before" != "$after" ]; then
  echo "lock changed before=$before after=$after" > /evidence/linux/proof/lock-hash.txt
  record lock-hash 1
fi
echo "unchanged $after" > /evidence/linux/proof/lock-hash.txt
record lock-hash 0

printf '%s\n' 0 > /evidence/linux/proof/proof.exit
echo "proof=ok"
exit 0
