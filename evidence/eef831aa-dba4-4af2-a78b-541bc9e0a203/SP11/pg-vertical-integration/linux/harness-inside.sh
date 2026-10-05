#!/bin/bash
# Compile and run the a203 vertical harness against the fixed archive.
# Offline. No cargo update. Registry checksums already in the frozen lock must not change.
set -euo pipefail
echo "oo_task=sp11-pg-vertical-integration"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a203"
echo "oo_toolchain_request=1.94.0"
echo "phase=harness"
echo "fixed_base=0ee7cf86ad443ee1d84fe9fe3ed3f7d8847799c1"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target/harness
export CARGO_NET_OFFLINE=true
export CARGO_TERM_COLOR=never
export HOME=/tmp
rustc -Vv
cargo -V
got=$(rustc --version)
printf '%s\n' "$got"
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; exit 2 ;;
esac
if [ ! -f /src/Cargo.lock ] || [ ! -f /src/.fixed-base-sha ]; then
  echo "fixed archive is missing" >&2
  exit 2
fi
if [ -f /src/crates/oo-gateway/src/bin/query-pg.rs ]; then
  echo "archive contains the moving query CLI" >&2
  exit 2
fi
sha=$(cat /src/.fixed-base-sha)
if [ "$sha" != "0ee7cf86ad443ee1d84fe9fe3ed3f7d8847799c1" ]; then
  echo "archive sha mismatch" >&2
  exit 2
