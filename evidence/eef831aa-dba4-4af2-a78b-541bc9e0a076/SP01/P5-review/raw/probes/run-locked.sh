#!/bin/bash
# Independent a076 locked Linux arm64 Rust 1.94 run. Source mount is read-only.
set -u
cd /src
exec > >(tee /out/locked-build.log) 2>&1
echo "oo_owner=a076"
echo "oo_scope=sp01-p5-review"
date -u +%Y-%m-%dT%H:%M:%SZ
echo "id=$(id)"
echo "uname=$(uname -a)"
rustc -Vv || true
cargo -V || true
echo "RUSTUP_TOOLCHAIN=${RUSTUP_TOOLCHAIN:-}"
got=$(rustc --version || true)
echo "rustc_version=$got"
capsh --print 2>/dev/null | head -20 || true
echo '--- proc1 status caps ---'
grep -E 'Cap|NoNewPrivs' /proc/1/status || true
echo '--- self status caps ---'
grep -E 'Cap|NoNewPrivs' /proc/self/status || true
if command -v protoc >/dev/null 2>&1; then
  echo "protoc_present=yes"
  command -v protoc
else
  echo "protoc_present=no"
fi
echo '--- libclang before apt ---'
command -v clang || echo 'clang_absent'
ldconfig -p 2>/dev/null | grep -i libclang || echo 'libclang_absent'
if ! command -v clang >/dev/null 2>&1 || ! ldconfig -p 2>/dev/null | grep -q 'libclang'; then
  export DEBIAN_FRONTEND=noninteractive
  apt-get update
  apt_rc=$?
  echo "apt_update_exit=$apt_rc"
  if [ "$apt_rc" -ne 0 ]; then
    echo "apt_update_failed"
    exit 30
  fi
  apt-get install -y --no-install-recommends clang libclang-dev llvm-dev pkg-config ca-certificates
  echo "apt_install_exit=$?"
fi
clang --version
echo '--- libclang after ---'
ldconfig -p | grep -i libclang || true
pkg-config --modversion libclang 2>/dev/null || true
llvm-config --version 2>/dev/null || true
if ! rustup component list --installed | grep -q '^clippy'; then
  rustup component add clippy
  echo "clippy_add_exit=$?"
fi
rustup component list --installed
echo '--- fetch locked, no -p ---'
cargo fetch --manifest-path /src/Cargo.toml --locked
echo "fetch_exit=$?"
if [ -e /src/Cargo.lock ]; then
  echo 'lock_still_present'
fi
# prove the mounted lock was not rewritten: checksum from the read-only mount
sha256sum /src/Cargo.lock /src/Cargo.toml
echo '--- tests default ---'
set +e
cargo test --manifest-path /src/Cargo.toml --locked --offline -- --test-threads=8
test_default=$?
echo "test_default_exit=$test_default"
set -e
if [ -x /target/debug/oo-p05-parser-worker ]; then
  cp -a /target/debug/oo-p05-parser-worker /out/worker-plain
  cp -a /target/debug/oo-p05-pg17-parser-smoke /out/smoke-plain
fi
echo '--- tests test-hooks ---'
set +e
cargo test --manifest-path /src/Cargo.toml --locked --offline --features test-hooks -- --test-threads=8
test_hooks=$?
echo "test_hooks_exit=$test_hooks"
set -e
if [ -x /target/debug/oo-p05-parser-worker ]; then
  cp -a /target/debug/oo-p05-parser-worker /out/worker-hooks
fi
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
# hooks clippy may rebuild the worker without preserving hooks if features differ.
# Rebuild the hooks worker explicitly and copy it again.
cargo build --manifest-path /src/Cargo.toml --locked --offline --features test-hooks --bins
echo "build_hooks_bins_exit=$?"
cp -a /target/debug/oo-p05-parser-worker /out/worker-hooks
cargo build --manifest-path /src/Cargo.toml --locked --offline --bins
echo "build_plain_bins_exit=$?"
cp -a /target/debug/oo-p05-parser-worker /out/worker-plain
cp -a /target/debug/oo-p05-pg17-parser-smoke /out/smoke-plain
echo '--- vendored parser identity ---'
find /cargo-home/registry/src -path '*pg_query-6.2.0*' -name 'pg_config.h' -print > /out/pg-config-paths.txt || true
while IFS= read -r hit; do
  echo "PG_CONFIG=$hit"
  grep -E 'PG_VERSION|PACKAGE_VERSION' "$hit" || true
done < /out/pg-config-paths.txt
find /cargo-home/registry/src -path '*pg_query-6.2.0*' -name 'pg_version.h' -print > /out/pg-version-paths.txt || true
while IFS= read -r hit; do
  echo "PG_VERSION_H=$hit"
  grep -E 'PG_VERSION' "$hit" || true
done < /out/pg-version-paths.txt
find /cargo-home/registry/cache -name 'pg_query-6.2.0.crate' -print > /out/crate-paths.txt || true
while IFS= read -r item; do
  echo -n "crate_file=$item "
  sha256sum "$item"
  wc -c "$item"
done < /out/crate-paths.txt
find /cargo-home/registry/src -path '*pg_query-6.2.0*' \( -name 'LIB_VERSION' -o -name 'pg_query.h' -o -name 'COPYING' -o -name 'LICENSE' \) -print | head -80 > /out/native-meta-paths.txt || true
echo '--- native meta paths ---'
cat /out/native-meta-paths.txt || true
echo '--- libclang linked? smoke ldd ---'
ldd /out/smoke-plain || true
echo "locked_phase_done test_default=$test_default test_hooks=$test_hooks clippy_default=$clippy_default clippy_hooks=$clippy_hooks"
if [ "$test_default" -ne 0 ] || [ "$test_hooks" -ne 0 ] || [ "$clippy_default" -ne 0 ] || [ "$clippy_hooks" -ne 0 ]; then
  exit 1
fi
exit 0
