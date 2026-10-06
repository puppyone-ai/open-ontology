#!/bin/bash
# Fixed fbc10b04 archive plus the description-candidate CLI overlay.
# Linux arm64 Rust 1.94. No cargo update. No apt. No PostgreSQL. No model.
# Origin inside: a253 commit 268e46972458fe53dd54412ab277b2cd09552a82
#   evidence/eef831aa-dba4-4af2-a78b-541bc9e0a253/SP07/description-candidate/linux/proof-7/inside.sh
#   blob 9f15b1b72a8f53f045107a42d6a977d90d75cde1
# Changes: overlay is the CLI bin, CLI test, and CLI contract only.
set -u
echo "oo_task=sp07-description-candidate-cli-proof"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a258"
echo "oo_fixed=fbc10b04017bd7e0eb922c3c60732bbdf9a7f46a"
echo "oo_source=827e768b9f94af6134a5734bd16e066ab0d47528"
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
mkdir -p /evidence/linux/proof/phases /cargo-home/bin /cargo-home/work
rm -rf /cargo-home/work/src
mkdir -p /cargo-home/work/src
cp -a /src/. /cargo-home/work/src/
cd /cargo-home/work/src
if [ -f rust-toolchain.toml ]; then
  mv rust-toolchain.toml rust-toolchain.toml.aside
fi
printf '%s\n' "moved-aside" > /evidence/linux/proof/toolchain-aside.txt

record() {
  mkdir -p /evidence/linux/proof/phases
  printf '%s\n' "$2" > "/evidence/linux/proof/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/proof/smoke.exit
    exit "$2"
  fi
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/linux/proof/rustc-version.txt
rustc -Vv > /evidence/linux/proof/rustc-verbose.txt
cargo -V | tee /evidence/linux/proof/cargo-version.txt
rustup show > /evidence/linux/proof/rustup-show.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
record toolchain 0

rustc --edition 2021 -D warnings /tools/sha256.rs -o /cargo-home/bin/sha256
record sha256-tool $?
export PATH="/cargo-home/bin:${PATH}"

if [ ! -f crates/oo-builder/src/description_candidate.rs ]; then
  echo "archive is missing the description candidate library" >&2
  record source 2
fi
if [ -e crates/oo-builder/src/bin/description-candidate.rs ]; then
  echo "archive already has the CLI before overlay" >&2
  record source 2
fi
mkdir -p crates/oo-builder/src/bin contracts/builder
cp /overlay/crates/oo-builder/src/bin/description-candidate.rs crates/oo-builder/src/bin/description-candidate.rs
cp /overlay/crates/oo-builder/tests/description_candidate_cli.rs crates/oo-builder/tests/description_candidate_cli.rs
cp /overlay/contracts/builder/description-candidate-cli-v1.md contracts/builder/description-candidate-cli-v1.md
diff -rq /src /cargo-home/work/src | LC_ALL=C sort > /evidence/linux/proof/tree-diff.txt || true
cat > /evidence/linux/proof/tree-diff-expected.txt <<'EOF'
Only in /cargo-home/work/src/contracts/builder: description-candidate-cli-v1.md
Only in /cargo-home/work/src/crates/oo-builder/src/bin: description-candidate.rs
Only in /cargo-home/work/src/crates/oo-builder/tests: description_candidate_cli.rs
Only in /cargo-home/work/src: rust-toolchain.toml.aside
Only in /src: rust-toolchain.toml
EOF
if ! cmp -s /evidence/linux/proof/tree-diff.txt /evidence/linux/proof/tree-diff-expected.txt; then
  echo "overlay tree diff is not the owned set" >&2
  record overlay 2
fi
record overlay 0
/cargo-home/bin/sha256 file \
  crates/oo-builder/src/bin/description-candidate.rs \
  crates/oo-builder/tests/description_candidate_cli.rs \
  contracts/builder/description-candidate-cli-v1.md \
  Cargo.lock \
  > /evidence/linux/proof/owned-sha256.txt
record owned-hash $?
printf '%s\n' "no" > /evidence/linux/proof/cargo-update-invoked.txt

echo "=== cargo fetch --locked --offline ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked --offline > /evidence/linux/proof/fetch.log 2>&1
fetch_exit=$?
set -e
record fetch "$fetch_exit"

echo "=== description-candidate CLI tests ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder --test description_candidate_cli -- --test-threads=1 \
  > /evidence/linux/proof/test-cli.log 2>&1
test_exit=$?
set -e
record test-cli "$test_exit"

echo "=== clippy description-candidate CLI ==="
if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/linux/proof/clippy-component.log 2>&1
  component_exit=$?
  set -e
  record clippy-component "$component_exit"
else
  printf '%s\n' "already-installed" > /evidence/linux/proof/clippy-component.log
  record clippy-component 0
fi
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-builder --bin description-candidate --test description_candidate_cli -- -D warnings \
  > /evidence/linux/proof/clippy.log 2>&1
clippy_exit=$?
set -e
record clippy "$clippy_exit"

/cargo-home/bin/sha256 file Cargo.lock | awk '{print $1}' > /evidence/linux/proof/lock-after.sha256
/cargo-home/bin/sha256 file /src/Cargo.lock | awk '{print $1}' > /evidence/linux/proof/lock-archive.sha256
after=$(cat /evidence/linux/proof/lock-after.sha256)
archive=$(cat /evidence/linux/proof/lock-archive.sha256)
if [ "$after" != "$archive" ]; then
  echo "lock changed archive=$archive after=$after" > /evidence/linux/proof/lock-hash.txt
  record lock-hash 1
fi
echo "unchanged $after" > /evidence/linux/proof/lock-hash.txt
record lock-hash 0

printf '%s\n' 0 > /evidence/linux/proof/smoke.exit
echo "proof=ok"
exit 0
