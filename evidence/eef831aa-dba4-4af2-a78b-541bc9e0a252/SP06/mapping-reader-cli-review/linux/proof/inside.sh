#!/bin/bash
# Live proof of the frozen provision-mapping-reader on a private TLS cluster.
# No overlay. No cargo update. The lock registry triples must not change.
set -u
echo "oo_task=sp06-mapping-reader-cli-review"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a252"
echo "oo_fixed=9df074813572569950d85bbe951069b4db83facc"
echo "oo_platform=linux/arm64"
echo "phase=proof"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1
export HOME=/tmp
export OO_A249_LIVE=1
export OO_A249_INIT_PG=/target/debug/init-pg
export OO_A249_PIPELINE=/target/debug/oo-pipeline
mkdir -p /evidence/linux/proof/phases /evidence/live/steps /cargo-home/bin
cd /src

record() {
  mkdir -p /evidence/linux/proof/phases
  printf '%s\n' "$2" > "/evidence/linux/proof/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/proof/proof.exit
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
printf '%s\n' "$got" | tee /evidence/linux/proof/rustc-version.txt
cargo -V | tee /evidence/linux/proof/cargo-version.txt
rustup show > /evidence/linux/proof/rustup-show.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
sha=$(cat /src/.fixed-base-sha)
printf '%s\n' "$sha" > /evidence/linux/proof/fixed-base.txt
if [ "$sha" != "9df074813572569950d85bbe951069b4db83facc" ]; then
  echo "archive sha mismatch" >&2
  record source 2
fi
if [ ! -f /src/crates/oo-gateway/src/bin/provision-mapping-reader.rs ]; then
  echo "fixed archive is missing the cli" >&2
  record source 2
fi
cmp -s /src/crates/oo-gateway/src/bin/provision-mapping-reader.rs /evidence/source/blobs/provision-mapping-reader.rs \
  && cmp -s /src/crates/oo-gateway/tests/provision_mapping_reader_cli.rs /evidence/source/blobs/provision_mapping_reader_cli.rs \
  && cmp -s /src/contracts/gateway/provision-mapping-reader-cli-v1.md /evidence/source/blobs/provision-mapping-reader-cli-v1.md \
  && cmp -s /src/crates/oo-gateway/src/lib.rs /evidence/source/blobs/lib.rs \
  && cmp -s /src/crates/oo-gateway/src/mapping_reader_access.rs /evidence/source/blobs/mapping_reader_access.rs \
  && cmp -s /src/crates/oo-gateway/src/policy.rs /evidence/source/blobs/policy.rs \
  && cmp -s /src/Cargo.lock /evidence/source/blobs/Cargo.lock
record source $?
printf '%s\n' "overlay_absent" > /evidence/linux/proof/overlay-absent.txt

if [ ! -x /cargo-home/bin/sha256 ]; then
  rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256
fi
export PATH="/cargo-home/bin:${PATH}"

libdir=$(cat /evidence/linux/smoke/libclang-path.txt)
multiarch=/cargo-home/a252-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a252-clang-root/usr/lib/llvm-19/lib/clang/19/include
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a252-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a252-clang-root/usr/share/pkgconfig"
if [ ! -e "$libdir/libclang.so" ] && [ ! -e "$libdir/libclang.so.1" ]; then
  echo "libclang is missing from the cargo volume" >&2
  record libclang 2
fi
record libclang 0
printf '%s\n' "no" > /evidence/linux/proof/cargo-update-invoked.txt

/cargo-home/bin/sha256 file \
  /src/crates/oo-gateway/src/bin/provision-mapping-reader.rs \
  /src/crates/oo-gateway/tests/provision_mapping_reader_cli.rs \
  /src/contracts/gateway/provision-mapping-reader-cli-v1.md \
  /src/Cargo.lock \
  > /evidence/linux/proof/product-sha256.txt
record product-hash $?

lock_triples /src/Cargo.lock > /evidence/linux/proof/lock-registry-before.txt

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/linux/proof/clippy-component.log 2>&1
  clippy_component=$?
  set -e
  record clippy-component "$clippy_component"
else
  echo "clippy_component=present" > /evidence/linux/proof/clippy-component.log
  record clippy-component 0
fi

export CARGO_NET_OFFLINE=true

echo "=== build init-pg and oo-pipeline ==="
set +e
cargo build --offline --locked --manifest-path /src/Cargo.toml -p oo-pipeline --bin init-pg --bin oo-pipeline \
  > /evidence/linux/proof/bins-build.log 2>&1
build_exit=$?
set -e
record bins "$build_exit"
if [ ! -x /target/debug/init-pg ] || [ ! -x /target/debug/oo-pipeline ]; then
  echo "init-pg or oo-pipeline was not built" >&2
  record bin-present 1
fi
/cargo-home/bin/sha256 file /target/debug/init-pg /target/debug/oo-pipeline > /evidence/linux/proof/bin-sha256.txt
record bin-hash $?

echo "=== provision-mapping-reader cli test ==="
set +e
cargo test --offline --locked --manifest-path /src/Cargo.toml -p oo-gateway --test provision_mapping_reader_cli -- --test-threads=1 --nocapture \
  > /evidence/linux/proof/test-live.log 2>&1
live_exit=$?
set -e
record test-live "$live_exit"
if ! grep -q 'test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out' /evidence/linux/proof/test-live.log; then
  echo "expected 14 cli tests" >&2
  record test-count 1
fi
if [ ! -s /evidence/live/oracle.txt ]; then
  echo "live oracle was not written" >&2
  record test-count 1
fi
record test-count 0

echo "=== clippy ==="
set +e
cargo clippy --offline --locked --manifest-path /src/Cargo.toml -p oo-gateway --bin provision-mapping-reader --test provision_mapping_reader_cli -- -D warnings \
  > /evidence/linux/proof/clippy.log 2>&1
clippy_exit=$?
set -e
record clippy "$clippy_exit"

lock_triples /src/Cargo.lock > /evidence/linux/proof/lock-registry-after.txt
if ! diff -q /evidence/linux/proof/lock-registry-before.txt /evidence/linux/proof/lock-registry-after.txt > /evidence/linux/proof/lock-registry-diff.txt; then
  record lock-diff 1
fi
printf '%s\n' "unchanged" > /evidence/linux/proof/lock-registry-diff.txt
record lock-diff 0

: > /tmp/a252-bans
for secret_file in /secrets/*.pass; do
  tr -d '\r\n' < "$secret_file" >> /tmp/a252-bans
  printf '\n' >> /tmp/a252-bans
done
set +e
grep -F -l -f /tmp/a252-bans \
  /evidence/live/oracle.txt \
  /evidence/live/outcome.json \
  /evidence/live/outcome.txt \
  /evidence/live/server.txt \
  /evidence/linux/proof/test-live.log \
  /evidence/live/steps/cli-installed.stdout \
  /evidence/live/steps/cli-installed.stderr \
  /evidence/live/steps/cli-replay.stdout \
  /evidence/live/steps/cli-peek-replay.stdout \
  /evidence/live/steps/cli-stale.stderr \
  /evidence/live/steps/cli-unknown.stderr \
  /evidence/live/steps/cli-partial.stderr \
  > /evidence/linux/proof/secret-scan-hits.txt
scan_exit=$?
set -e
rm -f /tmp/a252-bans
if [ "$scan_exit" -eq 0 ]; then
  echo "a secret appeared in an evidence file" >&2
  record secret-scan 1
fi
if [ "$scan_exit" -gt 1 ]; then
  echo "secret scan failed to read evidence" >&2
  record secret-scan 1
fi
if grep -F -l 'PRIVATE KEY' \
  /evidence/live/oracle.txt \
  /evidence/live/outcome.json \
  /evidence/live/server.txt \
  /evidence/linux/proof/test-live.log \
  /evidence/live/steps/*.stdout \
  /evidence/live/steps/*.stderr \
  > /evidence/linux/proof/pem-scan-hits.txt; then
  echo "a private key marker appeared in live evidence" >&2
  record secret-scan 1
fi
printf '%s\n' "clean" > /evidence/linux/proof/secret-scan-hits.txt
printf '%s\n' "clean" > /evidence/linux/proof/pem-scan-hits.txt
record secret-scan 0

printf '%s\n' 0 > /evidence/linux/proof/proof.exit
echo "mapping_reader_cli_review_proof=ok"
exit 0
