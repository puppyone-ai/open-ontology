#!/bin/bash
# Pre-overlay Linux arm64 Rust 1.94 smoke of fixed 03512f91.
# No cargo update. No apt. No capability added. No review test overlay.
set -u
echo "oo_task=sp06-sql-guard-review-smoke"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a179"
echo "oo_fixed=03512f91324ac33e772e9702fe806e8e04037ec4"
echo "oo_platform=linux/arm64"
echo "phase=preoverlay"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
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

if [ ! -f crates/oo-gateway/src/sql_guard.rs ]; then
  echo "fixed sql_guard.rs is absent" >&2
  record source 2
fi
if [ -f crates/oo-gateway/tests/sql_guard_a179_review.rs ]; then
  echo "review overlay is already in the archive" >&2
  record overlay 2
fi
printf '%s\n' "absent" > /evidence/linux/smoke/overlay-absent.txt
sha256sum Cargo.lock | awk '{print $1}' > /evidence/linux/smoke/lock-before.sha256

# /tmp is noexec. Extract the recorded debs onto the private cargo volume.
ROOT=/cargo-home/a179-clang-root
mkdir -p "$ROOT"
: > /evidence/linux/smoke/libclang-extract.log
while read -r name filename digest; do
  [ -n "$name" ] || continue
  deb="/debs/${name}.deb"
  if [ ! -s "$deb" ]; then
    echo "missing deb $name" >> /evidence/linux/smoke/libclang-extract.log
    record libclang 1
  fi
  got_deb=$(sha256sum "$deb" | awk '{print $1}')
  if [ "$got_deb" != "$digest" ]; then
    echo "checksum mismatch $name" >> /evidence/linux/smoke/libclang-extract.log
    record libclang 1
  fi
  echo "ok $name $got_deb" >> /evidence/linux/smoke/libclang-extract.log
  if ! dpkg-deb -x "$deb" "$ROOT" >> /evidence/linux/smoke/libclang-extract.log 2>&1; then
    record libclang 1
  fi
done < /evidence/linux/smoke/libclang-debs.txt
libclang_file=$(find "$ROOT/usr/lib" \( -name 'libclang.so' -o -name 'libclang-*.so' \) | head -1)
if [ -z "$libclang_file" ]; then
  echo "libclang.so not found" >> /evidence/linux/smoke/libclang-extract.log
  record libclang 1
fi
libdir=$(dirname "$libclang_file")
printf '%s\n' "$libdir" > /evidence/linux/smoke/libclang-dir.txt
multiarch="$ROOT/usr/lib/aarch64-linux-gnu"
llvm_inc="$ROOT/usr/lib/llvm-19/lib/clang/19/include"
if [ ! -f "$llvm_inc/stdbool.h" ]; then
  echo "recorded stdbool.h include is absent" >> /evidence/linux/smoke/libclang-extract.log
  record libclang 1
fi
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="$ROOT/usr/bin:${PATH}"
{
  echo "LIBCLANG_PATH=$LIBCLANG_PATH"
  echo "LD_LIBRARY_PATH=$LD_LIBRARY_PATH"
  echo "BINDGEN_EXTRA_CLANG_ARGS=$BINDGEN_EXTRA_CLANG_ARGS"
  ls -l "$libdir"/libclang.so* || true
  test -f "$llvm_inc/stdbool.h" && echo "stdbool.h=present"
  command -v protoc || echo "protoc=absent"
} > /evidence/linux/smoke/libclang.txt
if ! /lib/ld-linux-aarch64.so.1 --list "${multiarch}/libclang-19.so.19" > /evidence/linux/smoke/libclang-ldd.txt 2>&1; then
  record libclang-load 1
fi
if grep -q 'not found' /evidence/linux/smoke/libclang-ldd.txt; then
  record libclang-load 1
fi
record libclang 0
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
sha256sum Cargo.lock | awk '{print $1}' > /evidence/linux/smoke/lock-after.sha256
if ! cmp -s /evidence/linux/smoke/lock-before.sha256 /evidence/linux/smoke/lock-after.sha256; then
  echo "fetch changed Cargo.lock" > /evidence/linux/smoke/lock-hash.txt
  record lock-hash 1
