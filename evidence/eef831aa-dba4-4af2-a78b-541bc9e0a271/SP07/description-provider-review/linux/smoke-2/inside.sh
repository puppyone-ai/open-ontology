#!/bin/bash
# Focused MSRV smoke of fixed 9951cda7. No review overlay. No PostgreSQL. No model.
# The commit pins a newer rust-toolchain, so the writable copy moves that file aside
# and RUSTUP_TOOLCHAIN selects 1.94.0. The archive mount stays read-only.
set -u
echo "oo_task=sp07-description-provider-review-smoke"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a271"
echo "oo_fixed=9951cda7f128e30f1f6ce02e409d542d20a9e4fb"
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
rustc --edition 2021 -D warnings /tools/lock_subset.rs -o /cargo-home/bin/lock-subset
record lock-subset-tool $?
export PATH="/cargo-home/bin:${PATH}"

for path in \
  tools/description-provider/Cargo.toml \
  tools/description-provider/Cargo.lock \
  tools/description-provider/src/lib.rs \
  tools/description-provider/src/transport.rs \
  tools/description-provider/tests/provider.rs \
  crates/oo-builder/src/description_candidate.rs \
  crates/oo-builder/src/bin/description-candidate.rs \
  crates/oo-builder/src/lib.rs \
  Cargo.lock
do
  if [ ! -f "$path" ]; then
    echo "fixed archive is missing $path" >&2
    record source 2
  fi
done
if [ -e tools/description-provider/tests/provider_a271.rs ]; then
  echo "review overlay is present during smoke" >&2
  record source 2
fi
if ! cmp -s tools/description-provider/src/lib.rs /blobs/tools/description-provider/src/lib.rs \
  || ! cmp -s tools/description-provider/src/transport.rs /blobs/tools/description-provider/src/transport.rs \
  || ! cmp -s tools/description-provider/tests/provider.rs /blobs/tools/description-provider/tests/provider.rs \
  || ! cmp -s tools/description-provider/Cargo.lock /blobs/tools/description-provider/Cargo.lock \
  || ! cmp -s tools/description-provider/Cargo.toml /blobs/tools/description-provider/Cargo.toml \
  || ! cmp -s crates/oo-builder/src/description_candidate.rs /blobs/crates/oo-builder/src/description_candidate.rs \
  || ! cmp -s crates/oo-builder/src/lib.rs /blobs/crates/oo-builder/src/lib.rs \
  || ! cmp -s crates/oo-builder/src/bin/description-candidate.rs /blobs/crates/oo-builder/src/bin/description-candidate.rs \
  || ! cmp -s Cargo.lock /blobs/Cargo.lock
then
  echo "writable copy drifted from the fixed blobs" >&2
  record source 2
fi
printf '%s\n' "overlay_absent" > /evidence/overlay-absent.txt
record source 0
/cargo-home/bin/sha256 file \
  tools/description-provider/src/lib.rs \
  tools/description-provider/src/transport.rs \
  tools/description-provider/tests/provider.rs \
  tools/description-provider/Cargo.lock \
  crates/oo-builder/src/description_candidate.rs \
  crates/oo-builder/src/bin/description-candidate.rs \
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

echo "=== cargo fetch root --locked --offline ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked --offline > /evidence/fetch-offline.log 2>&1
offline_fetch=$?
set -e
printf '%s\n' "$offline_fetch" > /evidence/phases/fetch-offline.exit
if [ "$offline_fetch" -ne 0 ]; then
  echo "=== cargo fetch root --locked ==="
  set +e
  cargo fetch --manifest-path Cargo.toml --locked > /evidence/fetch.log 2>&1
  fetch_exit=$?
  set -e
else
  cp /evidence/fetch-offline.log /evidence/fetch.log
  fetch_exit=0
fi
record fetch "$fetch_exit"

echo "=== description-candidate ==="
set +e
cargo build --manifest-path Cargo.toml --locked --offline -p oo-builder --bin description-candidate \
  > /evidence/build-cli.log 2>&1
