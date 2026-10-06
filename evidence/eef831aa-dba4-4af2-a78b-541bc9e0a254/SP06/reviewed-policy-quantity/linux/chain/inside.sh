#!/bin/bash
# Build the quantity-armed reviewed-policy workspace plus the private harness,
# then run init, import, producer, reviewed-policy, install, analyze, and query.
# Offline. The host manifest and lock are not edited. Registry triples must not change.
set -u
echo "oo_task=sp06-reviewed-policy-quantity-chain"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a254"
echo "oo_fixed=c828e2781fc37eb62249fab54fa0b7fff86f0249"
echo "oo_platform=linux/arm64"
echo "phase=chain"
uname -a
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1
export CARGO_NET_OFFLINE=true
export HOME=/tmp
mkdir -p /evidence/linux/chain/phases /evidence/live/steps /cargo-home/a254-bins /tmp/a254-bundles

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
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
record toolchain 0

sha=$(cat /src/.fixed-base-sha)
if [ "$sha" != "c828e2781fc37eb62249fab54fa0b7fff86f0249" ]; then
  echo "archive sha mismatch" >&2
  record source 2
fi
record source 0

libdir=$(cat /evidence/linux/smoke/libclang-path.txt)
multiarch=/cargo-home/a254-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a254-clang-root/usr/lib/llvm-19/lib/clang/19/include
pkgroot=$(cat /evidence/linux/smoke/libclang-dir.txt)
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a254-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${pkgroot}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a254-clang-root/usr/share/pkgconfig"
if [ ! -e "$libdir/libclang.so" ] && [ ! -e "$libdir/libclang.so.1" ]; then
  echo "libclang is missing from the cargo volume" >&2
  record libclang 2
fi
record libclang 0
printf '%s\n' "no" > /evidence/linux/chain/cargo-update-invoked.txt

echo "=== role-pack-bundle from the fixed archive ==="
set +e
cargo build --manifest-path /src/Cargo.toml --locked --offline -p oo-builder --bin role-pack-bundle \
  > /evidence/linux/chain/bins-build-role-pack-bundle.log 2>&1
role_exit=$?
set -e
record bins-role-pack-bundle "$role_exit"
cp /target/debug/role-pack-bundle /cargo-home/a254-bins/role-pack-bundle
post_hash=$(awk '$2 ~ /reviewed-policy$/ {print $1}' /evidence/linux/postedit/bin-sha256.txt)
now_hash=$(/cargo-home/bin/sha256 file /cargo-home/a254-bins/reviewed-policy | awk '{print $1}')
printf 'postedit=%s chain=%s\n' "$post_hash" "$now_hash" > /evidence/linux/chain/reviewed-policy-bin.txt
if [ -z "$post_hash" ] || [ "$post_hash" != "$now_hash" ]; then
  echo "reviewed-policy bin is not the post-edit binary" >&2
  record bin-identity 2
fi
record bin-identity 0

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/linux/chain/clippy-component.log 2>&1
  clippy_component=$?
  set -e
  record clippy-component "$clippy_component"
else
  record clippy-component 0
fi

lock_triples /src/Cargo.lock > /evidence/linux/chain/lock-registry-before.txt
rm -rf /overlay/tree
mkdir -p /overlay/tree/a254-harness/src
cp -a /src/Cargo.toml /src/Cargo.lock /src/rust-toolchain.toml /src/crates /src/contracts /overlay/tree/
cp /evidence/overlay/reviewed_policy.rs /overlay/tree/crates/oo-gateway/src/reviewed_policy.rs
cp -a /evidence/src/. /overlay/tree/a254-harness/src/
cp /evidence/Cargo.toml /overlay/tree/a254-harness/Cargo.toml
if [ -f /overlay/tree/rust-toolchain.toml ]; then
  mv /overlay/tree/rust-toolchain.toml /overlay/tree/rust-toolchain.toml.aside
fi
sed -i 's/"crates\/xtask",/"crates\/xtask",\n    "a254-harness",/' /overlay/tree/Cargo.toml
if ! grep -q 'a254-harness' /overlay/tree/Cargo.toml; then
  echo "overlay member was not inserted" >&2
  record overlay-member 2
fi
awk '
  $0 ~ /"int2" \| "int4" \| "int8" => matches!/ {on=1}
  on {print}
  on && $0 ~ /"text" =>/ {exit}
