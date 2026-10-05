#!/bin/bash
# Build the fixed-archive compilers, overlay only the harness, and run the chain.
# Offline. No cargo update. Registry name/version/checksum triples must not
# gain or lose a package. Standalone lock resolution is not retried: a211
# preserved that failure because a standalone workspace drops registry packages.
set -u
echo "oo_task=sp11-reviewed-pg-composition"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a226"
echo "oo_fixed=ed661809cf6c1e8df37af5842838dd1eb839c98a"
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
mkdir -p /evidence/linux/chain/phases /evidence/live/steps /cargo-home/a226-bins /tmp/a226-bundles
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
if [ "$sha" != "ed661809cf6c1e8df37af5842838dd1eb839c98a" ]; then
  echo "archive sha mismatch" >&2
  record source 2
fi
if [ ! -f crates/oo-builder/src/bin/reviewed-effective-oos.rs ] || [ -e a226-harness ]; then
  echo "fixed archive is not the pre-overlay tree" >&2
  record source 2
fi

libdir=$(cat /evidence/linux/smoke/libclang-path.txt)
multiarch=/cargo-home/a226-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a226-clang-root/usr/lib/llvm-19/lib/clang/19/include
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a226-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a226-clang-root/usr/share/pkgconfig"
if [ ! -e "$libdir/libclang.so" ] && [ ! -e "$libdir/libclang.so.1" ]; then
  echo "libclang is missing from the cargo volume" >&2
  record libclang 2
fi
record libclang 0
printf '%s\n' "no" > /evidence/linux/chain/cargo-update-invoked.txt

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/linux/chain/clippy-component.log 2>&1
  clippy_add=$?
  set -e
  record clippy-component "$clippy_add"
else
  echo "clippy_component=already_installed" > /evidence/linux/chain/clippy-component.log
  record clippy-component 0
fi

echo "=== focused tests on the unchanged archive ==="
set +e
cargo test --manifest-path /src/Cargo.toml --locked --offline -p oo-builder --lib reviewed_effective -- --test-threads=1 \
  > /evidence/linux/chain/test-builder-lib.log 2>&1
builder_lib_exit=$?
set -e
record test-builder-lib "$builder_lib_exit"
set +e
cargo test --manifest-path /src/Cargo.toml --locked --offline -p oo-builder \
  --test reviewed_effective --test role_pack --test role_pack_cli --test role_pack_bundle \
  --test captured_pg_cli --test review_pg_cli -- --test-threads=1 \
  > /evidence/linux/chain/test-builder.log 2>&1
builder_exit=$?
set -e
record test-builder "$builder_exit"
set +e
cargo test --manifest-path /src/Cargo.toml --locked --offline -p oo-pipeline \
  --test init_pg_cli --test catalog_pg_cli_live --test import_pg_cli_live -- --test-threads=1 \
  > /evidence/linux/chain/test-pipeline.log 2>&1
pipeline_exit=$?
set -e
record test-pipeline "$pipeline_exit"
set +e
cargo test --manifest-path /src/Cargo.toml --locked --offline -p oo-gateway --lib policy::tests -- --test-threads=1 \
  > /evidence/linux/chain/test-gateway-lib.log 2>&1
gateway_exit=$?
set -e
record test-gateway-lib "$gateway_exit"
set +e
cargo test --manifest-path /src/Cargo.toml --locked --offline -p oo-gateway \
  --test query_pg_cli -- --test-threads=1 \
  > /evidence/linux/chain/test-query.log 2>&1
query_exit=$?
set -e
record test-query "$query_exit"

echo "=== locked compiler binaries ==="
for spec in \
  oo-pipeline:oo-pipeline \
  oo-pipeline:init-pg \
  oo-builder:oo-builder \
  oo-builder:reviewed-effective-oos \
  oo-builder:role-pack-bundle \
  oo-gateway:query-pg
