#!/bin/bash
# Frozen e447249 smoke, then the review harness. The product tree is not edited.
# Linux arm64 Rust 1.94. No cargo update. No apt. One private PostgreSQL is already up.
set -u
echo "oo_task=sp06-reviewed-quantity-review"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a259"
echo "oo_fixed=e447249ab1a7f4c82df4729090b1285c9ef4a4ec"
echo "oo_platform=linux/arm64"
uname -a
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
mkdir -p /evidence/linux/run/phases /cargo-home/bin /cargo-home/a259-bins
cd /src

record() {
  mkdir -p /evidence/linux/run/phases
  printf '%s\n' "$2" > "/evidence/linux/run/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/run/run.exit
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
printf '%s\n' "$got" | tee /evidence/linux/run/rustc-version.txt
rustc -Vv > /evidence/linux/run/rustc-verbose.txt
cargo -V | tee /evidence/linux/run/cargo-version.txt
rustup show > /evidence/linux/run/rustup-show.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
record toolchain 0

rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256
record sha256-tool $?
printf 'abc' > /cargo-home/sha256-abc.txt
/cargo-home/bin/sha256 file /cargo-home/sha256-abc.txt > /evidence/linux/run/sha256-self.txt
record sha256-self $?
if ! grep -q 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad' /evidence/linux/run/sha256-self.txt; then
  echo "sha256 abc vector mismatch" >&2
  record sha256-vector 1
fi
record sha256-vector 0
rustc --edition 2021 /tools/select-debs.rs -o /cargo-home/bin/select-debs
record select-debs-tool $?
export PATH="/cargo-home/bin:${PATH}"

sha=$(cat /src/.fixed-base-sha)
printf '%s\n' "$sha" > /evidence/linux/run/fixed-base.txt
if [ "$sha" != "e447249ab1a7f4c82df4729090b1285c9ef4a4ec" ]; then
  echo "archive sha mismatch" >&2
  record source 2
fi
if [ ! -f crates/oo-gateway/tests/reviewed_policy_quantity.rs ] \
  || [ ! -f contracts/gateway/reviewed-policy-quantity-v1.md ]; then
  echo "frozen quantity files are absent" >&2
  record source 2
fi
awk '
  $0 ~ /"int2" \| "int4" \| "int8" => matches!/ {on=1}
  on {print}
  on && $0 ~ /"text" =>/ {exit}
' crates/oo-gateway/src/reviewed_policy.rs > /evidence/linux/run/integer-arm.txt
if ! grep -q 'SemanticType::Quantity' /evidence/linux/run/integer-arm.txt; then
  echo "frozen integer arm does not admit quantity" >&2
  record source 2
fi
if grep -q 'SemanticType::Money' /evidence/linux/run/integer-arm.txt; then
  echo "frozen integer arm admits money" >&2
  record source 2
fi
record source 0
printf '%s\n' "no" > /evidence/linux/run/cargo-update-invoked.txt

if [ ! -s /evidence/linux/run/libclang-dir.txt ] || [ ! -d /cargo-home/a259-clang-root/usr/lib ]; then
  set +e
  bash /extract-libclang.sh > /evidence/linux/run/libclang-extract.log 2>&1
  extract_exit=$?
  set -e
  record libclang "$extract_exit"
else
  echo "libclang_root=reused" > /evidence/linux/run/libclang-extract.log
  record libclang 0
fi
libdir=$(cat /evidence/linux/run/libclang-dir.txt)
multiarch=/cargo-home/a259-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a259-clang-root/usr/lib/llvm-19/lib/clang/19/include
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a259-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a259-clang-root/usr/share/pkgconfig"
real=$(find /cargo-home/a259-clang-root/usr/lib -name 'libclang-19.so.19' -print -quit)
devso=$(find /cargo-home/a259-clang-root/usr/lib -name 'libclang.so' -print -quit)
if [ -n "$devso" ]; then
  export LIBCLANG_PATH="$(dirname "$devso")"
fi
export LD_LIBRARY_PATH="${LIBCLANG_PATH}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
printf '%s\n' "$LIBCLANG_PATH" > /evidence/linux/run/libclang-path.txt
if [ -z "$real" ]; then
  echo "libclang-19.so.19 missing" >&2
  record libclang-load 1
fi
/lib/ld-linux-aarch64.so.1 --list "$real" > /evidence/linux/run/libclang-ldd.txt 2>&1 || true
if grep -Eq 'not found|error while loading' /evidence/linux/run/libclang-ldd.txt; then
  record libclang-load 1
fi
record libclang-load 0

echo "=== cargo fetch --locked ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked > /evidence/linux/run/fetch.log 2>&1
fetch_exit=$?
set -e
record fetch "$fetch_exit"
export CARGO_NET_OFFLINE=true

echo "=== frozen bins ==="
for spec in \
  oo-builder:oo-builder \
  oo-builder:reviewed-effective-oos \
  oo-gateway:reviewed-policy \
  oo-gateway:install-column-acl \
  oo-gateway:provision-mapping-reader \
  oo-gateway:query-pg \
  oo-pipeline:oo-pipeline \
  oo-pipeline:init-pg \
  oo-pipeline:analyze-pg
do
  pkg=${spec%%:*}
  bin=${spec##*:}
  set +e
  cargo build --manifest-path Cargo.toml --locked --offline -p "$pkg" --bin "$bin" \
    > "/evidence/linux/run/bins-build-${bin}.log" 2>&1
  bins_exit=$?
  set -e
  record "bins-${bin}" "$bins_exit"
  cp "/target/debug/${bin}" /cargo-home/a259-bins/
done
record bin-copy 0

export OO_A254_OO_BUILDER=/cargo-home/a259-bins/oo-builder
export OO_A254_REVIEWED_EFFECTIVE=/cargo-home/a259-bins/reviewed-effective-oos
export OO_A254_REVIEWED_POLICY=/cargo-home/a259-bins/reviewed-policy
export OO_A234_OO_BUILDER=/cargo-home/a259-bins/oo-builder
export OO_A234_REVIEWED_EFFECTIVE=/cargo-home/a259-bins/reviewed-effective-oos
export OO_A234_REVIEWED_POLICY=/cargo-home/a259-bins/reviewed-policy
export OO_A227_OO_BUILDER=/cargo-home/a259-bins/oo-builder
export OO_A227_REVIEWED_EFFECTIVE=/cargo-home/a259-bins/reviewed-effective-oos
export OO_A259_OO_BUILDER=/cargo-home/a259-bins/oo-builder
export OO_A259_REVIEWED_EFFECTIVE=/cargo-home/a259-bins/reviewed-effective-oos
export OO_A259_REVIEWED_POLICY=/cargo-home/a259-bins/reviewed-policy
export OO_A259_PIPELINE=/cargo-home/a259-bins/oo-pipeline
export OO_A259_INIT_PG=/cargo-home/a259-bins/init-pg
export OO_A259_INSTALL_COLUMN_ACL=/cargo-home/a259-bins/install-column-acl
export OO_A259_PROVISION=/cargo-home/a259-bins/provision-mapping-reader
export OO_A259_ANALYZE=/cargo-home/a259-bins/analyze-pg
export OO_A259_QUERY=/cargo-home/a259-bins/query-pg
export OO_A259_EVIDENCE=/evidence
export OO_A259_FIXTURE=/evidence/fixture/source.sql
export OO_A259_HOST=oo-a259-pg
export OO_A259_PORT=5432
export OO_A259_CA_FILE=/secrets/ca.crt
export OO_A259_ADMIN_PASSWORD_FILE=/secrets/admin.pass
export OO_A259_READER_PASSWORD_FILE=/secrets/reader.pass
export OO_A259_RUNTIME_PASSWORD_FILE=/secrets/runtime.pass
export OO_A259_LOGIN_PASSWORD_FILE=/secrets/login.pass

echo "=== frozen focused tests before overlay ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --test reviewed_policy -- --test-threads=1 \
  > /evidence/linux/run/test-reviewed-policy.log 2>&1
test_exit=$?
set -e
record test-reviewed-policy "$test_exit"
if ! grep -q 'test result: ok. 21 passed' /evidence/linux/run/test-reviewed-policy.log; then
  record test-count 1
fi
record test-count 0

set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --test reviewed_policy_cli -- --test-threads=1 \
  > /evidence/linux/run/test-reviewed-policy-cli.log 2>&1
cli_exit=$?
set -e
record test-reviewed-policy-cli "$cli_exit"
if ! grep -q 'test result: ok. 6 passed' /evidence/linux/run/test-reviewed-policy-cli.log; then
  record cli-count 1
fi
record cli-count 0

set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --test reviewed_policy_quantity -- --test-threads=1 \
  > /evidence/linux/run/test-reviewed-policy-quantity.log 2>&1
qty_exit=$?
set -e
record test-reviewed-policy-quantity "$qty_exit"
if ! grep -q 'test result: ok. 6 passed' /evidence/linux/run/test-reviewed-policy-quantity.log; then
  record quantity-count 1
fi
record quantity-count 0

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/linux/run/clippy-component.log 2>&1
  clippy_component=$?
  set -e
  record clippy-component "$clippy_component"
else
  record clippy-component 0
fi

set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib --bin reviewed-policy --test reviewed_policy_quantity -- -D warnings \
  > /evidence/linux/run/clippy-component-product.log 2>&1
clippy_exit=$?
set -e
record clippy-product "$clippy_exit"

/cargo-home/bin/sha256 file Cargo.lock | awk '{print $1}' > /evidence/linux/run/lock-after.sha256
before=$(/cargo-home/bin/sha256 file /evidence/source/blobs/Cargo.lock | awk '{print $1}')
after=$(cat /evidence/linux/run/lock-after.sha256)
if [ "$before" != "$after" ]; then
  echo "lock changed before=$before after=$after" > /evidence/linux/run/lock-hash.txt
  record lock-hash 1
fi
echo "unchanged $after" > /evidence/linux/run/lock-hash.txt
record lock-hash 0
printf '%s\n' 0 > /evidence/linux/run/smoke.exit
echo "frozen_smoke=ok"

echo "=== harness overlay after smoke ==="
lock_triples /src/Cargo.lock > /evidence/linux/run/lock-registry-before.txt
rm -rf /overlay/tree
mkdir -p /overlay/tree/a259-harness/src
cp -a /src/Cargo.toml /src/Cargo.lock /src/crates /overlay/tree/
cp -a /evidence/harness/src/. /overlay/tree/a259-harness/src/
cp /evidence/harness/Cargo.toml /overlay/tree/a259-harness/Cargo.toml
if [ -f /src/rust-toolchain.toml ]; then
  cp /src/rust-toolchain.toml /overlay/tree/rust-toolchain.toml.aside
fi
cmp -s /src/crates/oo-gateway/src/reviewed_policy.rs /overlay/tree/crates/oo-gateway/src/reviewed_policy.rs
record overlay-source $?
sed -i 's/"crates\/xtask",/"crates\/xtask",\n    "a259-harness",/' /overlay/tree/Cargo.toml
if ! grep -q 'a259-harness' /overlay/tree/Cargo.toml; then
  echo "overlay member was not inserted" >&2
  record overlay-member 2
fi
cmp -s /src/crates/oo-gateway/src/reviewed_policy.rs /overlay/tree/crates/oo-gateway/src/reviewed_policy.rs
record overlay-unmodified $?
cp /overlay/tree/a259-harness/Cargo.toml /evidence/linux/run/overlay-member.toml
cp /overlay/tree/Cargo.toml /evidence/linux/run/overlay-workspace.toml
record overlay-member 0

set +e
cargo build --offline --manifest-path /overlay/tree/Cargo.toml -p oo-a259-quantity-review \
  > /evidence/linux/run/harness-build.log 2>&1
build_exit=$?
set -e
record build-harness "$build_exit"
cp /overlay/tree/Cargo.lock /evidence/linux/run/Cargo.lock.overlay
lock_triples /evidence/linux/run/Cargo.lock.overlay > /evidence/linux/run/lock-registry-after.txt
comm -13 /evidence/linux/run/lock-registry-before.txt /evidence/linux/run/lock-registry-after.txt \
  > /evidence/linux/run/lock-registry-added.txt
comm -23 /evidence/linux/run/lock-registry-before.txt /evidence/linux/run/lock-registry-after.txt \
  > /evidence/linux/run/lock-registry-removed.txt
if [ -s /evidence/linux/run/lock-registry-added.txt ] || [ -s /evidence/linux/run/lock-registry-removed.txt ]; then
  echo "frozen registry checksum set changed" >&2
  record lock-diff 4
fi
record lock-diff 0

set +e
cargo clippy --offline --manifest-path /overlay/tree/Cargo.toml \
  -p oo-a259-quantity-review --all-targets -- -D warnings \
  > /evidence/linux/run/harness-clippy.log 2>&1
harness_clippy=$?
set -e
record clippy-harness "$harness_clippy"

export OO_A259_BOUND=1
set +e
/target/debug/oo-a259-quantity-review > /evidence/linux/run/harness-run.log 2>&1
harness_exit=$?
set -e
record harness "$harness_exit"
if ! grep -q 'classify=ok' /evidence/linux/run/harness-run.log; then
  record harness-classify 1
fi
record harness-classify 0
if ! grep -q 'bound=ok' /evidence/linux/run/harness-run.log; then
  record harness-bound 1
fi
record harness-bound 0

if grep -R -n -E '-----BEGIN |PRIVATE KEY|postgres://|password=|api_key|api-key' /evidence \
  --exclude-dir linux >/tmp/secret-scan.txt 2>/dev/null; then
  echo "secret marker in evidence" >&2
  record secret-scan 1
fi
printf '%s\n' clean > /evidence/linux/run/secret-scan.txt
record secret-scan 0

printf '%s\n' 0 > /evidence/linux/run/run.exit
echo "review_run=ok"
exit 0
