#!/bin/bash
# Linux arm64 Rust 1.94 proof of the description provider.
# Fixed archive 182e2137 plus the provider tree mounted from the working copy.
# No cargo update. No apt. No PostgreSQL. No production network call.
# Origin inside: a258 commit 1cd687d3f106fb9bde8c2a4a0b40cf2dddae9368
#   evidence/eef831aa-dba4-4af2-a78b-541bc9e0a258/SP07/description-candidate-cli/linux/proof/inside.sh
#   blob c967e03f8afa3082d4c342c11e58f09ff023ec0b
# Changes: owner a263, provider overlay, separate provider target dir,
# lock subset check, description-candidate binary for one parity test.
set -u
echo "oo_task=sp07-description-provider-proof"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a263"
echo "oo_fixed=182e2137ee261ed889b1aff5ada6e505a4a17723"
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
mkdir -p /evidence/linux/proof-4/phases /cargo-home/bin /cargo-home/work
rm -rf /cargo-home/work/src
mkdir -p /cargo-home/work/src
cp -a /src/. /cargo-home/work/src/
cd /cargo-home/work/src
if [ -f rust-toolchain.toml ]; then
  mv rust-toolchain.toml rust-toolchain.toml.aside
fi
printf '%s\n' "moved-aside" > /evidence/linux/proof-4/toolchain-aside.txt

record() {
  mkdir -p /evidence/linux/proof-4/phases
  printf '%s\n' "$2" > "/evidence/linux/proof-4/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/proof-4/smoke.exit
    exit "$2"
  fi
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/linux/proof-4/rustc-version.txt
cargo -V | tee /evidence/linux/proof-4/cargo-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
record toolchain 0

rustc --edition 2021 -D warnings /tools/sha256.rs -o /cargo-home/bin/sha256
record sha256-tool $?
rustc --edition 2021 -D warnings /tools/lock_subset.rs -o /cargo-home/bin/lock-subset
record lock-subset-tool $?
export PATH="/cargo-home/bin:${PATH}"

if [ ! -f crates/oo-builder/src/bin/description-candidate.rs ]; then
  echo "fixed archive is missing the description-candidate CLI" >&2
  record source 2
fi
if [ -e tools/description-provider ]; then
  echo "description provider is already on the fixed archive" >&2
  record source 2
fi
printf '%s\n' "provider_absent_before_overlay" > /evidence/linux/proof-4/overlay-before.txt
/cargo-home/bin/sha256 file Cargo.lock | awk '{print $1}' > /evidence/linux/proof-4/lock-before.sha256
record source 0
printf '%s\n' "no" > /evidence/linux/proof-4/cargo-update-invoked.txt

echo "=== cargo fetch root --locked --offline ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked --offline > /evidence/linux/proof-4/fetch-root.log 2>&1
fetch_root=$?
set -e
record fetch-root "$fetch_root"

echo "=== build description-candidate ==="
set +e
cargo build --manifest-path Cargo.toml --locked --offline -p oo-builder --bin description-candidate \
  > /evidence/linux/proof-4/build-cli.log 2>&1
build_exit=$?
set -e
record build-cli "$build_exit"
if [ ! -x /target/debug/description-candidate ]; then
  echo "description-candidate binary is missing" >&2
  record cli-bin 2
fi
record cli-bin 0

rm -rf tools/description-provider
mkdir -p tools
cp -a /provider/. tools/description-provider/
rm -rf tools/description-provider/target
if [ -f tools/description-provider/Cargo.lock ]; then
  echo "provider overlay already had a lock; replacing it from the root lock" > /evidence/linux/proof-4/lock-note.txt
  rm tools/description-provider/Cargo.lock
fi
cp Cargo.lock tools/description-provider/Cargo.lock
printf '%s\n' "provider_overlaid" > /evidence/linux/proof-4/overlay-after.txt

export CARGO_TARGET_DIR=/target/provider
echo "=== cargo fetch provider ==="
set +e
cargo fetch --manifest-path tools/description-provider/Cargo.toml > /evidence/linux/proof-4/fetch-provider.log 2>&1
fetch_provider=$?
set -e
record fetch-provider "$fetch_provider"
cp tools/description-provider/Cargo.lock /evidence/linux/proof-4/provider-Cargo.lock

echo "=== lock subset ==="
set +e
/cargo-home/bin/lock-subset Cargo.lock tools/description-provider/Cargo.lock \
  > /evidence/linux/proof-4/lock-subset.txt 2> /evidence/linux/proof-4/lock-subset.err
subset_exit=$?
set -e
record lock-subset "$subset_exit"

echo "=== cargo fetch provider --locked --offline ==="
set +e
cargo fetch --manifest-path tools/description-provider/Cargo.toml --locked --offline \
  > /evidence/linux/proof-4/fetch-provider-offline.log 2>&1
offline_exit=$?
set -e
record fetch-provider-offline "$offline_exit"

echo "=== description-provider tests ==="
set +e
OO_A263_DESCRIPTION_CANDIDATE=/target/debug/description-candidate \
  cargo test --manifest-path tools/description-provider/Cargo.toml --locked --offline \
  -- --test-threads=1 > /evidence/linux/proof-4/test.log 2>&1
test_exit=$?
set -e
record test "$test_exit"

echo "=== clippy description-provider ==="
if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/linux/proof-4/clippy-component.log 2>&1
  component_exit=$?
  set -e
  record clippy-component "$component_exit"
else
  printf '%s\n' "already-installed" > /evidence/linux/proof-4/clippy-component.log
  record clippy-component 0
fi
set +e
cargo clippy --manifest-path tools/description-provider/Cargo.toml --locked --offline --all-targets -- -D warnings \
  > /evidence/linux/proof-4/clippy.log 2>&1
clippy_exit=$?
set -e
record clippy "$clippy_exit"

export CARGO_TARGET_DIR=/target
/cargo-home/bin/sha256 file Cargo.lock | awk '{print $1}' > /evidence/linux/proof-4/lock-after.sha256
before=$(cat /evidence/linux/proof-4/lock-before.sha256)
after=$(cat /evidence/linux/proof-4/lock-after.sha256)
if [ "$before" != "$after" ]; then
  echo "root lock changed before=$before after=$after" > /evidence/linux/proof-4/lock-hash.txt
  record lock-hash 1
fi
echo "unchanged $after" > /evidence/linux/proof-4/lock-hash.txt
record lock-hash 0
printf '%s\n' 0 > /evidence/linux/proof-4/smoke.exit
echo "proof=ok"
exit 0
