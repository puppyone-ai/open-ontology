#!/bin/bash
# Overlay the three owned product paths onto the fixed archive and run the live proof.
# Offline cargo. No cargo update. The lock registry triples must not change.
set -u
echo "oo_task=sp06-mapping-reader-cli"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a249"
echo "oo_fixed=e0704257a1509b968a80a9ccd2f200319d5483ad"
echo "oo_platform=linux/arm64"
echo "phase=chain"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1
export CARGO_NET_OFFLINE=true
export HOME=/tmp
export OO_A249_LIVE=1
export OO_A249_INIT_PG=/target/debug/init-pg
export OO_A249_PIPELINE=/target/debug/oo-pipeline
mkdir -p /evidence/linux/chain/phases /evidence/live/steps /cargo-home/bin
cd /src

record() {
  mkdir -p /evidence/linux/chain/phases
  printf '%s\n' "$2" > "/evidence/linux/chain/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/chain/chain.exit
    exit "$2"
  fi
}

lock_triples() {
  awk '
    $0 == "[[package]]" { if (name != "" && checksum != "") print name "\t" version "\t" checksum; name=""; version=""; checksum="" }
    $1 == "name" && $2 == "=" { name=$3; gsub(/"/, "", name) }
    $1 == "version" && $2 == "=" { version=$3; gsub(/"/, "", version) }
    $1 == "checksum" && $2 == "=" { checksum=$3; gsub(/"/, "", checksum) }
    END { if (name != "" && checksum != "") print name "\t" version "\t" checksum }
  ' "$1" | sort -u
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/linux/chain/rustc-version.txt
cargo -V | tee /evidence/linux/chain/cargo-version.txt
rustup show > /evidence/linux/chain/rustup-show.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
sha=$(cat /src/.fixed-base-sha)
printf '%s\n' "$sha" > /evidence/linux/chain/fixed-base.txt
if [ "$sha" != "e0704257a1509b968a80a9ccd2f200319d5483ad" ]; then
  echo "archive sha mismatch" >&2
  record source 2
fi
if [ -e /src/crates/oo-gateway/src/bin/provision-mapping-reader.rs ]; then
  echo "fixed archive already contains the cli" >&2
  record source 2
fi
if ! grep -q 'pub async fn provision_mapping_reader' /src/crates/oo-gateway/src/mapping_reader_access.rs; then
  echo "fixed archive is missing provision_mapping_reader" >&2
  record source 2
fi
record source 0

if [ ! -x /cargo-home/bin/sha256 ]; then
  rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256
fi
export PATH="/cargo-home/bin:${PATH}"

libdir=$(cat /evidence/linux/smoke/libclang-path.txt)
multiarch=/cargo-home/a249-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a249-clang-root/usr/lib/llvm-19/lib/clang/19/include
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a249-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a249-clang-root/usr/share/pkgconfig"
if [ ! -e "$libdir/libclang.so" ] && [ ! -e "$libdir/libclang.so.1" ]; then
  echo "libclang is missing from the cargo volume" >&2
  record libclang 2
fi
record libclang 0
printf '%s\n' "no" > /evidence/linux/chain/cargo-update-invoked.txt

rm -rf /overlay/tree
mkdir -p /overlay/tree
cp -a /src/. /overlay/tree/
mkdir -p /overlay/tree/crates/oo-gateway/src/bin /overlay/tree/crates/oo-gateway/tests /overlay/tree/contracts/gateway
cp /overlay/product/provision-mapping-reader.rs /overlay/tree/crates/oo-gateway/src/bin/provision-mapping-reader.rs
cp /overlay/product/provision_mapping_reader_cli.rs /overlay/tree/crates/oo-gateway/tests/provision_mapping_reader_cli.rs
cp /overlay/product/provision-mapping-reader-cli-v1.md /overlay/tree/contracts/gateway/provision-mapping-reader-cli-v1.md
if [ -f /overlay/tree/rust-toolchain.toml ]; then
  mv /overlay/tree/rust-toolchain.toml /overlay/tree/rust-toolchain.toml.aside
fi
cmp -s /overlay/product/provision-mapping-reader.rs /overlay/tree/crates/oo-gateway/src/bin/provision-mapping-reader.rs
record overlay $?
cmp -s /src/crates/oo-gateway/src/lib.rs /overlay/tree/crates/oo-gateway/src/lib.rs \
  && cmp -s /src/crates/oo-gateway/src/mapping_reader_access.rs /overlay/tree/crates/oo-gateway/src/mapping_reader_access.rs \
  && cmp -s /src/crates/oo-gateway/src/policy.rs /overlay/tree/crates/oo-gateway/src/policy.rs
record overlay-unchanged $?
if cmp -s /src/Cargo.toml /overlay/tree/Cargo.toml && cmp -s /src/Cargo.lock /overlay/tree/Cargo.lock \
  && cmp -s /src/crates/oo-gateway/Cargo.toml /overlay/tree/crates/oo-gateway/Cargo.toml; then
  printf '%s\n' "manifest_and_lock_unchanged" > /evidence/linux/chain/overlay-workspace.txt
else
  echo "overlay changed a manifest or Cargo.lock" >&2
  record overlay-lock 2
fi
record overlay-lock 0

lock_triples /src/Cargo.lock > /evidence/linux/chain/lock-registry-before.txt

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/linux/chain/clippy-component.log 2>&1
  clippy_component=$?
  set -e
  record clippy-component "$clippy_component"
else
  echo "clippy_component=present" > /evidence/linux/chain/clippy-component.log
  record clippy-component 0
fi

echo "=== build init-pg and oo-pipeline ==="
set +e
cargo build --offline --locked --manifest-path /overlay/tree/Cargo.toml -p oo-pipeline --bin init-pg --bin oo-pipeline \
  > /evidence/linux/chain/bins-build.log 2>&1
build_exit=$?
set -e
record bins "$build_exit"
test -x /target/debug/init-pg
test -x /target/debug/oo-pipeline
/cargo-home/bin/sha256 file /target/debug/init-pg /target/debug/oo-pipeline > /evidence/linux/chain/bin-sha256.txt
record bin-hash $?

echo "=== provision-mapping-reader cli test ==="
set +e
cargo test --offline --locked --manifest-path /overlay/tree/Cargo.toml -p oo-gateway --test provision_mapping_reader_cli -- --test-threads=1 --nocapture \
  > /evidence/linux/chain/test-live.log 2>&1
live_exit=$?
set -e
record test-live "$live_exit"

echo "=== clippy ==="
set +e
cargo clippy --offline --locked --manifest-path /overlay/tree/Cargo.toml -p oo-gateway --bin provision-mapping-reader --test provision_mapping_reader_cli -- -D warnings \
  > /evidence/linux/chain/clippy.log 2>&1
clippy_exit=$?
set -e
record clippy "$clippy_exit"

lock_triples /overlay/tree/Cargo.lock > /evidence/linux/chain/lock-registry-after.txt
if ! diff -q /evidence/linux/chain/lock-registry-before.txt /evidence/linux/chain/lock-registry-after.txt > /evidence/linux/chain/lock-registry-diff.txt; then
  record lock-diff 1
fi
printf '%s\n' "unchanged" > /evidence/linux/chain/lock-registry-diff.txt
record lock-diff 0
cmp -s /src/Cargo.lock /overlay/tree/Cargo.lock
record lock-bytes $?

: > /tmp/a249-bans
for secret_file in /secrets/*.pass; do
  tr -d '\r\n' < "$secret_file" >> /tmp/a249-bans
  printf '\n' >> /tmp/a249-bans
done
set +e
grep -F -l -f /tmp/a249-bans \
  /evidence/live/oracle.txt \
  /evidence/live/outcome.json \
  /evidence/live/outcome.txt \
  /evidence/linux/chain/test-live.log \
  /evidence/live/steps/cli-installed.stdout \
  /evidence/live/steps/cli-installed.stderr \
  /evidence/live/steps/cli-replay.stdout \
  /evidence/live/steps/cli-stale.stderr \
  /evidence/live/steps/cli-unknown.stderr \
  /evidence/live/steps/cli-partial.stderr \
  > /evidence/linux/chain/secret-scan-hits.txt
scan_exit=$?
set -e
rm -f /tmp/a249-bans
if [ "$scan_exit" -eq 0 ]; then
  echo "a secret appeared in an evidence file" >&2
  record secret-scan 1
fi
if [ "$scan_exit" -gt 1 ]; then
  echo "secret scan failed to read evidence" >&2
  record secret-scan 1
fi
printf '%s\n' "clean" > /evidence/linux/chain/secret-scan-hits.txt
record secret-scan 0

printf '%s\n' 0 > /evidence/linux/chain/chain.exit
echo "mapping_reader_cli_chain=ok"
exit 0