fi
echo "unchanged" > /evidence/linux/smoke/lock-hash.txt
record lock-hash 0

set +e
: > /evidence/linux/smoke/crate-manifests.txt
: > /evidence/linux/smoke/crate-manifests-missing.txt
manifest_count=0
while read -r name version; do
  [ -n "${name:-}" ] || continue
  toml=$(find /cargo-home/registry/src -path "*/${name}-${version}/Cargo.toml" -print -quit)
  if [ -z "$toml" ]; then
    echo "$name $version" >> /evidence/linux/smoke/crate-manifests-missing.txt
    continue
  fi
  license=$(awk -F= '$1=="license " { gsub(/[ "\047]/, "", $2); print $2; exit }' "$toml")
  rust_version=$(awk -F= '$1=="rust-version " { gsub(/[ "\047]/, "", $2); print $2; exit }' "$toml")
  links=$(awk -F= '$1=="links " { gsub(/[ "\047]/, "", $2); print $2; exit }' "$toml")
  crate_dir=$(dirname "$toml")
  build_rs=no
  if [ -f "$crate_dir/build.rs" ]; then
    build_rs=yes
  fi
  license_files=$(find "$crate_dir" -maxdepth 1 -name 'LICENSE*' -printf '%f\n' | sort | paste -sd, -)
  native=no
  case "$name" in
    pg_query|clang-sys|libloading|bindgen) native=yes ;;
  esac
  if [ "$build_rs" = yes ] || [ -n "$links" ]; then
    native=yes
  fi
  printf '%s %s license=%s rust-version=%s links=%s build_rs=%s native_marker=%s license_files=%s\n' \
    "$name" "$version" "${license:--}" "${rust_version:--}" "${links:--}" "$build_rs" "$native" "${license_files:--}" \
    >> /evidence/linux/smoke/crate-manifests.txt
  manifest_count=$((manifest_count + 1))
done < /evidence/linux/lock/added-pairs.txt
echo "manifests=$manifest_count" >> /evidence/linux/smoke/crate-manifests.txt
set -e
if [ -s /evidence/linux/smoke/crate-manifests-missing.txt ] || [ "$manifest_count" -ne 35 ]; then
  record crate-manifests 1
fi
record crate-manifests 0

echo "=== cargo test -p oo-gateway --lib ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib -- --test-threads=8 \
  > /evidence/linux/smoke/lib-test.log 2>&1
lib_exit=$?
set -e
record lib-test "$lib_exit"

echo "=== cargo test -p oo-gateway --test sql_guard ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --test sql_guard -- --test-threads=8 \
  > /evidence/linux/smoke/sql-guard-test.log 2>&1
guard_exit=$?
set -e
record sql-guard-test "$guard_exit"

echo "=== cargo clippy -p oo-gateway --all-targets ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-gateway --all-targets -- -D warnings \
  > /evidence/linux/smoke/clippy.log 2>&1
clippy_exit=$?
set -e
record clippy "$clippy_exit"

echo "=== cargo run -p oo-arch-check ==="
set +e
cargo run -p oo-arch-check --locked --offline --manifest-path Cargo.toml \
  > /evidence/linux/smoke/arch-check.log 2>&1
arch_exit=$?
set -e
record arch-check "$arch_exit"

if grep -n 'unsafe' crates/oo-gateway/src/sql_guard.rs crates/oo-gateway/src/lib.rs crates/oo-gateway/tests/sql_guard.rs \
  | grep -v 'forbid(unsafe_code)' | grep -v 'does not use unsafe' | grep -v 'no localunsafe' > /evidence/linux/smoke/unsafe-lines.txt; then
  record unsafe 1
fi
# The filter above exits 1 when grep finds nothing. Keep an explicit empty check.
if [ -s /evidence/linux/smoke/unsafe-lines.txt ]; then
  record unsafe 1
fi
record unsafe 0

printf '%s\n' 0 > /evidence/linux/smoke/smoke.exit
echo "smoke_ok"
