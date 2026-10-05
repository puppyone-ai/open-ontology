#!/bin/bash
# Build the fixed-archive CLIs, overlay only the harness, and run the chain.
# Offline. No cargo update. Registry name/version/checksum triples must not
# gain or lose a package. a203 effective-oos synthesis is not used.
set -u
echo "oo_task=sp11-pg-cli-chain"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a211"
echo "oo_fixed=a661dcdf72ff91561ae6b7cb79916e582884d08a"
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
mkdir -p /evidence/linux/chain/phases /evidence/live/steps /cargo-home/a211-bins
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
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
sha=$(cat /src/.fixed-base-sha)
printf '%s\n' "$sha" > /evidence/linux/chain/fixed-base.txt
if [ "$sha" != "a661dcdf72ff91561ae6b7cb79916e582884d08a" ]; then
  echo "archive sha mismatch" >&2
  record source 2
fi
if [ ! -f crates/oo-gateway/src/bin/query-pg.rs ] || [ -e a211-harness ]; then
  echo "fixed archive is not the pre-overlay tree" >&2
  record source 2
fi

libdir=$(cat /evidence/linux/smoke/libclang-path.txt)
multiarch=/cargo-home/a211-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a211-clang-root/usr/lib/llvm-19/lib/clang/19/include
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a211-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a211-clang-root/usr/share/pkgconfig"
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
cargo test --manifest-path /src/Cargo.toml --locked --offline -p oo-builder \
  --test captured_pg_cli --test review_pg_cli --test role_pack_cli -- --test-threads=1 \
  > /evidence/linux/chain/test-builder.log 2>&1
builder_exit=$?
set -e
record test-builder "$builder_exit"
set +e
cargo test --manifest-path /src/Cargo.toml --locked --offline -p oo-pipeline \
  --test catalog_pg_cli_live --test import_pg_cli_live -- --test-threads=1 \
  > /evidence/linux/chain/test-pipeline.log 2>&1
pipeline_exit=$?
set -e
record test-pipeline "$pipeline_exit"
set +e
cargo test --manifest-path /src/Cargo.toml --locked --offline -p oo-gateway \
  --test query_pg_cli -- --test-threads=1 \
  > /evidence/linux/chain/test-query.log 2>&1
query_exit=$?
set -e
record test-query "$query_exit"

