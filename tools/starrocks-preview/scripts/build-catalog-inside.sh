#!/bin/bash
# Linux 1.94 compile of the fixed a6825165 archive. One cargo job.
# The live workspace is not mounted. This script does not open a database.
set -euo pipefail

if [[ -n "${HOST_UID:-}" ]]; then
  trap 'chown -R "${HOST_UID}:${HOST_GID}" /evidence /bins || true' EXIT
fi

phase() {
  printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*" | tee -a /evidence/phase.txt
}

phase "rustc"
rustc --version | tee /evidence/rustc-version.txt
grep -q '^rustc 1.94.0 ' /evidence/rustc-version.txt
if [[ -e /src/rust-toolchain.toml ]]; then
  phase "FAIL toolchain file"
  exit 1
fi
grep -q 'oo.pipeline.native-sealed-catalog/v1' /src/crates/oo-pipeline/src/native_catalog.rs
grep -q 'import-native-catalog' /src/tools/pg-builder/src/main.rs
test -f /src/crates/oo-gateway/src/bin/install-column-acl.rs
test -f /src/tools/operator-preview/src/main.rs

export HOME=/tmp
export TMPDIR=/tmp
export CARGO_HOME=/cargo
export CARGO_TARGET_DIR=/target/catalog-a682
export CARGO_BUILD_JOBS=1
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_TERM_COLOR=never
cd /src

phase "libclang"
if ! find /usr -name 'libclang.so' -o -name 'libclang-*.so*' | grep -q .; then
  apt-get update > /evidence/apt-update.log 2>&1
  DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends libclang-dev \
    > /evidence/apt-clang.log 2>&1
fi
find /usr -name 'libclang.so' -o -name 'libclang-*.so*' | tee /evidence/libclang.txt
libclang_so="$(find /usr -name 'libclang.so' | head -n 1)"
if [[ -z "$libclang_so" ]]; then
  phase "FAIL libclang shared object"
  exit 1
fi
export LIBCLANG_PATH="$(dirname "$libclang_so")"
printf '%s\n' "$LIBCLANG_PATH" | tee /evidence/libclang-path.txt

phase "fetch"
cargo fetch --locked --manifest-path /src/Cargo.toml > /evidence/fetch.log 2>&1
printf '%s\n' 0 > /evidence/fetch.exit
phase "fetch pg-builder"
cargo fetch --locked --manifest-path /src/tools/pg-builder/Cargo.toml > /evidence/fetch-pg.log 2>&1
printf '%s\n' 0 > /evidence/fetch-pg.exit
phase "fetch operator"
cargo fetch --locked --manifest-path /src/tools/operator-preview/Cargo.toml > /evidence/fetch-operator.log 2>&1
printf '%s\n' 0 > /evidence/fetch-operator.exit

build_one() {
  local name="$1"
  shift
  phase "build ${name}"
  set +e
  cargo build --jobs 1 --locked --offline "$@" > "/evidence/${name}.log" 2>&1
  local code=$?
  set -e
  printf '%s\n' "$code" > "/evidence/${name}.exit"
  if [[ "$code" -ne 0 ]]; then
    tail -n 80 "/evidence/${name}.log" | tee -a /evidence/phase.txt
    phase "FAIL ${name} ${code}"
    exit 1
  fi
}

build_one pipeline -p oo-pipeline --bin oo-pipeline --bin init-pg
build_one acl -p oo-gateway --bin install-column-acl
build_one pg-builder --manifest-path /src/tools/pg-builder/Cargo.toml --bin pg-builder
build_one operator --manifest-path /src/tools/operator-preview/Cargo.toml --bin operator-preview

mkdir -p /bins
cp -a /target/catalog-a682/debug/oo-pipeline /bins/oo-pipeline
cp -a /target/catalog-a682/debug/init-pg /bins/init-pg
cp -a /target/catalog-a682/debug/install-column-acl /bins/install-column-acl
cp -a /target/catalog-a682/debug/pg-builder /bins/pg-builder
cp -a /target/catalog-a682/debug/operator-preview /bins/operator-preview
sha256sum /bins/oo-pipeline /bins/init-pg /bins/install-column-acl /bins/pg-builder /bins/operator-preview \
  | tee /evidence/binaries.sha256
grep -a -q 'catalog-import-native' /bins/oo-pipeline
grep -a -q 'install-native-sealed-catalog' /bins/oo-pipeline
grep -a -q 'import-native-catalog' /bins/pg-builder
grep -a -q 'date32 days since unix epoch' /bins/oo-pipeline || true
phase "binaries hashed"
printf '%s\n' 0 > /evidence/build.exit