fi
printf '%s\n' "$sha" > /evidence/linux/harness-fixed-base.txt
mkdir -p /evidence/linux/harness-phases /evidence/live
rm -f /evidence/linux/harness-phases/*.exit

libdir=$(cat /evidence/linux/libclang-dir.txt)
multiarch=/cargo-home/a203-clang-root/usr/lib/aarch64-linux-gnu
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I/cargo-home/a203-clang-root/usr/lib/llvm-19/lib/clang/19/include -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/a203-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a203-clang-root/usr/share/pkgconfig"
if [ ! -e "$libdir/libclang.so" ] && [ ! -e "$libdir/libclang.so.1" ]; then
  echo "libclang is missing from the cargo volume" >&2
  exit 2
fi

record() {
  printf '%s\n' "$2" > "/evidence/linux/harness-phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
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

cp /src/Cargo.lock /evidence/Cargo.lock.before
lock_triples /evidence/Cargo.lock.before > /evidence/linux/lock-registry-before.txt
cp /src/Cargo.lock /evidence/Cargo.lock
printf '%s\n' standalone > /evidence/linux/resolve-mode.txt

set +e
cargo build --offline --manifest-path /evidence/Cargo.toml > /evidence/linux/harness-build.log 2>&1
build_exit=$?
set -e
record build "$build_exit"

if [ "$build_exit" -ne 0 ]; then
  if grep -q 'could not compile `oo-a203-pg-vertical`' /evidence/linux/harness-build.log; then
    echo "harness crate failed to compile; overlay would repeat the same source error" >&2
    exit "$build_exit"
  fi
  echo "standalone resolve failed; preserving the log and trying one private overlay" >&2
  mkdir -p /evidence/linux/harness-attempt-build
  cp /evidence/linux/harness-build.log /evidence/linux/harness-attempt-build/harness-build.log
  rm -rf /tmp/a203-overlay
  mkdir -p /tmp/a203-overlay/a203-harness
  cp -a /src/Cargo.toml /src/Cargo.lock /src/rust-toolchain.toml /src/crates /src/contracts /tmp/a203-overlay/
  cp -a /evidence/src /tmp/a203-overlay/a203-harness/src
  cat > /tmp/a203-overlay/a203-harness/Cargo.toml <<'EOF'
[package]
name = "oo-a203-pg-vertical"
edition.workspace = true
license.workspace = true
publish.workspace = true
rust-version.workspace = true
version.workspace = true

[dependencies]
oo-builder = { path = "../crates/oo-builder" }
oo-connectors = { path = "../crates/oo-connectors" }
oo-gateway = { path = "../crates/oo-gateway" }
oo-pipeline = { path = "../crates/oo-pipeline" }
serde_json = { version = "=1.0.151", features = ["arbitrary_precision"] }
sha2 = { version = "=0.10.9", default-features = false, features = ["std"] }
sqlx = { version = "=0.9.0", default-features = false, features = ["runtime-tokio", "postgres"] }
sqlx-core = { version = "=0.9.0", default-features = false, features = ["_rt-tokio", "_tls-rustls-ring-no-roots"] }
tokio = { version = "=1.53.1", default-features = false, features = ["macros", "rt-multi-thread", "time"] }
tokio-postgres = { version = "=0.7.18", default-features = false, features = ["runtime"] }
EOF
  sed -i 's/"crates\/xtask",/"crates\/xtask",\n    "a203-harness",/' /tmp/a203-overlay/Cargo.toml
  cp /tmp/a203-overlay/a203-harness/Cargo.toml /evidence/linux/overlay-member.toml
  cp /tmp/a203-overlay/Cargo.toml /evidence/linux/overlay-workspace.toml
  printf '%s\n' overlay > /evidence/linux/resolve-mode.txt
  set +e
  cargo build --offline --manifest-path /tmp/a203-overlay/Cargo.toml -p oo-a203-pg-vertical > /evidence/linux/harness-build-overlay.log 2>&1
  build_exit=$?
  set -e
  record build-overlay "$build_exit"
  if [ "$build_exit" -ne 0 ]; then
    exit "$build_exit"
  fi
  cp /tmp/a203-overlay/Cargo.lock /evidence/Cargo.lock
  manifest=/tmp/a203-overlay/Cargo.toml
  package_args=(-p oo-a203-pg-vertical)
else
  manifest=/evidence/Cargo.toml
  package_args=()
fi

lock_triples /evidence/Cargo.lock > /evidence/linux/lock-registry-after.txt
comm -13 /evidence/linux/lock-registry-before.txt /evidence/linux/lock-registry-after.txt > /evidence/linux/lock-registry-added.txt
comm -23 /evidence/linux/lock-registry-before.txt /evidence/linux/lock-registry-after.txt > /evidence/linux/lock-registry-removed.txt
if [ -s /evidence/linux/lock-registry-added.txt ]; then
  echo "frozen registry checksum set gained a package" >&2
  record lock-diff 4
  exit 4
fi
record lock-diff 0

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  echo "=== rustup component add clippy --toolchain 1.94.0 ==="
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/linux/harness-clippy-component.log 2>&1
  clippy_add=$?
  set -e
  record clippy-component "$clippy_add"
  if [ "$clippy_add" -ne 0 ]; then
    exit "$clippy_add"
  fi
else
  echo "clippy_component=already_installed"
  record clippy-component 0
fi

set +e
cargo clippy --offline --locked --manifest-path "$manifest" "${package_args[@]}" --all-targets -- -D warnings > /evidence/linux/harness-clippy.log 2>&1
clippy_exit=$?
set -e
record clippy "$clippy_exit"
if [ "$clippy_exit" -ne 0 ]; then
  exit "$clippy_exit"
fi

export OO_A203_HOST=oo-a203-pg
export OO_A203_PORT=5432
export OO_A203_ADMIN_USER=oo_fixture_admin
export OO_A203_ADMIN_PASSWORD_FILE=/run/secrets/admin.pass
export OO_A203_READER_PASSWORD_FILE=/run/secrets/reader.pass
export OO_A203_RUNTIME_PASSWORD_FILE=/run/secrets/runtime.pass
export OO_A203_PARTY_PASSWORD_FILE=/run/secrets/party.pass
export OO_A203_INVOICE_PASSWORD_FILE=/run/secrets/invoice.pass
export OO_A203_CA_FILE=/run/secrets/ca.crt
export OO_A203_EVIDENCE=/evidence

set +e
/target/harness/debug/oo-a203-pg-vertical > /evidence/linux/harness-run.log 2>&1
run_exit=$?
set -e
record run "$run_exit"

secret_hit=0
for pass in /run/secrets/admin.pass /run/secrets/reader.pass /run/secrets/runtime.pass /run/secrets/party.pass /run/secrets/invoice.pass; do
  if grep -R -l -F -f "$pass" /evidence/live /evidence/linux/harness-run.log >/dev/null 2>&1; then
    secret_hit=1
  fi
done
if [ "$secret_hit" -ne 0 ]; then
  echo "password material was written into evidence" >&2
  record secret-scan 5
  exit 5
fi
record secret-scan 0
exit "$run_exit"