do
  pkg=${spec%%:*}
  bin=${spec##*:}
  set +e
  cargo build --manifest-path /src/Cargo.toml --locked --offline -p "$pkg" --bin "$bin" \
    > "/evidence/linux/chain/bins-build-${bin}.log" 2>&1
  bins_exit=$?
  set -e
  record "bins-${bin}" "$bins_exit"
done
set +e
cp /target/debug/oo-pipeline /target/debug/oo-builder /target/debug/query-pg \
  /target/debug/init-pg /target/debug/reviewed-effective-oos /target/debug/role-pack-bundle \
  /cargo-home/a226-bins/
copy_exit=$?
set -e
record bin-copy "$copy_exit"
set +e
/cargo-home/bin/sha256 file \
  /cargo-home/a226-bins/oo-pipeline \
  /cargo-home/a226-bins/init-pg \
  /cargo-home/a226-bins/oo-builder \
  /cargo-home/a226-bins/reviewed-effective-oos \
  /cargo-home/a226-bins/role-pack-bundle \
  /cargo-home/a226-bins/query-pg \
  > /evidence/linux/chain/bin-sha256.txt
hash_exit=$?
set -e
record bin-hash "$hash_exit"

lock_triples /src/Cargo.lock > /evidence/linux/chain/lock-registry-before.txt
mkdir -p /evidence/linux/chain/attempt-standalone
cat > /evidence/linux/chain/attempt-standalone/why.txt <<'EOF'
Standalone cargo resolution is not executed. a211 attempt-standalone showed that a
separate [workspace] rewrites the lock and drops registry packages. This harness is
an overlay member of the fixed archive workspace. The skipped log is that citation,
not a new failed build.
EOF
cp /evidence/linux/chain/attempt-standalone/why.txt /evidence/linux/chain/attempt-standalone-skipped.txt
printf '%s\n' overlay > /evidence/linux/chain/resolve-mode.txt

rm -rf /overlay/tree
mkdir -p /overlay/tree/a226-harness/src
cp -a /src/Cargo.toml /src/Cargo.lock /src/rust-toolchain.toml /src/crates /src/contracts /overlay/tree/
cp -a /evidence/src/. /overlay/tree/a226-harness/src/
cp /evidence/Cargo.toml /overlay/tree/a226-harness/Cargo.toml
sed -i 's/"crates\/xtask",/"crates\/xtask",\n    "a226-harness",/' /overlay/tree/Cargo.toml
if [ -f /overlay/tree/rust-toolchain.toml ]; then
  mv /overlay/tree/rust-toolchain.toml /overlay/tree/rust-toolchain.toml.aside
fi
if ! grep -q 'a226-harness' /overlay/tree/Cargo.toml; then
  echo "overlay member was not inserted" >&2
  record overlay-member 2
fi
cp /overlay/tree/a226-harness/Cargo.toml /evidence/linux/chain/overlay-member.toml
cp /overlay/tree/Cargo.toml /evidence/linux/chain/overlay-workspace.toml
export CARGO_TARGET_DIR=/target/harness
set +e
cargo build --offline --manifest-path /overlay/tree/Cargo.toml -p oo-a226-reviewed-pg-composition \
  > /evidence/linux/chain/harness-build-overlay.log 2>&1
build_exit=$?
set -e
cp /evidence/linux/chain/harness-build-overlay.log /evidence/linux/chain/harness-build.log
record build-overlay "$build_exit"
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
cargo clippy --offline --locked --manifest-path /overlay/tree/Cargo.toml \
  -p oo-a226-reviewed-pg-composition --all-targets -- -D warnings \
  > /evidence/linux/chain/harness-clippy.log 2>&1
clippy_exit=$?
set -e
record clippy-harness "$clippy_exit"

echo "=== clippy the compiler bins ==="
export CARGO_TARGET_DIR=/target
set +e
cargo clippy --manifest-path /src/Cargo.toml --locked --offline \
  -p oo-pipeline --bin oo-pipeline --bin init-pg -- -D warnings \
  > /evidence/linux/chain/clippy-pipeline.log 2>&1
clippy_pipeline=$?
set -e
record clippy-pipeline "$clippy_pipeline"
set +e
cargo clippy --manifest-path /src/Cargo.toml --locked --offline \
  -p oo-builder --bin oo-builder --bin reviewed-effective-oos --bin role-pack-bundle -- -D warnings \
  > /evidence/linux/chain/clippy-builder.log 2>&1
clippy_builder=$?
set -e
record clippy-builder "$clippy_builder"
set +e
cargo clippy --manifest-path /src/Cargo.toml --locked --offline \
  -p oo-gateway --bin query-pg -- -D warnings \
  > /evidence/linux/chain/clippy-query.log 2>&1
clippy_query=$?
set -e
record clippy-query "$clippy_query"

export OO_A226_HOST=oo-a226-pg
export OO_A226_PORT=5432
export OO_A226_ADMIN_USER=oo_fixture_admin
export OO_A226_ADMIN_PASSWORD_FILE=/secrets/admin.pass
export OO_A226_READER_PASSWORD_FILE=/secrets/reader.pass
export OO_A226_WRITER_PASSWORD_FILE=/secrets/writer.pass
export OO_A226_RUNTIME_PASSWORD_FILE=/secrets/runtime.pass
export OO_A226_LOGIN_PASSWORD_FILE=/secrets/login.pass
export OO_A226_CA_FILE=/secrets/ca.crt
export OO_A226_EVIDENCE=/evidence/live
export OO_A226_PIPELINE=/cargo-home/a226-bins/oo-pipeline
export OO_A226_BUILDER=/cargo-home/a226-bins/oo-builder
export OO_A226_QUERY=/cargo-home/a226-bins/query-pg
export OO_A226_INIT_PG=/cargo-home/a226-bins/init-pg
export OO_A226_REVIEWED_EFFECTIVE=/cargo-home/a226-bins/reviewed-effective-oos
export OO_A226_ROLE_PACK_BUNDLE=/cargo-home/a226-bins/role-pack-bundle
export OO_A226_FIXTURE=/evidence/fixture/source.sql
export OO_A226_BUNDLE_DIR=/tmp/a226-bundles

set +e
/target/harness/debug/oo-a226-reviewed-pg-composition > /evidence/linux/chain/harness-run.log 2>&1
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
echo "reviewed_pg_composition_container=ok"
exit 0
