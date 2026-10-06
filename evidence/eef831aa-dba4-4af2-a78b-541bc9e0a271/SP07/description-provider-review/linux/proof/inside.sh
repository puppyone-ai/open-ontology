#!/bin/bash
# Review proof of fixed 9951cda7. The archive stays read-only.
# The writable copy gains only provider_a271.rs. No PostgreSQL. No model.
set -u
echo "oo_task=sp07-description-provider-review-proof"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a271"
echo "oo_fixed=9951cda7f128e30f1f6ce02e409d542d20a9e4fb"
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
mkdir -p /evidence/phases /cargo-home/bin /cargo-home/work
rm -rf /cargo-home/work/src
mkdir -p /cargo-home/work/src
cp -a /src/. /cargo-home/work/src/
cd /cargo-home/work/src
if [ -f rust-toolchain.toml ]; then
  mv rust-toolchain.toml rust-toolchain.toml.aside
fi
printf '%s\n' "moved-aside" > /evidence/toolchain-aside.txt
cp /overlay/provider_a271.rs tools/description-provider/tests/provider_a271.rs

record() {
  mkdir -p /evidence/phases
  printf '%s\n' "$2" > "/evidence/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
}

abort() {
  record "$1" "$2"
  printf '%s\n' "$2" > /evidence/proof.exit
  exit "$2"
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/rustc-version.txt
rustc -Vv > /evidence/rustc-verbose.txt
cargo -V | tee /evidence/cargo-version.txt
rustup show > /evidence/rustup-show.txt
case "$got" in
  "rustc 1.94.0 "*) record toolchain 0 ;;
  *) echo "refusing toolchain: $got" >&2; abort toolchain 2 ;;
esac

rustc --edition 2021 -D warnings /tools/sha256.rs -o /cargo-home/bin/sha256
record sha256-tool $?
if [ "$(cat /evidence/phases/sha256-tool.exit)" -ne 0 ]; then
  printf '%s\n' 2 > /evidence/proof.exit
  exit 2
fi
rustc --edition 2021 -D warnings /tools/lock_subset.rs -o /cargo-home/bin/lock-subset
record lock-subset-tool $?
if [ "$(cat /evidence/phases/lock-subset-tool.exit)" -ne 0 ]; then
  printf '%s\n' 2 > /evidence/proof.exit
  exit 2
fi
export PATH="/cargo-home/bin:${PATH}"

if [ -e /src/tools/description-provider/tests/provider_a271.rs ]; then
  echo "fixed archive already contains the review test" >&2
  abort source 2
fi
if ! cmp -s tools/description-provider/tests/provider_a271.rs /overlay/provider_a271.rs; then
  echo "overlay copy drifted" >&2
  abort source 2
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
  echo "writable product bytes drifted from the fixed blobs" >&2
  abort source 2
fi
printf '%s\n' "overlay_copied" > /evidence/overlay-copied.txt
record source 0
/cargo-home/bin/sha256 file \
  tools/description-provider/src/lib.rs \
  tools/description-provider/tests/provider.rs \
  tools/description-provider/tests/provider_a271.rs \
  tools/description-provider/Cargo.lock \
  crates/oo-builder/src/description_candidate.rs \
  Cargo.lock \
  > /evidence/source-sha256.txt
record owned-hash $?
diff -rq /src /cargo-home/work/src | LC_ALL=C sort > /evidence/tree-diff.txt || true
cat > /evidence/tree-diff-expected.txt <<'EOF'
Only in /cargo-home/work/src/tools/description-provider/tests: provider_a271.rs
Only in /cargo-home/work/src: rust-toolchain.toml.aside
Only in /src: rust-toolchain.toml
EOF
if ! cmp -s /evidence/tree-diff.txt /evidence/tree-diff-expected.txt; then
  echo "proof tree diff is not the review test and toolchain-aside" >&2
  record tree 2
else
  record tree 0
fi
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
if [ "$fetch_exit" -ne 0 ]; then
  printf '%s\n' "$fetch_exit" > /evidence/proof.exit
  exit "$fetch_exit"
