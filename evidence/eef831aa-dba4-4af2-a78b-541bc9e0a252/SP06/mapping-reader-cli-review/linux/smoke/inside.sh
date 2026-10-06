#!/bin/bash
# Focused offline CLI smoke on the immutable 9df07481 archive, before the live proof.
# Linux arm64 Rust 1.94. No cargo update. No apt. No PostgreSQL. Live gate stays unset.
set -u
echo "oo_task=sp06-mapping-reader-cli-review-smoke"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a252"
echo "oo_fixed=9df074813572569950d85bbe951069b4db83facc"
echo "oo_platform=linux/arm64"
echo "phase=smoke"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
unset OO_A249_LIVE || true
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
rustup show > /evidence/linux/smoke/rustup-show.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
if ! grep -q '1.94.0' /evidence/linux/smoke/rustup-show.txt; then
  echo "rustup show did not report 1.94.0" >&2
  record toolchain 2
fi
record toolchain 0

rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256
record sha256-tool $?
rustc --edition 2021 /tools/select-debs.rs -o /cargo-home/bin/select-debs
record select-debs-tool $?
export PATH="/cargo-home/bin:${PATH}"

if [ ! -f crates/oo-gateway/src/bin/provision-mapping-reader.rs ]; then
  echo "fixed archive is missing the cli" >&2
  record source 2
fi
if [ ! -f crates/oo-gateway/tests/provision_mapping_reader_cli.rs ]; then
  echo "fixed archive is missing the cli test" >&2
  record source 2
fi
if [ ! -f contracts/gateway/provision-mapping-reader-cli-v1.md ]; then
  echo "fixed archive is missing the cli contract" >&2
  record source 2
fi
if ! grep -q 'pub async fn provision_mapping_reader' crates/oo-gateway/src/mapping_reader_access.rs; then
  echo "provision_mapping_reader is absent" >&2
  record source 2
fi
if ! grep -q 'compile_column_acl_set' crates/oo-gateway/src/bin/provision-mapping-reader.rs; then
  echo "cli does not compile before provision" >&2
  record source 2
fi
if [ -f rust-toolchain.toml ]; then
  echo "toolchain file is still in the archive" >&2
  record source 2
fi
sha=$(cat .fixed-base-sha)
if [ "$sha" != "9df074813572569950d85bbe951069b4db83facc" ]; then
  echo "archive sha mismatch" >&2
  record source 2
fi
printf '%s\n' "overlay_absent" > /evidence/linux/smoke/overlay-absent.txt
cmp -s crates/oo-gateway/src/bin/provision-mapping-reader.rs /evidence/source/blobs/provision-mapping-reader.rs
record blob-cmp $?

/cargo-home/bin/sha256 file \
  crates/oo-gateway/src/bin/provision-mapping-reader.rs \
  crates/oo-gateway/tests/provision_mapping_reader_cli.rs \
  contracts/gateway/provision-mapping-reader-cli-v1.md \
  crates/oo-gateway/src/lib.rs \
  crates/oo-gateway/src/mapping_reader_access.rs \
  crates/oo-gateway/src/policy.rs \
  crates/oo-gateway/Cargo.toml \
  Cargo.toml \
  Cargo.lock \
  > /evidence/linux/smoke/owned-sha256.txt
record owned-hash $?

if [ ! -s /evidence/linux/smoke/libclang-dir.txt ] || [ ! -d /cargo-home/a252-clang-root/usr/lib ]; then
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
multiarch=/cargo-home/a252-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a252-clang-root/usr/lib/llvm-19/lib/clang/19/include
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a252-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a252-clang-root/usr/share/pkgconfig"
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
real=$(find /cargo-home/a252-clang-root/usr/lib -name 'libclang-19.so.19' -print -quit)
devso=$(find /cargo-home/a252-clang-root/usr/lib -name 'libclang.so' -print -quit)
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

if grep -nE 'unsafe[[:space:]{]' \
  crates/oo-gateway/src/bin/provision-mapping-reader.rs \
  crates/oo-gateway/tests/provision_mapping_reader_cli.rs \
  > /evidence/linux/smoke/unsafe-lines.txt; then
  echo "committed cli contains an unsafe block" >&2
  record unsafe 2
fi
printf '%s\n' "no unsafe block" > /evidence/linux/smoke/unsafe-lines.txt
printf '%s\n' 0 > /evidence/linux/smoke/phases/unsafe.exit

echo "=== cargo fetch --locked ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked > /evidence/linux/smoke/fetch.log 2>&1
fetch_exit=$?
set -e
record fetch "$fetch_exit"

echo "=== offline provision_mapping_reader_cli ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --test provision_mapping_reader_cli -- --test-threads=1 \
  > /evidence/linux/smoke/test-cli.log 2>&1
cli_exit=$?
set -e
record test-cli "$cli_exit"
if ! grep -q 'test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out' /evidence/linux/smoke/test-cli.log; then
  echo "expected 14 offline cli tests" >&2
  record test-count 1
fi
if [ -f /evidence/live/oracle.txt ]; then
  echo "smoke wrote a live oracle" >&2
  record test-count 1
fi
record test-count 0

/cargo-home/bin/sha256 file Cargo.lock | awk '{print $1}' > /evidence/linux/smoke/lock-after.sha256
before=$(awk '$2 ~ /Cargo.lock$/ {print $1}' /evidence/linux/smoke/owned-sha256.txt)
after=$(cat /evidence/linux/smoke/lock-after.sha256)
if [ "$before" != "$after" ]; then
  echo "lock changed before=$before after=$after" > /evidence/linux/smoke/lock-hash.txt
  record lock-hash 1
fi
if [ "$after" != "16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a" ]; then
  echo "lock content hash drifted" > /evidence/linux/smoke/lock-hash.txt
  record lock-hash 1
fi
echo "unchanged $after" > /evidence/linux/smoke/lock-hash.txt
record lock-hash 0

printf '%s\n' 0 > /evidence/linux/smoke/smoke.exit
echo "prelive_smoke=ok"
exit 0
