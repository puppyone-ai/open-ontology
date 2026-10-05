#!/bin/bash
# Review harness on a fresh archive of 9daeaa05. Product bytes are the commit. No PostgreSQL.
set -u
echo "oo_task=sp07-reviewed-effective-review-overlay"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a228"
echo "oo_fixed=9daeaa05e34c154391a757fc78fb47f09c954c6a"
echo "phase=overlay"
uname -a
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
export OO_A228_ORACLE_DIR=/evidence/linux/overlay/oracle
mkdir -p /evidence/linux/overlay/phases /evidence/linux/overlay/oracle /cargo-home/bin
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
rustc -Vv > /evidence/linux/overlay/rustc-verbose.txt
cargo -V | tee /evidence/linux/overlay/cargo-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
record toolchain 0

if [ ! -f crates/oo-builder/tests/reviewed_effective_a228_review.rs ]; then
  echo "review harness is missing" >&2
  record source 2
fi
if [ -f rust-toolchain.toml ]; then
  echo "extracted tree still selects a toolchain file" >&2
  record source 2
fi
printf '%s\n' "review_harness_present" > /evidence/linux/overlay/overlay-present.txt
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
  && cmp -s crates/oo-builder/src/bin/oo-builder.rs /blobs/oo-builder.rs \
  && cmp -s crates/oo-builder/tests/reviewed_effective_a228_review.rs /evidence/overlay/reviewed_effective_a228_review.rs
blob_cmp=$?
printf '%s\n' "$blob_cmp" > /evidence/linux/overlay/blob-cmp.exit
if [ "$blob_cmp" -ne 0 ]; then
  echo "archive or harness drifted" >&2
  record blob-cmp 2
fi
record blob-cmp 0

rustc --edition 2021 -D warnings /tools/sha256.rs -o /cargo-home/bin/sha256
record sha256-tool $?
export PATH="/cargo-home/bin:${PATH}"

/cargo-home/bin/sha256 file \
  crates/oo-builder/src/reviewed_effective.rs \
  crates/oo-builder/src/lib.rs \
  crates/oo-builder/src/bin/reviewed-effective-oos.rs \
  crates/oo-builder/tests/reviewed_effective.rs \
  crates/oo-builder/tests/reviewed_effective_a228_review.rs \
  contracts/builder/reviewed-effective-oos-v1.md \
  Cargo.lock \
  > /evidence/linux/overlay/source-sha256.txt
record owned-hash $?

printf '%s\n' "no" > /evidence/linux/overlay/cargo-update-invoked.txt

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/linux/overlay/clippy-component.log 2>&1
  clippy_add=$?
  set -e
  record clippy-component "$clippy_add"
else
  echo "clippy_component=already_installed" > /evidence/linux/overlay/clippy-component.log
  record clippy-component 0
fi

echo "=== cargo fetch --locked --offline ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked --offline > /evidence/linux/overlay/fetch.log 2>&1
fetch_exit=$?
set -e
record fetch "$fetch_exit"

echo "=== cargo test reviewed_effective_a228_review ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder --test reviewed_effective_a228_review -- --test-threads=1 --nocapture \
  > /evidence/linux/overlay/test.log 2>&1
test_exit=$?
set -e
record test "$test_exit"

echo "=== cargo clippy ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-builder --lib --bins --test reviewed_effective --test reviewed_effective_a228_review -- -D warnings \
  > /evidence/linux/overlay/clippy.log 2>&1
clippy_exit=$?
set -e
record clippy "$clippy_exit"

/cargo-home/bin/sha256 file Cargo.lock | awk '{print $1}' > /evidence/linux/overlay/lock-after.sha256
before=$(awk '$2 ~ /Cargo.lock$/ {print $1}' /evidence/linux/overlay/source-sha256.txt)
after=$(cat /evidence/linux/overlay/lock-after.sha256)
if [ "$before" != "$after" ]; then
  echo "lock changed before=$before after=$after" > /evidence/linux/overlay/lock-hash.txt
  record lock-hash 1
fi
echo "unchanged $after" > /evidence/linux/overlay/lock-hash.txt
record lock-hash 0

printf '%s\n' 0 > /evidence/linux/overlay/overlay.exit
echo "overlay_review=ok"
exit 0
