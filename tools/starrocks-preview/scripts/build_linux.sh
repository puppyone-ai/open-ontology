#!/bin/bash
# Linux Rust 1.94 product compile. One job. Does not touch the driver target.
set -u
export CARGO_HOME=/cargo-home
export CARGO_BUILD_JOBS=1
export RUSTUP_TOOLCHAIN=1.94.0
mkdir -p /evidence/build /cargo-home /target/preview /target/ws /target/pg-builder /target/admission /target/operator
if command -v git >/dev/null 2>&1; then
  git config --global --add safe.directory /repo || true
  git config --global --add safe.directory '*' || true
fi
rustup run 1.94.0 rustc -vV > /evidence/build/rustc-vv.txt
if ! grep -q '^release: 1.94.0$' /evidence/build/rustc-vv.txt; then
  printf '91\n' > /evidence/build/build.exit
  exit 91
fi
if ! rustup run 1.94.0 cargo clippy --version >/dev/null 2>&1; then
  rustup component add clippy --toolchain 1.94.0 > /evidence/build/clippy-component.stdout 2> /evidence/build/clippy-component.stderr
  component=$?
  printf '%s\n' "$component" > /evidence/build/clippy-component.exit
  if [ "$component" != 0 ]; then
    printf '%s\n' "$component" > /evidence/build/build.exit
    exit "$component"
  fi
fi

cd /src
if [ ! -f Cargo.lock ]; then
  rustup run 1.94.0 cargo generate-lockfile > /evidence/build/generate-lock.stdout 2> /evidence/build/generate-lock.stderr
  gen=$?
  printf '%s\n' "$gen" > /evidence/build/generate-lock.exit
  if [ "$gen" != 0 ]; then
    printf '%s\n' "$gen" > /evidence/build/build.exit
    exit "$gen"
  fi
fi
cp Cargo.lock /evidence/build/Cargo.lock
rustup run 1.94.0 cargo fetch --locked > /evidence/build/fetch.stdout 2> /evidence/build/fetch.stderr
fetch=$?
printf '%s\n' "$fetch" > /evidence/build/fetch.exit
if [ "$fetch" != 0 ]; then
  printf '%s\n' "$fetch" > /evidence/build/build.exit
  exit "$fetch"
fi
export CARGO_TARGET_DIR=/target/preview
rustup run 1.94.0 cargo test --offline --locked --jobs 1 --all-targets > /evidence/build/test.stdout 2> /evidence/build/test.stderr
test_code=$?
printf '%s\n' "$test_code" > /evidence/build/test.exit
if [ "$test_code" != 0 ]; then
  printf '%s\n' "$test_code" > /evidence/build/build.exit
  exit "$test_code"
fi
rustup run 1.94.0 cargo clippy --offline --locked --jobs 1 --all-targets -- -D warnings > /evidence/build/clippy.stdout 2> /evidence/build/clippy.stderr
clippy_code=$?
printf '%s\n' "$clippy_code" > /evidence/build/clippy.exit
if [ "$clippy_code" != 0 ]; then
  printf '%s\n' "$clippy_code" > /evidence/build/build.exit
  exit "$clippy_code"
fi
rustup run 1.94.0 cargo build --offline --locked --jobs 1 > /evidence/build/build.stdout 2> /evidence/build/build.stderr
build_code=$?
printf '%s\n' "$build_code" > /evidence/build/build.exit
if [ "$build_code" != 0 ]; then
  exit "$build_code"
fi
sha256sum /target/preview/debug/starrocks-preview > /evidence/build/preview.sha256

# pg_query's bindgen needs libclang. The pinned image does not contain it.
# Install it in this container only. The image digest is not retagged.
if ! ldconfig -p 2>/dev/null | grep -q libclang; then
  apt-get update > /evidence/build/apt.stdout 2> /evidence/build/apt.stderr
  apt-get install -y --no-install-recommends libclang-dev >> /evidence/build/apt.stdout 2>> /evidence/build/apt.stderr
  apt_code=$?
  printf '%s\n' "$apt_code" > /evidence/build/apt.exit
  if [ "$apt_code" != 0 ]; then
    exit "$apt_code"
  fi
fi

# Read-only bins. Locks that already exist stay --locked. Lockless trees are copied
# so cargo does not write a lock into the source tree.
cd /repo
export CARGO_TARGET_DIR=/target/ws
rustup run 1.94.0 cargo fetch --locked --manifest-path /repo/Cargo.toml > /evidence/build/ws-fetch.stdout 2> /evidence/build/ws-fetch.stderr
ws_fetch=$?
printf '%s\n' "$ws_fetch" > /evidence/build/ws-fetch.exit
if [ "$ws_fetch" != 0 ]; then
  exit "$ws_fetch"
