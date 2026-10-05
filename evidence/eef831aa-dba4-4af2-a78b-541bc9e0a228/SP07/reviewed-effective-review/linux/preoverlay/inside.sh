#!/bin/bash
# Pre-overlay smoke of fixed commit 9daeaa05. The review harness is not on this archive.
# Linux arm64 Rust 1.94. No cargo update. No PostgreSQL.
set -u
echo "oo_task=sp07-reviewed-effective-review-preoverlay"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a228"
echo "oo_fixed=9daeaa05e34c154391a757fc78fb47f09c954c6a"
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
mkdir -p /evidence/linux/preoverlay/phases /cargo-home/bin
cd /src

record() {
  mkdir -p /evidence/linux/preoverlay/phases
  printf '%s\n' "$2" > "/evidence/linux/preoverlay/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/preoverlay/smoke.exit
    exit "$2"
  fi
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/linux/preoverlay/rustc-version.txt
rustc -Vv > /evidence/linux/preoverlay/rustc-verbose.txt
cargo -V | tee /evidence/linux/preoverlay/cargo-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
record toolchain 0

rustc --edition 2021 -D warnings /tools/sha256.rs -o /cargo-home/bin/sha256
record sha256-tool $?
export PATH="/cargo-home/bin:${PATH}"

if [ ! -f crates/oo-builder/src/reviewed_effective.rs ]; then
  echo "fixed archive is missing the product module" >&2
  record source 2
fi
if [ -f crates/oo-builder/tests/reviewed_effective_a228_review.rs ]; then
  echo "review harness is already on the pre-overlay archive" >&2
  record source 2
fi
if [ -f rust-toolchain.toml ]; then
  echo "extracted tree still selects a toolchain file" >&2
  record source 2
fi
printf '%s\n' "review_harness_absent" > /evidence/linux/preoverlay/overlay-absent.txt
record source 0

cmp -s crates/oo-builder/src/reviewed_effective.rs /blobs/reviewed_effective.rs \
  && cmp -s crates/oo-builder/src/lib.rs /blobs/lib.rs \
  && cmp -s crates/oo-builder/src/bin/reviewed-effective-oos.rs /blobs/reviewed-effective-oos.rs \
  && cmp -s crates/oo-builder/tests/reviewed_effective.rs /blobs/reviewed_effective_test.rs \
  && cmp -s contracts/builder/reviewed-effective-oos-v1.md /blobs/reviewed-effective-oos-v1.md \
  && cmp -s crates/oo-builder/Cargo.toml /blobs/Cargo.toml \
  && cmp -s Cargo.lock /blobs/Cargo.lock \
  && cmp -s crates/oo-builder/src/role_pack.rs /blobs/role_pack.rs \
  && cmp -s crates/oo-builder/src/captured_pg.rs /blobs/captured_pg.rs \
  && cmp -s crates/oo-builder/src/bin/oo-builder.rs /blobs/oo-builder.rs
blob_cmp=$?
printf '%s\n' "$blob_cmp" > /evidence/linux/preoverlay/blob-cmp.exit
if [ "$blob_cmp" -ne 0 ]; then
  echo "archive drifted from the saved blobs" >&2
  record blob-cmp 2
fi
record blob-cmp 0

/cargo-home/bin/sha256 file \
  crates/oo-builder/src/reviewed_effective.rs \
  crates/oo-builder/src/lib.rs \
  crates/oo-builder/src/bin/reviewed-effective-oos.rs \
  crates/oo-builder/tests/reviewed_effective.rs \
  contracts/builder/reviewed-effective-oos-v1.md \
  Cargo.lock \
  > /evidence/linux/preoverlay/archive-sha256.txt
record owned-hash $?

printf '%s\n' "no" > /evidence/linux/preoverlay/cargo-update-invoked.txt

echo "=== cargo fetch --locked --offline ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked --offline > /evidence/linux/preoverlay/fetch-offline.log 2>&1
offline_fetch=$?
set -e
printf '%s\n' "$offline_fetch" > /evidence/linux/preoverlay/phases/fetch-offline.exit
if [ "$offline_fetch" -ne 0 ]; then
  echo "=== cargo fetch --locked ==="
  set +e
  cargo fetch --manifest-path Cargo.toml --locked > /evidence/linux/preoverlay/fetch.log 2>&1
  fetch_exit=$?
  set -e
else
  cp /evidence/linux/preoverlay/fetch-offline.log /evidence/linux/preoverlay/fetch.log
  fetch_exit=0
fi
record fetch "$fetch_exit"

echo "=== cargo test --lib reviewed_effective ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder --lib reviewed_effective -- --test-threads=1 \
  > /evidence/linux/preoverlay/lib-test.log 2>&1
lib_exit=$?
set -e
record lib-test "$lib_exit"

echo "=== cargo test --test reviewed_effective ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder --test reviewed_effective -- --test-threads=1 \
  > /evidence/linux/preoverlay/cli-test.log 2>&1
cli_exit=$?
set -e
record cli-test "$cli_exit"

/cargo-home/bin/sha256 file Cargo.lock | awk '{print $1}' > /evidence/linux/preoverlay/lock-after.sha256
before=$(awk '$2 ~ /Cargo.lock$/ {print $1}' /evidence/linux/preoverlay/archive-sha256.txt)
after=$(cat /evidence/linux/preoverlay/lock-after.sha256)
if [ "$before" != "$after" ]; then
  echo "lock changed before=$before after=$after" > /evidence/linux/preoverlay/lock-hash.txt
  record lock-hash 1
fi
echo "unchanged $after" > /evidence/linux/preoverlay/lock-hash.txt
record lock-hash 0

printf '%s\n' 0 > /evidence/linux/preoverlay/smoke.exit
echo "preoverlay_smoke=ok"
exit 0
