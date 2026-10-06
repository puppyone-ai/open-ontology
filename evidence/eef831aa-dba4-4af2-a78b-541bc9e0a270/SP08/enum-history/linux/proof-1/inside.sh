#!/bin/bash
# Linux arm64 Rust 1.94 proof of enum-history.
# Fixed archive 0bcd494 plus the tool overlay. No cargo update. No PostgreSQL.
# Origin inside:
#   evidence/eef831aa-dba4-4af2-a78b-541bc9e0a263/SP07/description-provider/linux/proof-7/inside.sh
#   sha256 aac405f57524b1f4004ebecf2ead7aa33c07829860d9f7f167c58e4b3388f01c
#   commit efdffed87f4e19070536cbd6a72fa1a6583e639e
# Deltas: cp -R (not cp -a); no description-candidate build; overlay is
# tools/enum-history; path packages are enum-history and oo-core only.
set -u
echo "oo_task=sp08-enum-history-proof-1"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a270"
echo "oo_fixed=0bcd494457091d4b55c82fc6b9922b5bc5dc44c4"
echo "oo_platform=linux/arm64"
echo "phase=proof-1"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
mkdir -p /cargo-home/bin /cargo-home/work
rm -rf /cargo-home/work/src
mkdir -p /cargo-home/work/src
cp -R /src/. /cargo-home/work/src/
cd /cargo-home/work/src
if [ -f rust-toolchain.toml ]; then
  mv rust-toolchain.toml rust-toolchain.toml.aside
fi
printf '%s\n' "moved-aside" > /evidence/toolchain-aside.txt
printf '%s\n' "no" > /evidence/cargo-update-invoked.txt

record() {
  printf '%s\n' "$2" > "/evidence/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/smoke.exit
    exit "$2"
  fi
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/rustc-version.txt
cargo -V | tee /evidence/cargo-version.txt
rustc -vV > /evidence/rustc-verbose.txt
case "$got" in
  "rustc 1.94.0 "*) record version 0 ;;
  *) echo "refusing toolchain: $got" >&2; record version 2 ;;
esac

mkdir -p /cargo-home/bin
rustc --edition 2021 -D warnings /tools/lock_subset.rs -o /cargo-home/bin/lock-subset
record lock-subset-tool $?
export PATH="/cargo-home/bin:${PATH}"

if [ -e tools/enum-history ]; then
  echo "enum-history is already on the fixed archive" >&2
  record absence 2
fi
printf '%s\n' "enum-history absent" > /evidence/absence.txt
record absence 0

sha256sum examples/schema/enum-ysk-zt.yaml crates/oo-core/src/oos/parse.rs crates/oo-core/src/oos/model.rs \
  > /evidence/archive-sha256.txt
example_hash=$(sha256sum examples/schema/enum-ysk-zt.yaml | awk '{print $1}')
if [ "$example_hash" != "07d7e26d32edaba62bc2287cfec6c22816c78ce784b22b5df42ef7d437214839" ]; then
  echo "enum example hash drifted: $example_hash" >&2
  record example-hash 2
fi
record example-hash 0
sha256sum Cargo.lock | awk '{print $1}' > /evidence/lock-before.sha256

echo "=== cargo fetch root locked offline ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked --offline > /evidence/fetch-root.stdout 2> /evidence/fetch-root.stderr
fetch_root=$?
set -e
record fetch-root "$fetch_root"

echo "=== overlay enum-history ==="
rm -rf tools/enum-history
mkdir -p tools/enum-history
cp -R /tool/. tools/enum-history/
find tools/enum-history -name '._*' -delete
find tools/enum-history -name '.DS_Store' -delete
rm -rf tools/enum-history/target
if [ -f tools/enum-history/Cargo.lock ]; then
  echo "overlay already had a lock; replacing it from the root lock" > /evidence/lock-note.txt
  rm tools/enum-history/Cargo.lock
fi
cp Cargo.lock tools/enum-history/Cargo.lock
if [ ! -f tools/enum-history/src/lib.rs ]; then
  echo "overlay is missing src/lib.rs" >&2
  record overlay 2
fi
find tools/enum-history -type f | sort | xargs sha256sum > /evidence/source-sha256.txt
printf '%s\n' "enum-history overlaid" > /evidence/overlay-after.txt
record overlay 0

export CARGO_TARGET_DIR=/target/enum-history
echo "=== cargo fetch enum-history ==="
set +e
cargo fetch --manifest-path tools/enum-history/Cargo.toml > /evidence/fetch-child.stdout 2> /evidence/fetch-child.stderr
fetch_child=$?
set -e
if [ -f tools/enum-history/Cargo.lock ]; then
  cp tools/enum-history/Cargo.lock /evidence/Cargo.lock
fi
record fetch-child "$fetch_child"

echo "=== lock subset ==="
set +e
/cargo-home/bin/lock-subset Cargo.lock tools/enum-history/Cargo.lock \
  > /evidence/lock-subset.txt 2> /evidence/lock-subset.err
subset_exit=$?
set -e
record lock-subset "$subset_exit"

echo "=== cargo fetch enum-history locked offline ==="
set +e
cargo fetch --manifest-path tools/enum-history/Cargo.toml --locked --offline \
  > /evidence/fetch-child-offline.stdout 2> /evidence/fetch-child-offline.stderr
offline_exit=$?
set -e
record fetch-child-offline "$offline_exit"

echo "=== enum-history tests ==="
set +e
cargo test --manifest-path tools/enum-history/Cargo.toml --locked --offline -- --test-threads=1 \
  > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
set -e
record test "$test_exit"

echo "=== rustup component add clippy ==="
if rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  printf '%s\n' "already-installed" > /evidence/clippy-component.stdout
  record clippy-component 0
else
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/clippy-component.stdout 2> /evidence/clippy-component.stderr
  component_exit=$?
  set -e
  record clippy-component "$component_exit"
fi

echo "=== cargo clippy enum-history ==="
set +e
cargo clippy --manifest-path tools/enum-history/Cargo.toml --locked --offline --all-targets -- -D warnings \
  > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
record clippy "$clippy_exit"

export CARGO_TARGET_DIR=/target
sha256sum Cargo.lock | awk '{print $1}' > /evidence/lock-after.sha256
before=$(cat /evidence/lock-before.sha256)
after=$(cat /evidence/lock-after.sha256)
if [ "$before" != "$after" ]; then
  echo "root lock changed before=$before after=$after" > /evidence/lock-hash.txt
  record lock-hash 1
fi
echo "unchanged $after" > /evidence/lock-hash.txt
record lock-hash 0
printf '%s\n' 0 > /evidence/smoke.exit
echo "proof=ok"
exit 0