fi
rustup run 1.94.0 cargo build --offline --locked --jobs 1 --manifest-path /repo/Cargo.toml -p oo-pipeline --bin init-pg --bin oo-pipeline -p oo-gateway --bin install-column-acl > /evidence/build/ws-build.stdout 2> /evidence/build/ws-build.stderr
ws=$?
printf '%s\n' "$ws" > /evidence/build/ws-build.exit
if [ "$ws" != 0 ]; then
  exit "$ws"
fi

export CARGO_TARGET_DIR=/target/pg-builder
cd /repo/tools/pg-builder
rustup run 1.94.0 cargo fetch --locked > /evidence/build/pg-fetch.stdout 2> /evidence/build/pg-fetch.stderr
pg_fetch=$?
printf '%s\n' "$pg_fetch" > /evidence/build/pg-fetch.exit
if [ "$pg_fetch" != 0 ]; then
  exit "$pg_fetch"
fi
rustup run 1.94.0 cargo build --offline --locked --jobs 1 --bin pg-builder > /evidence/build/pg-build.stdout 2> /evidence/build/pg-build.stderr
pg=$?
printf '%s\n' "$pg" > /evidence/build/pg-build.exit
if [ "$pg" != 0 ]; then
  exit "$pg"
fi

copy_tool() {
  name="$1"
  dest="/tmp/tree-$name"
  rm -rf "$dest"
  mkdir -p "$dest/tools"
  ln -s /repo/crates "$dest/crates"
  ln -s /repo/testing "$dest/testing"
  cp /repo/Cargo.toml /repo/Cargo.lock "$dest/"
  for dir in /repo/tools/*; do
    base=$(basename "$dir")
    if [ "$base" = "$name" ]; then
      cp -a "$dir" "$dest/tools/$base"
      rm -f "$dest/tools/$base/Cargo.lock"
    else
      ln -s "$dir" "$dest/tools/$base"
    fi
  done
}

copy_tool source-admission
export CARGO_TARGET_DIR=/target/admission
cd /tmp/tree-source-admission/tools/source-admission
rustup run 1.94.0 cargo generate-lockfile > /evidence/build/admission-lock.stdout 2> /evidence/build/admission-lock.stderr
al=$?
printf '%s\n' "$al" > /evidence/build/admission-lock.exit
if [ "$al" != 0 ]; then
  exit "$al"
fi
cp Cargo.lock /evidence/build/source-admission.Cargo.lock
rustup run 1.94.0 cargo fetch --locked > /evidence/build/admission-fetch.stdout 2> /evidence/build/admission-fetch.stderr
af=$?
printf '%s\n' "$af" > /evidence/build/admission-fetch.exit
if [ "$af" != 0 ]; then
  exit "$af"
fi
rustup run 1.94.0 cargo build --offline --locked --jobs 1 --bin oo-source-admission > /evidence/build/admission-build.stdout 2> /evidence/build/admission-build.stderr
ab=$?
printf '%s\n' "$ab" > /evidence/build/admission-build.exit
if [ "$ab" != 0 ]; then
  exit "$ab"
fi

copy_tool operator-preview
export CARGO_TARGET_DIR=/target/operator
cd /tmp/tree-operator-preview/tools/operator-preview
rustup run 1.94.0 cargo generate-lockfile > /evidence/build/operator-lock.stdout 2> /evidence/build/operator-lock.stderr
ol=$?
printf '%s\n' "$ol" > /evidence/build/operator-lock.exit
if [ "$ol" != 0 ]; then
  exit "$ol"
fi
cp Cargo.lock /evidence/build/operator-preview.Cargo.lock
rustup run 1.94.0 cargo fetch --locked > /evidence/build/operator-fetch.stdout 2> /evidence/build/operator-fetch.stderr
of=$?
printf '%s\n' "$of" > /evidence/build/operator-fetch.exit
if [ "$of" != 0 ]; then
  exit "$of"
fi
rustup run 1.94.0 cargo build --offline --locked --jobs 1 --bin operator-preview > /evidence/build/operator-build.stdout 2> /evidence/build/operator-build.stderr
ob=$?
printf '%s\n' "$ob" > /evidence/build/operator-build.exit
if [ "$ob" != 0 ]; then
  exit "$ob"
fi

mkdir -p /evidence/build/bin
cp /target/preview/debug/starrocks-preview /evidence/build/bin/starrocks-preview
cp /target/ws/debug/init-pg /evidence/build/bin/init-pg
cp /target/ws/debug/oo-pipeline /evidence/build/bin/oo-pipeline
cp /target/ws/debug/install-column-acl /evidence/build/bin/install-column-acl
cp /target/pg-builder/debug/pg-builder /evidence/build/bin/pg-builder
cp /target/admission/debug/oo-source-admission /evidence/build/bin/oo-source-admission
cp /target/operator/debug/operator-preview /evidence/build/bin/operator-preview
sha256sum /evidence/build/bin/* > /evidence/build/binaries.sha256
exit 0
