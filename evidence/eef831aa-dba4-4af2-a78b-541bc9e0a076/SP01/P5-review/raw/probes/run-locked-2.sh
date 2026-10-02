#!/bin/bash
# Continue after apt-get exit 100 under cap-drop ALL. Extract verified Debian debs; do not apt.
set -u
exec > >(tee /out/locked-build-2.log) 2>&1
echo "oo_owner=a076 phase=locked-2"
date -u +%Y-%m-%dT%H:%M:%SZ
echo '--- extract debs ---'
rm -rf /opt/clangroot
mkdir -p /opt/clangroot
for deb in /probe/debs/*.deb; do
  echo "extract $deb"
  tmp=$(mktemp -d)
  if ! (cd "$tmp" && ar x "$deb"); then
    echo "ar_failed $deb"
    exit 31
  fi
  data=$(echo "$tmp"/data.tar.*)
  tar -C /opt/clangroot -xf "$data"
  rm -rf "$tmp"
done
ln -sfn clang-19 /opt/clangroot/usr/bin/clang
export PATH="/opt/clangroot/usr/bin:${PATH}"
export LD_LIBRARY_PATH="/opt/clangroot/usr/lib/aarch64-linux-gnu${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export LIBCLANG_PATH="/opt/clangroot/usr/lib/aarch64-linux-gnu"
export PKG_CONFIG_PATH="/opt/clangroot/usr/lib/aarch64-linux-gnu/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
hash -r
echo "clang_path=$(command -v clang)"
clang --version
echo '--- libclang files ---'
find /opt/clangroot -name 'libclang.so*' -o -name 'libclang-19.so*' | head -40
echo '--- ldd libclang ---'
so=$(find /opt/clangroot -name 'libclang.so.1' -o -name 'libclang-19.so*' | head -1)
echo "so=$so"
ldd "$so" || true
missing=$(ldd "$so" | grep -c 'not found' || true)
echo "ldd_missing=$missing"
if [ "$missing" != 0 ]; then
  echo 'libclang_ldd_incomplete'
  exit 32
fi
if command -v protoc >/dev/null 2>&1; then
  echo "protoc_present=yes"
  exit 3
fi
echo "protoc_present=no"
cd /src
echo '--- fetch locked ---'
cargo fetch --manifest-path /src/Cargo.toml --locked
echo "fetch_exit=$?"
sha256sum /src/Cargo.lock
echo '--- tests default ---'
set +e
cargo test --manifest-path /src/Cargo.toml --locked --offline -- --test-threads=8
test_default=$?
echo "test_default_exit=$test_default"
set -e
cp -a /target/debug/oo-p05-parser-worker /out/worker-plain || true
cp -a /target/debug/oo-p05-pg17-parser-smoke /out/smoke-plain || true
echo '--- tests test-hooks ---'
set +e
cargo test --manifest-path /src/Cargo.toml --locked --offline --features test-hooks -- --test-threads=8
test_hooks=$?
echo "test_hooks_exit=$test_hooks"
set -e
echo '--- clippy default ---'
set +e
cargo clippy --manifest-path /src/Cargo.toml --all-targets --locked --offline -- -D warnings
clippy_default=$?
echo "clippy_default_exit=$clippy_default"
echo '--- clippy test-hooks ---'
cargo clippy --manifest-path /src/Cargo.toml --all-targets --locked --offline --features test-hooks -- -D warnings
clippy_hooks=$?
echo "clippy_hooks_exit=$clippy_hooks"
set -e
cargo build --manifest-path /src/Cargo.toml --locked --offline --features test-hooks --bins
cp -a /target/debug/oo-p05-parser-worker /out/worker-hooks
cargo build --manifest-path /src/Cargo.toml --locked --offline --bins
cp -a /target/debug/oo-p05-parser-worker /out/worker-plain
cp -a /target/debug/oo-p05-pg17-parser-smoke /out/smoke-plain
echo '--- vendored identity ---'
find /cargo-home/registry/src -path '*pg_query-6.2.0*' -name 'pg_config.h' -print > /out/pg-config-paths.txt || true
while IFS= read -r hit; do
  echo "PG_CONFIG=$hit"
  grep -E 'PG_VERSION|PACKAGE_VERSION' "$hit" || true
done < /out/pg-config-paths.txt
find /cargo-home/registry/cache -name 'pg_query-6.2.0.crate' -print > /out/crate-paths.txt || true
while IFS= read -r item; do
  sha256sum "$item"
  wc -c "$item"
done < /out/crate-paths.txt
echo '--- smoke ldd ---'
ldd /out/smoke-plain || true
echo "locked_phase_done test_default=$test_default test_hooks=$test_hooks clippy_default=$clippy_default clippy_hooks=$clippy_hooks"
printf '%s\n' "$test_default" > /out/test-default.exit
printf '%s\n' "$test_hooks" > /out/test-hooks.exit
printf '%s\n' "$clippy_default" > /out/clippy-default.exit
printf '%s\n' "$clippy_hooks" > /out/clippy-hooks.exit
if [ "$test_default" -ne 0 ] || [ "$test_hooks" -ne 0 ] || [ "$clippy_default" -ne 0 ] || [ "$clippy_hooks" -ne 0 ]; then
  exit 1
fi
exit 0
