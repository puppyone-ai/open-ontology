#!/bin/bash
# Focused live review and Clippy after the pre-overlay smoke. The archive is the
# fixed commit plus one review test. No manifest edit. No cargo update.
# Origin: a241 overlay inside.sh and a240 chain inside.sh.
# Changes: a246 names, public init/import, provision_mapping_reader, no product overlay.
set -u
echo "oo_task=sp06-mapping-reader-review-overlay"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a246"
echo "oo_fixed=1f1028f6161783d6e3bf1e9beb3a23a1161deca0"
echo "oo_platform=linux/arm64"
echo "phase=overlay"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1
export CARGO_NET_OFFLINE=true
export HOME=/tmp
export OO_A246_LIVE=1
export OO_A246_INIT_PG=/target/debug/init-pg
export OO_A246_PIPELINE=/target/debug/oo-pipeline
unset OO_A240_LIVE || true
mkdir -p /evidence/linux/overlay/phases /evidence/live/steps /cargo-home/bin
cd /src

record() {
  mkdir -p /evidence/linux/overlay/phases
  printf '%s\n' "$2" > "/evidence/linux/overlay/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/overlay/overlay.exit
    exit "$2"
  fi
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/linux/overlay/rustc-version.txt
cargo -V | tee /evidence/linux/overlay/cargo-version.txt
rustup show > /evidence/linux/overlay/rustup-show.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
printf '%s\n' 0 > /evidence/linux/overlay/phases/toolchain.exit

if [ ! -f crates/oo-gateway/src/mapping_reader_access.rs ]; then
  echo "fixed archive is missing mapping_reader_access.rs" >&2
  record source 2
fi
if [ ! -f crates/oo-gateway/tests/mapping_reader_access_a246_review.rs ]; then
  echo "review test is missing from the overlay tree" >&2
  record source 2
fi
if [ -f rust-toolchain.toml ]; then
  echo "extracted tree still selects a toolchain file" >&2
  record source 2
fi
if [ -e /secrets/server.key ] || [ -e /secrets/ca.key ] || [ -e /secrets/server.csr ]; then
  echo "fixture mount contains a private key" >&2
  record source 2
fi
record source 0

if [ ! -x /cargo-home/bin/sha256 ]; then
  rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256
fi
export PATH="/cargo-home/bin:${PATH}"
/cargo-home/bin/sha256 file \
  crates/oo-gateway/src/mapping_reader_access.rs \
  crates/oo-gateway/src/lib.rs \
  crates/oo-gateway/tests/mapping_reader_access_pg_live.rs \
  contracts/gateway/mapping-reader-access-v1.md \
  crates/oo-gateway/src/policy.rs \
  Cargo.lock \
  crates/oo-pipeline/src/bin/init-pg.rs \
  crates/oo-pipeline/src/bin/oo-pipeline.rs \
  > /evidence/linux/overlay/product-sha256.txt
if ! cmp -s /evidence/linux/smoke/owned-sha256.txt /evidence/linux/overlay/product-sha256.txt; then
  echo "product bytes changed after the smoke" >&2
  record frozen-match 1
fi
record frozen-match 0
/cargo-home/bin/sha256 file crates/oo-gateway/tests/mapping_reader_access_a246_review.rs \
  > /evidence/linux/overlay/review-test-sha256.txt
record product-hash 0

libdir=$(cat /evidence/linux/smoke/libclang-path.txt)
multiarch=/cargo-home/a246-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a246-clang-root/usr/lib/llvm-19/lib/clang/19/include
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a246-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a246-clang-root/usr/share/pkgconfig"
if [ ! -e "$libdir/libclang.so" ] && [ ! -e "$libdir/libclang.so.1" ]; then
  echo "libclang is missing from the cargo volume" >&2
  record libclang 2
fi
printf '%s\n' "$LIBCLANG_PATH" > /evidence/linux/overlay/libclang.txt
record libclang 0
printf '%s\n' "no" > /evidence/linux/overlay/cargo-update-invoked.txt

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/linux/overlay/clippy-component.log 2>&1
  clippy_component=$?
  set -e
  record clippy-component "$clippy_component"
else
  echo "clippy_component=present" > /evidence/linux/overlay/clippy-component.log
  record clippy-component 0
fi

echo "=== build init-pg and oo-pipeline ==="
set +e
cargo build --offline --locked --manifest-path /src/Cargo.toml -p oo-pipeline --bin init-pg --bin oo-pipeline \
  > /evidence/linux/overlay/bins-build.log 2>&1
build_exit=$?
set -e
record bins "$build_exit"
test -x /target/debug/init-pg
test -x /target/debug/oo-pipeline
/cargo-home/bin/sha256 file /target/debug/init-pg /target/debug/oo-pipeline > /evidence/linux/overlay/bin-sha256.txt
record bin-hash 0

echo "=== mapping reader review live test ==="
set +e
cargo test --offline --locked --manifest-path /src/Cargo.toml -p oo-gateway --test mapping_reader_access_a246_review -- --test-threads=1 --nocapture \
  > /evidence/linux/overlay/test.log 2>&1
test_exit=$?
set -e
record test "$test_exit"
if ! grep -q 'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out' /evidence/linux/overlay/test.log; then
  echo "expected 1 review test" >&2
  record test-count 1
fi
record test-count 0

required='server=tls_17.10
import_before_init=target schema is not ready
init=bootstrapped
import_state=awaiting_publish
runtime_owner=false
column_acl=installed
catalog_before=42501
login_mismatch=LoginMismatch
stale=StaleGeneration
mapping_absent=MappingAbsent
login_absent=LoginAbsent
partial_privilege=usage_only_refused
business_grants=unchanged
provision=Installed
public=absent
replay=Replay
typed_query=clerk_customer_3
hidden=42501
mapping_rows=all
outside_policy=private_note,ratio
tenant_isolation=not_provided
source_writes=reader_runtime_login_zero
runtime_owner_after=false'
printf '%s\n' "$required" > /evidence/linux/overlay/oracle-required.txt
missing=0
while IFS= read -r line; do
  [ -n "$line" ] || continue
  if ! grep -Fxq "$line" /evidence/live/oracle.txt; then
    echo "oracle missing $line" >&2
    missing=1
  fi
done <<EOF
$required
EOF
if [ "$missing" -ne 0 ]; then
  record oracle 1
fi
record oracle 0

echo "=== clippy ==="
set +e
cargo clippy --offline --locked --manifest-path /src/Cargo.toml -p oo-gateway --lib --test mapping_reader_access_a246_review --test mapping_reader_access_pg_live -- -D warnings \
  > /evidence/linux/overlay/clippy.log 2>&1
clippy_exit=$?
set -e
record clippy "$clippy_exit"

/cargo-home/bin/sha256 file Cargo.lock | awk '{print $1}' > /evidence/linux/overlay/lock-after.sha256
before=$(awk '$2=="Cargo.lock" {print $1}' /evidence/linux/smoke/owned-sha256.txt)
after=$(cat /evidence/linux/overlay/lock-after.sha256)
if [ "$before" != "$after" ]; then
  echo "lock changed before=$before after=$after" > /evidence/linux/overlay/lock-hash.txt
  record lock-hash 1
fi
echo "unchanged $after" > /evidence/linux/overlay/lock-hash.txt
record lock-hash 0

if grep -nE 'unsafe[[:space:]{]' \
  crates/oo-gateway/src/mapping_reader_access.rs \
  crates/oo-gateway/tests/mapping_reader_access_a246_review.rs \
  > /evidence/linux/overlay/unsafe-lines.txt; then
  echo "review scope contains an unsafe block" >&2
  record unsafe 2
fi
printf '%s\n' "no unsafe block" > /evidence/linux/overlay/unsafe-lines.txt
printf '%s\n' 0 > /evidence/linux/overlay/phases/unsafe.exit

: > /tmp/a246-bans
for secret_file in /secrets/*.pass; do
  tr -d '\r\n' < "$secret_file" >> /tmp/a246-bans
  printf '\n' >> /tmp/a246-bans
done
set +e
grep -F -l -f /tmp/a246-bans \
  /evidence/live/oracle.txt \
  /evidence/live/outcome.json \
  /evidence/live/outcome.txt \
  /evidence/linux/overlay/test.log \
  > /evidence/linux/overlay/secret-scan-hits.txt
scan_exit=$?
set -e
if [ "$scan_exit" -eq 0 ]; then
  echo "secret scan found a password" >&2
  record secret-scan 1
fi
if [ "$scan_exit" -ne 1 ]; then
  echo "secret scan failed" >&2
  record secret-scan 1
fi
printf '%s\n' "clean" > /evidence/linux/overlay/secret-scan-hits.txt
record secret-scan 0

printf '%s\n' 0 > /evidence/linux/overlay/overlay.exit
echo "overlay_review=ok"
exit 0
