#!/bin/bash
# Pre-overlay smoke of the immutable a661dcdf archive. No review test is present.
# Linux arm64 Rust 1.94. No cargo update. No apt.
set -u
echo "oo_task=sp09-query-cli-review-smoke"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a208"
echo "oo_fixed=a661dcdf72ff91561ae6b7cb79916e582884d08a"
echo "oo_platform=linux/arm64"
echo "phase=pre-overlay-smoke"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=4
mkdir -p /evidence/linux/smoke/phases /cargo-home/bin
cd /src

record() {
  mkdir -p /evidence/linux/smoke/phases
  printf '%s\n' "$2" > "/evidence/linux/smoke/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/smoke/smoke.exit
    exit "$2"
  fi
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/linux/smoke/rustc-version.txt
rustc -Vv > /evidence/linux/smoke/rustc-verbose.txt
cargo -V | tee /evidence/linux/smoke/cargo-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac

rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256
record sha256-tool $?
rustc --edition 2021 /tools/select-debs.rs -o /cargo-home/bin/select-debs
record select-debs-tool $?
export PATH="/cargo-home/bin:${PATH}"

if [ -f crates/oo-gateway/tests/query_pg_cli_a208_review.rs ]; then
  echo "review overlay is present during smoke" >&2
  record overlay-absent 2
fi
printf '%s\n' "absent" > /evidence/linux/smoke/overlay-absent.txt
record overlay-absent 0

if ! grep -n 'guard.bound_role = Some(request.role.to_string());' crates/oo-gateway/src/query_executor.rs > /evidence/linux/smoke/prebind.txt; then
  echo "shipping executor is missing the pre-guard bind" >&2
  record prebind 2
fi
record prebind 0

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
  > /evidence/linux/smoke/owned-sha256.txt
record owned-hash $?

if [ ! -s /evidence/linux/smoke/libclang-dir.txt ] || [ ! -d /cargo-home/a208-clang-root/usr/lib ]; then
  set +e
  bash /extract-libclang.sh > /evidence/linux/smoke/libclang-extract.log 2>&1
  extract_exit=$?
  set -e
  record libclang "$extract_exit"
else
  echo "libclang_root=reused" > /evidence/linux/smoke/libclang-extract.log
  record libclang 0
fi
libdir=$(cat /evidence/linux/smoke/libclang-dir.txt)
multiarch=/cargo-home/a208-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a208-clang-root/usr/lib/llvm-19/lib/clang/19/include
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a208-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a208-clang-root/usr/share/pkgconfig"
{
  echo "LIBCLANG_PATH=$LIBCLANG_PATH"
  echo "LD_LIBRARY_PATH=$LD_LIBRARY_PATH"
  ls -l "$libdir"/libclang.so* || true
  test -f "$llvm_inc/stdbool.h" && echo "stdbool.h=present" || echo "stdbool.h=absent"
} > /evidence/linux/smoke/libclang.txt
if [ ! -f "$llvm_inc/stdbool.h" ]; then
  record libclang-includes 1
fi
record libclang-includes 0
real=$(find /cargo-home/a208-clang-root/usr/lib -name 'libclang-19.so.19' -print -quit)
devso=$(find /cargo-home/a208-clang-root/usr/lib -name 'libclang.so' -print -quit)
if [ -n "$devso" ]; then
  export LIBCLANG_PATH="$(dirname "$devso")"
fi
export LD_LIBRARY_PATH="${LIBCLANG_PATH}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
if [ -z "$real" ]; then
  echo "libclang-19.so.19 missing under the private clang root" >&2
  record libclang-load 1
fi
printf '%s\n' "$LIBCLANG_PATH" > /evidence/linux/smoke/libclang-path.txt
/lib/ld-linux-aarch64.so.1 --list "$real" > /evidence/linux/smoke/libclang-ldd.txt 2>&1 || true
if grep -Eq 'not found|error while loading' /evidence/linux/smoke/libclang-ldd.txt; then
  record libclang-load 1
fi
if ! grep -q 'libLLVM' /evidence/linux/smoke/libclang-ldd.txt; then
  echo "libclang did not resolve LLVM" >&2
  record libclang-load 1
fi
record libclang-load 0
printf '%s\n' "no" > /evidence/linux/smoke/cargo-update-invoked.txt

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/linux/smoke/clippy-component.log 2>&1
  clippy_add=$?
  set -e
  record clippy-component "$clippy_add"
else
  echo "clippy_component=already_installed" > /evidence/linux/smoke/clippy-component.log
  record clippy-component 0
fi

echo "=== cargo fetch --locked ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked > /evidence/linux/smoke/fetch.log 2>&1
fetch_exit=$?
set -e
record fetch "$fetch_exit"

echo "=== cargo test -p oo-gateway --lib ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib -- --test-threads=4 \
  > /evidence/linux/smoke/test.log 2>&1
test_exit=$?
set -e
record test "$test_exit"

echo "=== cargo test query_pg_cli offline ==="
set +e
env -u OO_A198_LIVE cargo test --manifest-path Cargo.toml --locked --offline \
  -p oo-gateway --test query_pg_cli -- --test-threads=1 --nocapture \
  > /evidence/linux/smoke/cli-offline.log 2>&1
cli_exit=$?
set -e
record cli-offline "$cli_exit"
if ! grep -q 'OO_A198_LIVE is unset; the private postgres comparison was not run' /evidence/linux/smoke/cli-offline.log; then
  echo "offline CLI test did not skip the live comparison" >&2
  record cli-gate 1
fi
record cli-gate 0

echo "=== cargo clippy lib and query-pg ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib --bin query-pg -- -D warnings \
  > /evidence/linux/smoke/clippy.log 2>&1
clippy_exit=$?
set -e
record clippy "$clippy_exit"

echo "=== gated catalog test, OO_A190_LIVE unset ==="
set +e
env -u OO_A190_LIVE cargo test --manifest-path Cargo.toml --locked --offline \
  -p oo-gateway --test policy_catalog_pg_live -- --test-threads=1 --nocapture \
  > /evidence/linux/smoke/gated.log 2>&1
gated_exit=$?
set -e
record gated "$gated_exit"
if ! grep -q 'OO_A190_LIVE is unset; the policy catalog comparison was not run' /evidence/linux/smoke/gated.log; then
  echo "gated run did not say the comparison was skipped" >&2
  record gated-message 1
fi
if grep -Eq 'panicked at|query failed' /evidence/linux/smoke/gated.log; then
  echo "gated run entered the live body" >&2
  record gated-message 1
fi
printf '%s\n' 'cargo_ok_means_gate_returned comparison_not_run=1' > /evidence/linux/smoke/gated-note.txt
record gated-message 0

/cargo-home/bin/sha256 file Cargo.lock | awk '{print $1}' > /evidence/linux/smoke/lock-after.sha256
before=$(awk '$2 ~ /Cargo.lock$/ {print $1}' /evidence/linux/smoke/owned-sha256.txt)
after=$(cat /evidence/linux/smoke/lock-after.sha256)
if [ "$before" != "$after" ]; then
  echo "lock changed before=$before after=$after" > /evidence/linux/smoke/lock-hash.txt
  record lock-hash 1
fi
echo "unchanged $after" > /evidence/linux/smoke/lock-hash.txt
record lock-hash 0

printf '%s\n' 0 > /evidence/linux/smoke/smoke.exit
echo "pre_overlay_smoke=ok"
exit 0
