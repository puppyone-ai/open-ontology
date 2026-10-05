#!/bin/bash
# Review proof on fixed 54358076 plus the mounted a229 test.
# Linux arm64 Rust 1.94. Product bytes must match the smoke hashes.
# Origin: a200 proof inside.sh 64b70ee4050267d147128abbf254be93151effcb94b67051a5c0b01f878fadf0
# Changes: product bytes must match smoke, live oracle is the review fixture, clippy after, 1 job.
set -u
echo "oo_task=sp06-column-acl-set-review-proof"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a229"
echo "oo_fixed=543580768dec60d127eb428c09e0a9a400b18272"
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
export OO_A229_LIVE=1
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

if [ ! -f crates/oo-gateway/tests/column_acl_set_a229_review.rs ]; then
  echo "review overlay is not mounted" >&2
  record source 2
fi
if ! grep -q 'install_column_acl_set' crates/oo-gateway/src/policy.rs; then
  echo "fixed policy.rs is missing the set API" >&2
  record source 2
fi
printf '%s\n' "overlay_mounted" > /evidence/linux/proof/overlay-mounted.txt
record source 0

/cargo-home/bin/sha256 file \
  crates/oo-gateway/src/policy.rs \
  crates/oo-gateway/src/lib.rs \
  crates/oo-gateway/tests/column_acl_set_pg_live.rs \
  contracts/gateway/column-acl-set-v1.md \
  Cargo.lock \
  crates/oo-gateway/Cargo.toml \
  > /evidence/linux/proof/product-sha256.txt
record product-hash $?
if ! cmp -s /evidence/linux/proof/product-sha256.txt /evidence/linux/smoke/owned-sha256.txt; then
  echo "product bytes differ from the pre-overlay smoke" >&2
  record product-match 1
fi
record product-match 0
/cargo-home/bin/sha256 file crates/oo-gateway/tests/column_acl_set_a229_review.rs \
  > /evidence/linux/proof/overlay.sha256
record overlay-hash $?

libdir=$(cat /evidence/linux/smoke/libclang-dir.txt)
multiarch=/cargo-home/a229-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a229-clang-root/usr/lib/llvm-19/lib/clang/19/include
devso=$(find /cargo-home/a229-clang-root/usr/lib -name 'libclang.so' -print -quit)
if [ -n "$devso" ]; then
  libdir=$(dirname "$devso")
fi
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a229-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a229-clang-root/usr/share/pkgconfig"
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
echo "=== cargo test --test column_acl_set_a229_review ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline \
  -p oo-gateway --test column_acl_set_a229_review -- --test-threads=1 --nocapture \
  > /evidence/linux/proof/test.log 2>&1
test_exit=$?
set -e
record test "$test_exit"
if ! grep -q 'test result: ok. 1 passed' /evidence/linux/proof/test.log; then
  echo "expected the live review test to pass" >&2
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
pure_refusals=empty_duplicate_mixed_filter_metadata_unknown_bounds
pure_grant_events=0
bad_second_mapping=MappingAbsent
bad_second_role=absent
bad_second_grants=0
bad_second_grant_events=0
bad_second_timing=pre_create_role
bad_type=TypeRefused
bad_type_role=absent
bad_type_grants=0
bad_type_grant_events=0
bad_type_timing=pre_create_role
login_absent=LoginAbsent
login_absent_grant_events=0
digest_a_max=7
digest_b_max=11
stale_cross_digest=StaleGeneration
stale_grant_events=0
legacy_second_table=GrantMismatch
legacy_set_widen=GrantMismatch
legacy_counter_columns=1
scribe_effect=Installed
glance_effect=Installed
scribe_replay=Replay
old_api_one_table_replay=Replay
slip_grants_after_old_replay=unchanged
partial_refuse=GrantMismatch
changed_refuse=GrantMismatch
unexpected_grant=left_in_place
membership_refuse=MembershipMismatch
grants_after_refuse=unchanged
post_grant_fault=raised
post_grant_sqlstate=P0001
post_grant_role=absent
post_grant_grants=0
post_grant_timing=after_earlier_grants
post_grant_not_pregrant=true
post_grant_target=second_table
post_grant_events_match=true
same_backend_after_fault=true
grantor=oo_fixture_admin
membership_grantor=oo_fixture_admin
nonsuperuser_grantor=not_proven
role_shape=nologin
membership=inherit=false set=true admin=false
table_privileges=0
metadata_select=false
schema_usage=usage_without_create
publication=0
reltuples_before=unknown
reltuples_after=known
loader_mapping_grant=fixture_login_not_installer
query_scribe_exit=0
query_glance_exit=0
glance_hidden=absent
query_guard_exit=6
guard_audit=absent
hidden_write=42501
raw_star=42501
reader_pid_stable=true
authentication=not_claimed
rls=open
audit_wal=open
EOF
installed=$(awk -F= '$1=="installed_grant_events" {print $2}' /evidence/live/oracle.txt)
posted=$(awk -F= '$1=="post_grant_events" {print $2}' /evidence/live/oracle.txt)
if [ -z "$installed" ] || [ "$installed" != "$posted" ] || [ "$installed" -lt 2 ]; then
  echo "grant event counts installed=$installed posted=$posted" >&2
  record oracle 1
fi
secret_hit=0
for secret in /run/oo-a229/fixture/admin.pass /run/oo-a229/fixture/login.pass /run/oo-a229/fixture/other.pass; do
  if grep -R -F -q -f "$secret" \
    /evidence/live/oracle.txt \
    /evidence/linux/proof/test.log \
    /evidence/linux/proof/lib-test.log \
    /evidence/live; then
    secret_hit=1
  fi
done
if [ "$secret_hit" -ne 0 ]; then
  echo "a fixture secret appeared in the oracle or test output" >&2
  record oracle 1
fi
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
echo "=== cargo clippy -p oo-gateway --lib --test column_acl_set_a229_review --test column_acl_set_pg_live ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline \
  -p oo-gateway --lib --test column_acl_set_a229_review --test column_acl_set_pg_live -- -D warnings \
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
