#!/bin/bash
# Overlay proof: install-column-acl CLI tests, then clippy on the owned targets.
# Linux arm64 Rust 1.94. One builder. No cargo update. No apt.
# Origin: a218 proof inside.sh (column-acl-set/linux/proof/inside.sh).
# Changes: a235 oracle, frozen files must match smoke, clippy on the bin and test.
set -u
echo "oo_task=sp06-column-acl-cli-proof"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a235"
echo "oo_fixed=a48db85fc13148874349d318affd995f7212188d"
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
export OO_A235_LIVE=1
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

if [ ! -f crates/oo-gateway/src/bin/install-column-acl.rs ]; then
  echo "cli overlay is not mounted" >&2
  record source 2
fi
if [ ! -f crates/oo-gateway/tests/install_column_acl_cli.rs ]; then
  echo "cli test is not mounted" >&2
  record source 2
fi
if [ ! -f contracts/gateway/install-column-acl-cli-v1.md ]; then
  echo "cli contract is not mounted" >&2
  record source 2
fi
if [ -f rust-toolchain.toml ]; then
  echo "proof tree still selects a toolchain file" >&2
  record source 2
fi
if [ -e /run/oo-a235/fixture/server.key ] || [ -e /run/oo-a235/fixture/ca.key ] || [ -e /run/oo-a235/fixture/server.csr ]; then
  echo "builder fixture contains a private key" >&2
  record fixture-keys 2
fi
find /run/oo-a235/fixture -type f -print | sort > /evidence/linux/proof/fixture-files.txt
printf '%s\n' "overlay_mounted" > /evidence/linux/proof/overlay-mounted.txt
record fixture-keys 0
record source 0

/cargo-home/bin/sha256 file \
  crates/oo-gateway/src/policy.rs \
  crates/oo-gateway/src/lib.rs \
  crates/oo-gateway/Cargo.toml \
  Cargo.lock \
  contracts/gateway/column-acl-set-v1.md \
  contracts/gateway/reviewed-policy-v1.md \
  crates/oo-gateway/src/bin/query-pg.rs \
  > /evidence/linux/proof/product-sha256.txt
record product-hash $?
/cargo-home/bin/sha256 file \
  crates/oo-gateway/src/bin/install-column-acl.rs \
  crates/oo-gateway/tests/install_column_acl_cli.rs \
  contracts/gateway/install-column-acl-cli-v1.md \
  > /evidence/linux/proof/owned-new-sha256.txt
while read -r hash path; do
  [ -n "$path" ] || continue
  if ! awk -v h="$hash" -v p="$path" '$1==h && $2==p {found=1} END {exit !found}' /evidence/linux/proof/product-sha256.txt; then
    echo "frozen file changed $path" >&2
    record frozen-match 1
  fi
done < /evidence/linux/smoke/owned-sha256.txt
record frozen-match 0

libdir=$(cat /evidence/linux/smoke/libclang-dir.txt)
multiarch=/cargo-home/a235-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a235-clang-root/usr/lib/llvm-19/lib/clang/19/include
devso=$(find /cargo-home/a235-clang-root/usr/lib -name 'libclang.so' -print -quit)
if [ -n "$devso" ]; then
  libdir=$(dirname "$devso")
fi
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a235-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a235-clang-root/usr/share/pkgconfig"
{
  echo "LIBCLANG_PATH=$LIBCLANG_PATH"
  test -f "$llvm_inc/stdbool.h" && echo "stdbool.h=present" || echo "stdbool.h=absent"
} > /evidence/linux/proof/libclang.txt
if [ ! -f "$llvm_inc/stdbool.h" ] || [ -z "$libdir" ]; then
  record libclang 1
fi
record libclang 0
printf '%s\n' "no" > /evidence/linux/proof/cargo-update-invoked.txt

rm -f /evidence/live/oracle.txt
echo "=== cargo test --test install_column_acl_cli ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline \
  -p oo-gateway --test install_column_acl_cli -- --test-threads=1 --nocapture \
  > /evidence/linux/proof/test.log 2>&1
test_exit=$?
set -e
record test "$test_exit"
if ! grep -q 'test result: ok. 12 passed' /evidence/linux/proof/test.log; then
  echo "expected 12 install-column-acl cli tests" >&2
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
malformed_scan=scan
malformed_request=request
malformed_roles=unchanged
stale=policy stale
stale_role=absent
bad_second=policy mapping
bad_second_role=absent
clerk_effect=installed
grantor=oo_fixture_admin
peek_effect=installed
clerk_replay=replay
replay_tables=same
column_grants_clerk=6
column_grants_peek=4
table_privileges=0
mapping_select=0
publication=0
role_shape=nologin
audit_wal=absent
authentication=not_claimed
query_party=1
hidden_write=42501
EOF
admin=$(tr -d '\n' < /run/oo-a235/fixture/admin.pass)
login=$(tr -d '\n' < /run/oo-a235/fixture/login.pass)
if grep -F -q "$admin" /evidence/live/oracle.txt || grep -F -q "$login" /evidence/live/oracle.txt; then
  echo "oracle contains a password" >&2
  record oracle 1
fi
if grep -F -q "$admin" /evidence/linux/proof/test.log || grep -F -q "$login" /evidence/linux/proof/test.log; then
  echo "test log contains a password" >&2
  record oracle 1
fi
unset admin login
record oracle 0

if ! grep -q 'forbid(unsafe_code)' crates/oo-gateway/src/bin/install-column-acl.rs; then
  echo "cli bin does not forbid unsafe" >&2
  record unsafe 2
fi
if grep -n 'unsafe' crates/oo-gateway/src/bin/install-column-acl.rs | grep -v 'forbid(unsafe_code)' > /evidence/linux/proof/unsafe-bin.txt; then
  echo "install-column-acl.rs contains an unsafe token" >&2
  record unsafe 2
fi
if grep -n 'unsafe' crates/oo-gateway/tests/install_column_acl_cli.rs > /evidence/linux/proof/unsafe-test.txt; then
  echo "cli test contains an unsafe token" >&2
  record unsafe 2
fi
: > /evidence/linux/proof/unsafe-test.txt
printf '%s\n' 0 > /evidence/linux/proof/phases/unsafe.exit

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
echo "=== cargo clippy --bin install-column-acl --test install_column_acl_cli ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline \
  -p oo-gateway --bin install-column-acl --test install_column_acl_cli -- -D warnings \
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