build_exit=$?
set -e
record build-cli "$build_exit"
if [ ! -x /target/debug/description-candidate ]; then
  echo "description-candidate binary is missing" >&2
  record cli-bin 2
fi
record cli-bin 0

echo "=== cargo fetch provider --locked --offline ==="
export CARGO_TARGET_DIR=/target/provider
set +e
cargo fetch --manifest-path tools/description-provider/Cargo.toml --locked --offline \
  > /evidence/fetch-provider-offline.log 2>&1
provider_offline=$?
set -e
printf '%s\n' "$provider_offline" > /evidence/phases/fetch-provider-offline.exit
if [ "$provider_offline" -ne 0 ]; then
  echo "=== cargo fetch provider --locked ==="
  set +e
  cargo fetch --manifest-path tools/description-provider/Cargo.toml --locked \
    > /evidence/fetch-provider.log 2>&1
  provider_fetch=$?
  set -e
else
  cp /evidence/fetch-provider-offline.log /evidence/fetch-provider.log
  provider_fetch=0
fi
record fetch-provider "$provider_fetch"

echo "=== lock subset ==="
set +e
/cargo-home/bin/lock-subset Cargo.lock tools/description-provider/Cargo.lock \
  > /evidence/lock-subset.txt 2> /evidence/lock-subset.err
subset_exit=$?
set -e
record lock-subset "$subset_exit"
pins=$(grep -c 'version = "=' tools/description-provider/Cargo.toml || true)
{
  echo "lock_subset=$(cat /evidence/lock-subset.txt)"
  echo "direct_exact_version_pins=$pins"
  echo "scope=every registry name/version/source/checksum in the provider lock"
} > /evidence/registry-vs-direct.txt
cp tools/description-provider/Cargo.lock /tmp/tampered.lock
sed -i '0,/checksum = "[0-9a-f]\{64\}"/s//checksum = "0000000000000000000000000000000000000000000000000000000000000000"/' /tmp/tampered.lock
set +e
/cargo-home/bin/lock-subset Cargo.lock /tmp/tampered.lock \
  > /evidence/lock-subset-tamper.txt 2> /evidence/lock-subset-tamper.err
tamper_exit=$?
set -e
if [ "$tamper_exit" -eq 0 ]; then
  echo "tampered checksum was accepted" >&2
  record lock-subset-tamper 1
fi
record lock-subset-tamper 0
rm -f /tmp/tampered.lock

echo "=== description-provider 25 tests ==="
set +e
OO_A263_DESCRIPTION_CANDIDATE=/target/debug/description-candidate \
  cargo test --manifest-path tools/description-provider/Cargo.toml --locked --offline \
  -- --test-threads=1 > /evidence/test.log 2>&1
test_exit=$?
set -e
record test "$test_exit"

export CARGO_TARGET_DIR=/target
/cargo-home/bin/sha256 file Cargo.lock | awk '{print $1}' > /evidence/lock-after.sha256
/cargo-home/bin/sha256 file tools/description-provider/Cargo.lock | awk '{print $1}' > /evidence/provider-lock-after.sha256
before=$(/cargo-home/bin/sha256 file /blobs/Cargo.lock | awk '{print $1}')
provider_before=$(/cargo-home/bin/sha256 file /blobs/tools/description-provider/Cargo.lock | awk '{print $1}')
after=$(cat /evidence/lock-after.sha256)
provider_after=$(cat /evidence/provider-lock-after.sha256)
printf '%s\n' "$before" > /evidence/lock-archive.sha256
printf '%s\n' "$provider_before" > /evidence/provider-lock-archive.sha256
if [ "$before" != "$after" ] || [ "$provider_before" != "$provider_after" ]; then
  echo "lock changed root $before $after provider $provider_before $provider_after" > /evidence/lock-hash.txt
  record lock-hash 1
fi
echo "unchanged root=$after provider=$provider_after" > /evidence/lock-hash.txt
record lock-hash 0

printf '%s\n' 0 > /evidence/smoke.exit
echo "smoke=ok"
exit 0