fi

echo "=== description-candidate ==="
set +e
cargo build --manifest-path Cargo.toml --locked --offline -p oo-builder --bin description-candidate \
  > /evidence/build-cli.log 2>&1
build_exit=$?
set -e
record build-cli "$build_exit"
if [ "$build_exit" -ne 0 ] || [ ! -x /target/debug/description-candidate ]; then
  printf '%s\n' 1 > /evidence/proof.exit
  exit 1
fi

export CARGO_TARGET_DIR=/target/provider
echo "=== cargo fetch provider --locked --offline ==="
set +e
cargo fetch --manifest-path tools/description-provider/Cargo.toml --locked --offline \
  > /evidence/fetch-provider-offline.log 2>&1
provider_offline=$?
set -e
printf '%s\n' "$provider_offline" > /evidence/phases/fetch-provider-offline.exit
if [ "$provider_offline" -ne 0 ]; then
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
if [ "$provider_fetch" -ne 0 ]; then
  printf '%s\n' "$provider_fetch" > /evidence/proof.exit
  exit "$provider_fetch"
fi

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

export OO_A263_DESCRIPTION_CANDIDATE=/target/debug/description-candidate
export OO_A271_DESCRIPTION_CANDIDATE=/target/debug/description-candidate

echo "=== provider lib ==="
set +e
cargo test --manifest-path tools/description-provider/Cargo.toml --locked --offline --lib \
  -- --test-threads=1 > /evidence/test-lib.log 2>&1
lib_exit=$?
set -e
record test-lib "$lib_exit"

echo "=== provider author tests ==="
set +e
cargo test --manifest-path tools/description-provider/Cargo.toml --locked --offline --test provider \
  -- --test-threads=1 > /evidence/test-provider.log 2>&1
author_exit=$?
set -e
record test-provider "$author_exit"

echo "=== provider_a271 ==="
set +e
cargo test --manifest-path tools/description-provider/Cargo.toml --locked --offline --test provider_a271 \
  -- --test-threads=1 > /evidence/test-a271.log 2>&1
a271_exit=$?
set -e
record test-a271 "$a271_exit"

echo "=== clippy component ==="
set +e
rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'
have_clippy=$?
set -e
if [ "$have_clippy" -ne 0 ]; then
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/clippy-component.log 2>&1
  component_exit=$?
  set -e
  record clippy-component "$component_exit"
else
  printf '%s\n' "already-installed" > /evidence/clippy-component.log
  record clippy-component 0
fi

echo "=== clippy -D warnings ==="
set +e
cargo clippy --manifest-path tools/description-provider/Cargo.toml --locked --offline --all-targets -- -D warnings \
  > /evidence/clippy.log 2>&1
clippy_exit=$?
set -e
record clippy "$clippy_exit"

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
else
  echo "unchanged root=$after provider=$provider_after" > /evidence/lock-hash.txt
  record lock-hash 0
fi

final=0
if [ "$subset_exit" -ne 0 ]; then
  final=$subset_exit
elif [ "$lib_exit" -ne 0 ]; then
  final=$lib_exit
elif [ "$author_exit" -ne 0 ]; then
  final=$author_exit
elif [ "$a271_exit" -ne 0 ]; then
  final=$a271_exit
elif [ "$clippy_exit" -ne 0 ]; then
  final=$clippy_exit
elif [ "$(cat /evidence/phases/tree.exit)" -ne 0 ]; then
  final=2
elif [ "$(cat /evidence/phases/lock-hash.exit)" -ne 0 ]; then
  final=1
elif [ "$(cat /evidence/phases/clippy-component.exit)" -ne 0 ]; then
  final="$(cat /evidence/phases/clippy-component.exit)"
fi
printf '%s\n' "$final" > /evidence/proof.exit
if [ "$final" -eq 0 ]; then
  echo "proof=ok"
else
  echo "proof=failed exit=$final"
fi
exit "$final"