' /overlay/tree/crates/oo-gateway/src/reviewed_policy.rs > /evidence/linux/chain/integer-arm.txt
if ! grep -q 'SemanticType::Quantity' /evidence/linux/chain/integer-arm.txt; then
  echo "chain overlay does not accept integer quantity" >&2
  record overlay-member 2
fi
cp /overlay/tree/a254-harness/Cargo.toml /evidence/linux/chain/overlay-member.toml
cp /overlay/tree/Cargo.toml /evidence/linux/chain/overlay-workspace.toml
record overlay-member 0

set +e
cargo build --offline --manifest-path /overlay/tree/Cargo.toml -p oo-a254-pg-operator-composition \
  > /evidence/linux/chain/harness-build.log 2>&1
build_exit=$?
set -e
record build-harness "$build_exit"
cp /overlay/tree/Cargo.lock /evidence/linux/chain/Cargo.lock.overlay
lock_triples /evidence/linux/chain/Cargo.lock.overlay > /evidence/linux/chain/lock-registry-after.txt
comm -13 /evidence/linux/chain/lock-registry-before.txt /evidence/linux/chain/lock-registry-after.txt \
  > /evidence/linux/chain/lock-registry-added.txt
comm -23 /evidence/linux/chain/lock-registry-before.txt /evidence/linux/chain/lock-registry-after.txt \
  > /evidence/linux/chain/lock-registry-removed.txt
if [ -s /evidence/linux/chain/lock-registry-added.txt ] || [ -s /evidence/linux/chain/lock-registry-removed.txt ]; then
  echo "frozen registry checksum set changed" >&2
  record lock-diff 4
fi
record lock-diff 0

set +e
cargo clippy --offline --manifest-path /overlay/tree/Cargo.toml \
  -p oo-a254-pg-operator-composition --all-targets -- -D warnings \
  > /evidence/linux/chain/harness-clippy.log 2>&1
clippy_exit=$?
set -e
record clippy-harness "$clippy_exit"

export OO_A254_HOST=oo-a254-pg
export OO_A254_PORT=5432
export OO_A254_ADMIN_USER=oo_fixture_admin
export OO_A254_ADMIN_PASSWORD_FILE=/secrets/admin.pass
export OO_A254_READER_PASSWORD_FILE=/secrets/reader.pass
export OO_A254_WRITER_PASSWORD_FILE=/secrets/writer.pass
export OO_A254_RUNTIME_PASSWORD_FILE=/secrets/runtime.pass
export OO_A254_LOGIN_PASSWORD_FILE=/secrets/login.pass
export OO_A254_CA_FILE=/secrets/ca.crt
export OO_A254_EVIDENCE=/evidence/live
export OO_A254_PIPELINE=/cargo-home/a254-bins/oo-pipeline
export OO_A254_BUILDER=/cargo-home/a254-bins/oo-builder
export OO_A254_QUERY=/cargo-home/a254-bins/query-pg
export OO_A254_INIT_PG=/cargo-home/a254-bins/init-pg
export OO_A254_REVIEWED_EFFECTIVE=/cargo-home/a254-bins/reviewed-effective-oos
export OO_A254_ROLE_PACK_BUNDLE=/cargo-home/a254-bins/role-pack-bundle
export OO_A254_REVIEWED_POLICY=/cargo-home/a254-bins/reviewed-policy
export OO_A254_INSTALL_COLUMN_ACL=/cargo-home/a254-bins/install-column-acl
export OO_A254_ANALYZE_PG=/cargo-home/a254-bins/analyze-pg
export OO_A254_FIXTURE=/evidence/fixture/source.sql
export OO_A254_BUNDLE_DIR=/tmp/a254-bundles

set +e
/target/debug/oo-a254-pg-operator-composition > /evidence/linux/chain/harness-run.log 2>&1
run_exit=$?
set -e
record run "$run_exit"

secret_hit=0
for pass in /secrets/admin.pass /secrets/reader.pass /secrets/writer.pass /secrets/runtime.pass /secrets/login.pass; do
  if grep -R -l -F -f "$pass" /evidence/live /evidence/linux/chain/harness-run.log >/dev/null 2>&1; then
    secret_hit=1
  fi
done
if [ "$secret_hit" -ne 0 ]; then
  echo "password material was written into evidence" >&2
  record secret-scan 5
fi
record secret-scan 0
printf '%s\n' 0 > /evidence/linux/chain/chain.exit
echo "quantity_chain=ok"
exit 0
