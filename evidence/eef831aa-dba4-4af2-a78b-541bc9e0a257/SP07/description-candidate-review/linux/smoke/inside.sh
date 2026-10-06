#!/bin/bash
# Focused MSRV smoke of fixed fbc10b04. No review overlay. No PostgreSQL. No model.
# The commit pins rust-toolchain 1.98.1, so the writable copy moves that file aside
# and RUSTUP_TOOLCHAIN selects 1.94.0. The archive mount stays read-only.
set -u
echo "oo_task=sp07-description-candidate-review-smoke"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a257"
echo "oo_fixed=fbc10b04017bd7e0eb922c3c60732bbdf9a7f46a"
echo "oo_platform=linux/arm64"
echo "phase=smoke"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
mkdir -p /evidence/phases /cargo-home/bin /cargo-home/work
rm -rf /cargo-home/work/src
mkdir -p /cargo-home/work/src
cp -a /src/. /cargo-home/work/src/
cd /cargo-home/work/src
if [ -f rust-toolchain.toml ]; then
  mv rust-toolchain.toml rust-toolchain.toml.aside
fi
printf '%s\n' "moved-aside" > /evidence/toolchain-aside.txt

record() {
  mkdir -p /evidence/phases
  printf '%s\n' "$2" > "/evidence/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/smoke.exit
    exit "$2"
  fi
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/rustc-version.txt
rustc -Vv > /evidence/rustc-verbose.txt
cargo -V | tee /evidence/cargo-version.txt
rustup show > /evidence/rustup-show.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
record toolchain 0

rustc --edition 2021 -D warnings /tools/sha256.rs -o /cargo-home/bin/sha256
record sha256-tool $?
export PATH="/cargo-home/bin:${PATH}"

if [ ! -f crates/oo-builder/src/description_candidate.rs ]; then
  echo "fixed archive is missing description_candidate.rs" >&2
  record source 2
fi
if [ -e crates/oo-builder/tests/description_candidate_a257.rs ]; then
  echo "review overlay is present during smoke" >&2
  record source 2
fi
printf '%s\n' "overlay_absent" > /evidence/overlay-absent.txt
if ! cmp -s crates/oo-builder/src/description_candidate.rs /blobs/description_candidate.rs \
  || ! cmp -s crates/oo-builder/src/lib.rs /blobs/lib.rs \
  || ! cmp -s crates/oo-builder/tests/description_candidate.rs /blobs/description_candidate_test.rs \
  || ! cmp -s contracts/builder/description-candidate-v1.md /blobs/description-candidate-v1.md \
  || ! cmp -s Cargo.lock /blobs/Cargo.lock
then
  echo "writable copy drifted from the fixed blobs" >&2
  record source 2
fi
record source 0
/cargo-home/bin/sha256 file \
  crates/oo-builder/src/description_candidate.rs \
  crates/oo-builder/src/lib.rs \
  crates/oo-builder/tests/description_candidate.rs \
  contracts/builder/description-candidate-v1.md \
  Cargo.lock \
  > /evidence/source-sha256.txt
record owned-hash $?
diff -rq /src /cargo-home/work/src | LC_ALL=C sort > /evidence/tree-diff.txt || true
cat > /evidence/tree-diff-expected.txt <<'EOF'
Only in /cargo-home/work/src: rust-toolchain.toml.aside
Only in /src: rust-toolchain.toml
EOF
if ! cmp -s /evidence/tree-diff.txt /evidence/tree-diff-expected.txt; then
  echo "smoke tree diff is not toolchain-aside only" >&2
  record tree 2
fi
record tree 0
printf '%s\n' "no" > /evidence/cargo-update-invoked.txt

echo "=== cargo fetch --locked --offline ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked --offline > /evidence/fetch-offline.log 2>&1
offline_fetch=$?
set -e
printf '%s\n' "$offline_fetch" > /evidence/phases/fetch-offline.exit
if [ "$offline_fetch" -ne 0 ]; then
  echo "=== cargo fetch --locked ==="
  set +e
  cargo fetch --manifest-path Cargo.toml --locked > /evidence/fetch.log 2>&1
  fetch_exit=$?
  set -e
else
  cp /evidence/fetch-offline.log /evidence/fetch.log
  fetch_exit=0
fi
record fetch "$fetch_exit"

echo "=== description_candidate lib tests ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder --lib description_candidate -- --test-threads=1 \
  > /evidence/test-lib.log 2>&1
lib_exit=$?
set -e
record test-lib "$lib_exit"

echo "=== description_candidate integration tests ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder --test description_candidate -- --test-threads=1 \
  > /evidence/test-description.log 2>&1
test_exit=$?
set -e
record test-description "$test_exit"

/cargo-home/bin/sha256 file Cargo.lock | awk '{print $1}' > /evidence/lock-after.sha256
before=$(/cargo-home/bin/sha256 file /blobs/Cargo.lock | awk '{print $1}')
after=$(cat /evidence/lock-after.sha256)
printf '%s\n' "$before" > /evidence/lock-archive.sha256
if [ "$before" != "$after" ]; then
  echo "lock changed before=$before after=$after" > /evidence/lock-hash.txt
  record lock-hash 1
fi
echo "unchanged $after" > /evidence/lock-hash.txt
record lock-hash 0

printf '%s\n' 0 > /evidence/smoke.exit
echo "smoke=ok"
exit 0
