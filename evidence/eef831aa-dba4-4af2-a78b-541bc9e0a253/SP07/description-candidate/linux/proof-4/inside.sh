#!/bin/bash
# Fixed 3f8807b archive plus the description-candidate overlay.
# Linux arm64 Rust 1.94. No cargo update. No apt. No PostgreSQL. No model.
set -u
echo "oo_task=sp07-description-candidate-proof"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a253"
echo "oo_fixed=3f8807b495bd78ee1d8c99aca41c54c3137778c6"
echo "oo_platform=linux/arm64"
echo "phase=proof-4"
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
rustc -Vv > /evidence/linux/proof-4/rustc-verbose.txt
cargo -V | tee /evidence/linux/proof-4/cargo-version.txt
rustup show > /evidence/linux/proof-4/rustup-show.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
record toolchain 0

rustc --edition 2021 -D warnings /tools/sha256.rs -o /cargo-home/bin/sha256
record sha256-tool $?
export PATH="/cargo-home/bin:${PATH}"

if [ -e crates/oo-builder/src/description_candidate.rs ]; then
  echo "archive copy already has description_candidate.rs before overlay" >&2
  record source 2
fi
cp /overlay/crates/oo-builder/src/lib.rs crates/oo-builder/src/lib.rs
cp /overlay/crates/oo-builder/src/description_candidate.rs crates/oo-builder/src/description_candidate.rs
cp /overlay/crates/oo-builder/tests/description_candidate.rs crates/oo-builder/tests/description_candidate.rs
mkdir -p contracts/builder
cp /overlay/contracts/builder/description-candidate-v1.md contracts/builder/description-candidate-v1.md
diff -rq /src /cargo-home/work/src | LC_ALL=C sort > /evidence/linux/proof-4/tree-diff.txt || true
cat > /evidence/linux/proof-4/tree-diff-expected.txt <<'EOF'
Files /src/crates/oo-builder/src/lib.rs and /cargo-home/work/src/crates/oo-builder/src/lib.rs differ
Only in /cargo-home/work/src/contracts/builder: description-candidate-v1.md
Only in /cargo-home/work/src/crates/oo-builder/src: description_candidate.rs
Only in /cargo-home/work/src/crates/oo-builder/tests: description_candidate.rs
Only in /cargo-home/work/src: rust-toolchain.toml.aside
Only in /src: rust-toolchain.toml
EOF
if ! cmp -s /evidence/linux/proof-4/tree-diff.txt /evidence/linux/proof-4/tree-diff-expected.txt; then
  echo "overlay tree diff is not the owned set" >&2
  record overlay 2
fi
record overlay 0
/cargo-home/bin/sha256 file \
  crates/oo-builder/src/lib.rs \
  crates/oo-builder/src/description_candidate.rs \
  crates/oo-builder/tests/description_candidate.rs \
  contracts/builder/description-candidate-v1.md \
  Cargo.lock \
  > /evidence/linux/proof-4/owned-sha256.txt
record owned-hash $?
printf '%s\n' "no" > /evidence/linux/proof-4/cargo-update-invoked.txt

echo "=== cargo fetch --locked --offline ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked --offline > /evidence/linux/proof-4/fetch.log 2>&1
fetch_exit=$?
set -e
record fetch "$fetch_exit"

echo "=== description_candidate lib tests ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder --lib description_candidate::tests -- --test-threads=1 \
  > /evidence/linux/proof-4/test-lib.log 2>&1
lib_exit=$?
set -e
record test-lib "$lib_exit"

echo "=== description_candidate integration tests ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder --test description_candidate -- --test-threads=1 \
  > /evidence/linux/proof-4/test-description.log 2>&1
test_exit=$?
set -e
record test-description "$test_exit"

echo "=== clippy description candidate surface ==="
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
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-builder --lib --test description_candidate -- -D warnings \
  > /evidence/linux/proof-4/clippy.log 2>&1
clippy_exit=$?
set -e
record clippy "$clippy_exit"

/cargo-home/bin/sha256 file Cargo.lock | awk '{print $1}' > /evidence/linux/proof-4/lock-after.sha256
/cargo-home/bin/sha256 file /src/Cargo.lock | awk '{print $1}' > /evidence/linux/proof-4/lock-archive.sha256
after=$(cat /evidence/linux/proof-4/lock-after.sha256)
archive=$(cat /evidence/linux/proof-4/lock-archive.sha256)
if [ "$after" != "$archive" ]; then
  echo "lock changed archive=$archive after=$after" > /evidence/linux/proof-4/lock-hash.txt
  record lock-hash 1
fi
echo "unchanged $after" > /evidence/linux/proof-4/lock-hash.txt
record lock-hash 0

printf '%s\n' 0 > /evidence/linux/proof-4/smoke.exit
echo "proof=ok"
exit 0
