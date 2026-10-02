#!/bin/bash
# Baseline focused smoke for the immutable P5 spike. Each phase writes its own exit.
# apt-get update is not this script; that exit is already baseline-apt-update.exit.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export HOME=/tmp/oo-home
mkdir -p "$HOME" /opt/clangroot /tmp/oo-work
echo "phase_start=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo "id=$(id)"
echo "RUSTUP_TOOLCHAIN=${RUSTUP_TOOLCHAIN}"
rustc -Vv
cargo -V
echo "protoc=$(command -v protoc || echo absent)"

echo "--- extract pinned debs with dpkg-deb ---"
set +e
for deb in /debs/*.deb; do
  echo "extract $deb"
  dpkg-deb -x "$deb" /opt/clangroot
  rc=$?
  echo "dpkg_deb_exit=$rc file=$deb"
  if [ "$rc" -ne 0 ]; then
    echo "$rc" > /out/baseline-clang-extract.exit
    echo "extract_failed" > /out/baseline-phases.done
    exit "$rc"
  fi
done
echo 0 > /out/baseline-clang-extract.exit
ln -sfn clang-19 /opt/clangroot/usr/bin/clang
export PATH="/opt/clangroot/usr/bin:/opt/clangroot/usr/lib/llvm-19/bin:${PATH}"
export LD_LIBRARY_PATH="/opt/clangroot/usr/lib/aarch64-linux-gnu${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export LIBCLANG_PATH="/opt/clangroot/usr/lib/aarch64-linux-gnu"
export PKG_CONFIG_PATH="/opt/clangroot/usr/lib/aarch64-linux-gnu/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
clang --version
echo "--- ldd libclang ---"
so=$(find /opt/clangroot -name 'libclang-19.so.19' | head -1)
echo "so=$so"
ldd "$so" > /out/baseline-ldd.txt 2>&1 || true
if grep -q 'not found' /out/baseline-ldd.txt; then
  echo 1 > /out/baseline-ldd.exit
  echo "ldd_missing" > /out/baseline-phases.done
  exit 1
fi
echo 0 > /out/baseline-ldd.exit
cat /out/baseline-ldd.txt

echo "--- source hashes before cargo ---"
find /src -type f | sort | xargs sha256sum > /out/baseline-src-before.sha256
sha256sum /src/Cargo.toml /src/Cargo.lock > /out/baseline-manifest-before.sha256

if rustup component list --installed | grep -q '^clippy'; then
  echo "clippy_present_before=yes"
  echo 0 > /out/baseline-clippy-add.exit
else
  echo "clippy_present_before=no"
  rustup component add clippy > /out/baseline-clippy-add.log 2>&1
  echo $? > /out/baseline-clippy-add.exit
fi
echo "clippy_add_exit=$(cat /out/baseline-clippy-add.exit)"

echo "--- cargo fetch --locked (no -p) ---"
cargo fetch --manifest-path /src/Cargo.toml --locked > /out/baseline-fetch.log 2>&1
echo $? > /out/baseline-fetch.exit
echo "fetch_exit=$(cat /out/baseline-fetch.exit)"
if [ "$(cat /out/baseline-fetch.exit)" -ne 0 ]; then
  echo "fetch_failed" > /out/baseline-phases.done
  exit 1
fi

echo "--- cargo test default ---"
cargo test --manifest-path /src/Cargo.toml --locked --offline -- --test-threads=8 > /out/baseline-test-default.log 2>&1
echo $? > /out/baseline-test-default.exit
echo "test_default_exit=$(cat /out/baseline-test-default.exit)"

echo "--- cargo test test-hooks ---"
cargo test --manifest-path /src/Cargo.toml --locked --offline --features test-hooks -- --test-threads=8 > /out/baseline-test-hooks.log 2>&1
echo $? > /out/baseline-test-hooks.exit
echo "test_hooks_exit=$(cat /out/baseline-test-hooks.exit)"

echo "--- clippy default ---"
cargo clippy --manifest-path /src/Cargo.toml --all-targets --locked --offline -- -D warnings > /out/baseline-clippy-default.log 2>&1
echo $? > /out/baseline-clippy-default.exit
echo "clippy_default_exit=$(cat /out/baseline-clippy-default.exit)"

echo "--- clippy test-hooks ---"
cargo clippy --manifest-path /src/Cargo.toml --all-targets --locked --offline --features test-hooks -- -D warnings > /out/baseline-clippy-hooks.log 2>&1
echo $? > /out/baseline-clippy-hooks.exit
echo "clippy_hooks_exit=$(cat /out/baseline-clippy-hooks.exit)"

echo "--- source hashes after cargo ---"
find /src -type f | sort | xargs sha256sum > /out/baseline-src-after.sha256
sha256sum /src/Cargo.toml /src/Cargo.lock > /out/baseline-manifest-after.sha256
if cmp -s /out/baseline-manifest-before.sha256 /out/baseline-manifest-after.sha256; then
  echo "manifest_unchanged=yes"
else
  echo "manifest_unchanged=no"
fi
echo "phase_end=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo "baseline_phases_finished" > /out/baseline-phases.done
exit 0