echo "=== locked CLI binaries ==="
for spec in oo-pipeline:oo-pipeline oo-builder:oo-builder oo-gateway:query-pg; do
  pkg=${spec%%:*}
  bin=${spec##*:}
  set +e
  cargo build --manifest-path /src/Cargo.toml --locked --offline -p "$pkg" --bin "$bin" \
    > "/evidence/linux/chain/bins-build-${bin}.log" 2>&1
  bins_exit=$?
  set -e
  record "bins-${bin}" "$bins_exit"
done
cp /target/debug/oo-pipeline /target/debug/oo-builder /target/debug/query-pg /cargo-home/a211-bins/
set +e
/cargo-home/bin/sha256 file \
  /cargo-home/a211-bins/oo-pipeline \
  /cargo-home/a211-bins/oo-builder \
  /cargo-home/a211-bins/query-pg \
  > /evidence/linux/chain/bin-sha256.txt
hash_exit=$?
set -e
record bin-hash "$hash_exit"

lock_triples /src/Cargo.lock > /evidence/linux/chain/lock-registry-before.txt
export CARGO_TARGET_DIR=/target/harness
printf '%s\n' standalone > /evidence/linux/chain/resolve-mode.txt
rm -rf /tmp/a211-standalone
mkdir -p /tmp/a211-standalone/src
cp /evidence/Cargo.toml /tmp/a211-standalone/Cargo.toml
cp /evidence/src/main.rs /tmp/a211-standalone/src/main.rs
cp /src/Cargo.lock /tmp/a211-standalone/Cargo.lock
set +e
cargo build --offline --manifest-path /tmp/a211-standalone/Cargo.toml \
  > /evidence/linux/chain/harness-build.log 2>&1
build_exit=$?
set -e
printf '%s\n' "$build_exit" > /evidence/linux/chain/phases/build.exit
echo "phase_exit=$build_exit phase=build"
need_overlay=0
if [ "$build_exit" -ne 0 ]; then
  if grep -q 'could not compile `oo-a211-pg-cli-chain`' /evidence/linux/chain/harness-build.log; then
    echo "harness crate failed to compile" >&2
    printf '%s\n' "$build_exit" > /evidence/linux/chain/chain.exit
    exit "$build_exit"
  fi
  echo "standalone resolve failed; preserving the log and trying one private overlay" >&2
  need_overlay=1
else
  lock_triples /tmp/a211-standalone/Cargo.lock > /tmp/a211-standalone-after.txt
  comm -13 /evidence/linux/chain/lock-registry-before.txt /tmp/a211-standalone-after.txt > /tmp/a211-standalone-added.txt
  comm -23 /evidence/linux/chain/lock-registry-before.txt /tmp/a211-standalone-after.txt > /tmp/a211-standalone-removed.txt
  if [ -s /tmp/a211-standalone-added.txt ] || [ -s /tmp/a211-standalone-removed.txt ]; then
    echo "standalone lock is not the frozen registry set; trying one private overlay" >&2
    need_overlay=1
  fi
fi
if [ "$need_overlay" -eq 1 ]; then
  mkdir -p /evidence/linux/chain/attempt-standalone
  cp /evidence/linux/chain/harness-build.log /evidence/linux/chain/attempt-standalone/harness-build.log
  if [ -f /tmp/a211-standalone/Cargo.lock ]; then
    cp /tmp/a211-standalone/Cargo.lock /evidence/linux/chain/attempt-standalone/Cargo.lock
  fi
  if [ -f /tmp/a211-standalone-removed.txt ]; then
    cp /tmp/a211-standalone-added.txt /tmp/a211-standalone-removed.txt /evidence/linux/chain/attempt-standalone/
  fi
  rm -rf /overlay/tree
  mkdir -p /overlay/tree/a211-harness/src
  cp -a /src/Cargo.toml /src/Cargo.lock /src/rust-toolchain.toml /src/crates /src/contracts /overlay/tree/
  cp -a /evidence/src/main.rs /overlay/tree/a211-harness/src/main.rs
  cat > /overlay/tree/a211-harness/Cargo.toml <<'EOF'
[package]
name = "oo-a211-pg-cli-chain"
edition.workspace = true
license.workspace = true
publish.workspace = true
rust-version.workspace = true
version.workspace = true

[lints]
workspace = true

[dependencies]
oo-gateway = { path = "../crates/oo-gateway" }
oo-pipeline = { path = "../crates/oo-pipeline" }
serde_json = { version = "=1.0.151", features = ["arbitrary_precision"] }
sha2 = { version = "=0.10.9", default-features = false, features = ["std"] }
sqlx = { version = "=0.9.0", default-features = false, features = ["runtime-tokio", "postgres"] }
sqlx-core = { version = "=0.9.0", default-features = false, features = ["_rt-tokio", "_tls-rustls-ring-no-roots"] }
tokio = { version = "=1.53.1", default-features = false, features = ["macros", "rt-multi-thread", "time"] }
tokio-postgres = { version = "=0.7.18", default-features = false, features = ["runtime"] }
EOF
  sed -i 's/"crates\/xtask",/"crates\/xtask",\n    "a211-harness",/' /overlay/tree/Cargo.toml
  cp /overlay/tree/a211-harness/Cargo.toml /evidence/linux/chain/overlay-member.toml
  cp /overlay/tree/Cargo.toml /evidence/linux/chain/overlay-workspace.toml
  printf '%s\n' overlay > /evidence/linux/chain/resolve-mode.txt
  set +e
  cargo build --offline --manifest-path /overlay/tree/Cargo.toml -p oo-a211-pg-cli-chain \
    > /evidence/linux/chain/harness-build-overlay.log 2>&1
  build_exit=$?
  set -e
  record build-overlay "$build_exit"
  cp /overlay/tree/Cargo.lock /evidence/linux/chain/Cargo.lock.overlay
  manifest=/overlay/tree/Cargo.toml
  package_args=(-p oo-a211-pg-cli-chain)
else
  manifest=/tmp/a211-standalone/Cargo.toml
  package_args=()
  cp /tmp/a211-standalone/Cargo.lock /evidence/linux/chain/Cargo.lock.overlay
fi

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
cargo clippy --offline --locked --manifest-path "$manifest" "${package_args[@]}" --all-targets -- -D warnings \
  > /evidence/linux/chain/harness-clippy.log 2>&1
clippy_exit=$?
set -e
record clippy-harness "$clippy_exit"

echo "=== clippy the three bins ==="
export CARGO_TARGET_DIR=/target
set +e
cargo clippy --manifest-path /src/Cargo.toml --locked --offline \
  -p oo-pipeline --bin oo-pipeline -- -D warnings \
  > /evidence/linux/chain/clippy-pipeline.log 2>&1
clippy_pipeline=$?
set -e
record clippy-pipeline "$clippy_pipeline"
set +e
cargo clippy --manifest-path /src/Cargo.toml --locked --offline \
  -p oo-builder --bin oo-builder -- -D warnings \
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

export OO_A211_HOST=oo-a211-pg
export OO_A211_PORT=5432
export OO_A211_ADMIN_USER=oo_fixture_admin
export OO_A211_ADMIN_PASSWORD_FILE=/secrets/admin.pass
export OO_A211_READER_PASSWORD_FILE=/secrets/reader.pass
export OO_A211_WRITER_PASSWORD_FILE=/secrets/writer.pass
export OO_A211_RUNTIME_PASSWORD_FILE=/secrets/runtime.pass
export OO_A211_LOGIN_PASSWORD_FILE=/secrets/login.pass
export OO_A211_CA_FILE=/secrets/ca.crt
export OO_A211_EVIDENCE=/evidence/live
export OO_A211_PIPELINE=/cargo-home/a211-bins/oo-pipeline
export OO_A211_BUILDER=/cargo-home/a211-bins/oo-builder
export OO_A211_QUERY=/cargo-home/a211-bins/query-pg

set +e
/target/harness/debug/oo-a211-pg-cli-chain > /evidence/linux/chain/harness-run.log 2>&1
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
echo "pg_cli_chain_container=ok"
exit 0
