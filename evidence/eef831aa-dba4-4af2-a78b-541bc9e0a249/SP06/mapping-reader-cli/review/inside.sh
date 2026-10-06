#!/bin/bash
# Independent review proof. Re-runs the committed CLI test on the fixed archive.
# Does not edit product, manifests, or the lock. No cargo update.
set -u
echo "oo_task=sp06-mapping-reader-cli-review"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a249"
echo "oo_fixed=e0704257a1509b968a80a9ccd2f200319d5483ad"
echo "oo_product=9df074813572569950d85bbe951069b4db83facc"
echo "oo_platform=linux/arm64"
echo "phase=review"
uname -a
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
mkdir -p /evidence/review/phases /evidence/live/steps /cargo-home/bin
cd /src

record() {
  mkdir -p /evidence/review/phases
  printf '%s\n' "$2" > "/evidence/review/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/review/review.exit
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
printf '%s\n' "$got" | tee /evidence/review/rustc-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
sha=$(cat /src/.fixed-base-sha)
printf '%s\n' "$sha" > /evidence/review/fixed-base.txt
if [ "$sha" != "e0704257a1509b968a80a9ccd2f200319d5483ad" ]; then
  echo "archive sha mismatch" >&2
  record source 2
fi
if [ -e /src/crates/oo-gateway/src/bin/provision-mapping-reader.rs ]; then
  echo "fixed archive already contains the cli" >&2
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
printf '%s\n' "no" > /evidence/review/cargo-update-invoked.txt

rm -rf /overlay/review-tree
mkdir -p /overlay/review-tree
cp -a /src/. /overlay/review-tree/
mkdir -p /overlay/review-tree/crates/oo-gateway/src/bin /overlay/review-tree/crates/oo-gateway/tests /overlay/review-tree/contracts/gateway
cp /overlay/product/provision-mapping-reader.rs /overlay/review-tree/crates/oo-gateway/src/bin/provision-mapping-reader.rs
cp /overlay/product/provision_mapping_reader_cli.rs /overlay/review-tree/crates/oo-gateway/tests/provision_mapping_reader_cli.rs
cp /overlay/product/provision-mapping-reader-cli-v1.md /overlay/review-tree/contracts/gateway/provision-mapping-reader-cli-v1.md
if [ -f /overlay/review-tree/rust-toolchain.toml ]; then
  mv /overlay/review-tree/rust-toolchain.toml /overlay/review-tree/rust-toolchain.toml.aside
fi
cmp -s /src/crates/oo-gateway/src/lib.rs /overlay/review-tree/crates/oo-gateway/src/lib.rs \
  && cmp -s /src/crates/oo-gateway/src/mapping_reader_access.rs /overlay/review-tree/crates/oo-gateway/src/mapping_reader_access.rs \
  && cmp -s /src/crates/oo-gateway/src/policy.rs /overlay/review-tree/crates/oo-gateway/src/policy.rs \
  && cmp -s /src/Cargo.lock /overlay/review-tree/Cargo.lock \
  && cmp -s /src/crates/oo-gateway/Cargo.toml /overlay/review-tree/crates/oo-gateway/Cargo.toml
record overlay-unchanged $?

/cargo-home/bin/sha256 file \
  /overlay/review-tree/crates/oo-gateway/src/bin/provision-mapping-reader.rs \
  /overlay/review-tree/crates/oo-gateway/tests/provision_mapping_reader_cli.rs \
  /overlay/review-tree/contracts/gateway/provision-mapping-reader-cli-v1.md \
  > /evidence/review/overlay-sha256.txt
awk '{print $1}' /evidence/review/expected-sha256.txt > /tmp/expected-hex
awk '{print $1}' /evidence/review/overlay-sha256.txt > /tmp/overlay-hex
if ! cmp -s /tmp/expected-hex /tmp/overlay-hex; then
  echo "overlay bytes are not the committed product" >&2
  record product-bytes 1
fi
record product-bytes 0

lock_triples /src/Cargo.lock > /evidence/review/lock-registry-before.txt

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/review/clippy-component.log 2>&1
  clippy_component=$?
  set -e
  record clippy-component "$clippy_component"
else
  echo "clippy_component=present" > /evidence/review/clippy-component.log
  record clippy-component 0
fi

echo "=== build init-pg and oo-pipeline ==="
set +e
cargo build --offline --locked --manifest-path /overlay/review-tree/Cargo.toml -p oo-pipeline --bin init-pg --bin oo-pipeline \
  > /evidence/review/bins-build.log 2>&1
build_exit=$?
set -e
record bins "$build_exit"

echo "=== independent provision-mapping-reader cli test ==="
set +e
cargo test --offline --locked --manifest-path /overlay/review-tree/Cargo.toml -p oo-gateway --test provision_mapping_reader_cli -- --test-threads=1 --nocapture \
  > /evidence/review/test-live.log 2>&1
live_exit=$?
set -e
record test-live "$live_exit"

echo "=== clippy ==="
set +e
cargo clippy --offline --locked --manifest-path /overlay/review-tree/Cargo.toml -p oo-gateway --bin provision-mapping-reader --test provision_mapping_reader_cli -- -D warnings \
  > /evidence/review/clippy.log 2>&1
clippy_exit=$?
set -e
record clippy "$clippy_exit"

lock_triples /overlay/review-tree/Cargo.lock > /evidence/review/lock-registry-after.txt
if ! diff -q /evidence/review/lock-registry-before.txt /evidence/review/lock-registry-after.txt >/tmp/lock-diff; then
  record lock-diff 1
fi
printf '%s\n' "unchanged" > /evidence/review/lock-registry-diff.txt
record lock-diff 0

: > /tmp/a249-bans
for secret_file in /secrets/*.pass; do
  tr -d '\r\n' < "$secret_file" >> /tmp/a249-bans
  printf '\n' >> /tmp/a249-bans
done
set +e
grep -F -l -f /tmp/a249-bans \
  /evidence/live/oracle.txt \
  /evidence/live/outcome.json \
  /evidence/review/test-live.log \
  /evidence/live/steps/cli-installed.stdout \
  /evidence/live/steps/cli-replay.stdout \
  /evidence/live/steps/cli-stale.stderr \
  /evidence/live/steps/cli-unknown.stderr \
  /evidence/live/steps/cli-partial.stderr \
  > /evidence/review/secret-scan-hits.txt
scan_exit=$?
set -e
rm -f /tmp/a249-bans
if [ "$scan_exit" -eq 0 ]; then
  echo "a secret appeared in a review evidence file" >&2
  record secret-scan 1
fi
if [ "$scan_exit" -gt 1 ]; then
  echo "secret scan failed to read evidence" >&2
  record secret-scan 1
fi
printf '%s\n' "clean" > /evidence/review/secret-scan-hits.txt
record secret-scan 0

printf '%s\n' 0 > /evidence/review/review.exit
echo "mapping_reader_cli_review=ok"
exit 0
