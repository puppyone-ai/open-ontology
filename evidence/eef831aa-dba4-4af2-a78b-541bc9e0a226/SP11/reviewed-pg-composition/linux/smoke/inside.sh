#!/bin/bash
# Focused compiler smoke on the immutable ed661809 archive, before any harness overlay.
# Linux arm64 Rust 1.94. No cargo update. No apt. No PostgreSQL.
set -u
echo "oo_task=sp11-reviewed-pg-composition-preoverlay"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a226"
echo "oo_fixed=ed661809cf6c1e8df37af5842838dd1eb839c98a"
echo "oo_platform=linux/arm64"
echo "phase=preoverlay"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
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

rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256
record sha256-tool $?
rustc --edition 2021 /tools/select-debs.rs -o /cargo-home/bin/select-debs
record select-debs-tool $?
export PATH="/cargo-home/bin:${PATH}"

if [ ! -f crates/oo-pipeline/src/bin/init-pg.rs ] \
  || [ ! -f crates/oo-builder/src/bin/reviewed-effective-oos.rs ] \
  || [ ! -f crates/oo-builder/src/bin/role-pack-bundle.rs ] \
  || [ ! -f crates/oo-gateway/src/bin/query-pg.rs ]; then
  echo "fixed archive is missing a compiler binary source" >&2
  record source 2
fi
if ! grep -q 'fn install_column_acl_set' crates/oo-gateway/src/policy.rs; then
  echo "install_column_acl_set is absent" >&2
  record source 2
fi
if ! grep -q 'fn analyze_mapped_shadows' crates/oo-pipeline/src/target_statistics.rs; then
  echo "analyze_mapped_shadows is absent" >&2
  record source 2
fi
if [ -e a226-harness ]; then
  echo "harness overlay is already on the pre-overlay archive" >&2
  record source 2
fi
printf '%s\n' "overlay_absent" > /evidence/linux/smoke/overlay-absent.txt
cmp -s crates/oo-gateway/src/policy.rs /evidence/source/blobs/policy.rs
record blob-cmp $?

/cargo-home/bin/sha256 file \
  crates/oo-pipeline/src/bin/oo-pipeline.rs \
  crates/oo-pipeline/src/bin/init-pg.rs \
  crates/oo-pipeline/src/catalog_cli.rs \
  crates/oo-pipeline/src/import_cli.rs \
  crates/oo-pipeline/src/target_statistics.rs \
  crates/oo-builder/src/bin/oo-builder.rs \
  crates/oo-builder/src/bin/reviewed-effective-oos.rs \
  crates/oo-builder/src/bin/role-pack-bundle.rs \
  crates/oo-builder/src/reviewed_effective.rs \
  crates/oo-gateway/src/bin/query-pg.rs \
  crates/oo-gateway/src/policy.rs \
  Cargo.lock \
  > /evidence/linux/smoke/owned-sha256.txt
record owned-hash $?

if [ ! -s /evidence/linux/smoke/libclang-dir.txt ] || [ ! -d /cargo-home/a226-clang-root/usr/lib ]; then
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
multiarch=/cargo-home/a226-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a226-clang-root/usr/lib/llvm-19/lib/clang/19/include
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a226-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a226-clang-root/usr/share/pkgconfig"
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
real=$(find /cargo-home/a226-clang-root/usr/lib -name 'libclang-19.so.19' -print -quit)
devso=$(find /cargo-home/a226-clang-root/usr/lib -name 'libclang.so' -print -quit)
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

echo "=== cargo fetch --locked ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked > /evidence/linux/smoke/fetch.log 2>&1
fetch_exit=$?
set -e
record fetch "$fetch_exit"

echo "=== builder library reviewed_effective ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder --lib reviewed_effective -- --test-threads=1 \
  > /evidence/linux/smoke/test-builder-lib.log 2>&1
builder_lib_exit=$?
set -e
record test-builder-lib "$builder_lib_exit"

echo "=== builder CLI tests ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder \
  --test reviewed_effective --test role_pack --test role_pack_cli --test role_pack_bundle \
  --test captured_pg_cli --test review_pg_cli -- --test-threads=1 \
  > /evidence/linux/smoke/test-builder.log 2>&1
builder_exit=$?
set -e
record test-builder "$builder_exit"

echo "=== pipeline CLI tests, live tests ignored ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline \
  --test init_pg_cli --test catalog_pg_cli_live --test import_pg_cli_live -- --test-threads=1 \
  > /evidence/linux/smoke/test-pipeline.log 2>&1
pipeline_exit=$?
set -e
record test-pipeline "$pipeline_exit"

echo "=== gateway policy unit tests ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib policy::tests -- --test-threads=1 \
  > /evidence/linux/smoke/test-gateway-lib.log 2>&1
gateway_exit=$?
set -e
record test-gateway-lib "$gateway_exit"

echo "=== query-pg CLI tests, live test skipped without OO_A198_LIVE ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway \
  --test query_pg_cli -- --test-threads=1 \
  > /evidence/linux/smoke/test-query.log 2>&1
query_exit=$?
set -e
record test-query "$query_exit"

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
echo "preoverlay_smoke=ok"
exit 0
