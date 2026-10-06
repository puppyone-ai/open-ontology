#!/bin/bash
# Fixed-archive query-pg, evidence example, one zero-row TLS select and one contrary row.
# Adapter and comparator are the post binaries. This script does not rebuild them.
# A probe exit of 0 is the TLS proof. Query-pg build failures stay in the evidence files.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target/a276-live
export CARGO_TERM_COLOR=never
cd /src

record() {
  printf '%s\n' "$2" > "/evidence/$1"
}

echo "=== identity ==="
rustc -vV
echo "rustc_vv_exit=$?"
cargo -V
echo "cargo_v_exit=$?"
rustup show
uname -a
cat /etc/os-release
id
echo "=== toolchain file absent ==="
if [ -f rust-toolchain.toml ]; then
  echo "repo toolchain file still active" >&2
  record toolchain-absent.exit 2
  exit 2
fi
record toolchain-absent.exit 0
if [ ! -f rust-toolchain.toml.repo ]; then
  echo "repo toolchain file missing from archive copy" >&2
  record toolchain-absent.exit 2
  exit 2
fi

rustc_version=$(rustc -vV | awk '/^release:/{print $2}')
printf '%s\n' "$rustc_version" > /evidence/rustc-release.txt
if [ "$rustc_version" != "1.94.0" ]; then
  echo "rustc is not 1.94.0" >&2
  record rustc-release.exit 2
  exit 2
fi
record rustc-release.exit 0

echo "=== reused binaries ==="
ADAPTER=/target/a276-deps/debug/query-result-adapter
COMPARATOR=/target/a276-deps/debug/result-comparator
if [ ! -x "$ADAPTER" ] || [ ! -x "$COMPARATOR" ]; then
  echo "post adapter or comparator binary is missing" >&2
  record reused-bin.exit 2
  exit 2
fi
sha256sum "$ADAPTER" "$COMPARATOR" > /evidence/reused-bin-sha256.txt
adapter_sha=$(awk 'NR==1 {print $1}' /evidence/reused-bin-sha256.txt)
comparator_sha=$(awk 'NR==2 {print $1}' /evidence/reused-bin-sha256.txt)
if [ "$adapter_sha" != "3313d9321d689b6f70ab07389411f55d7359939c30ac66e48452e4db14210df1" ] \
  || [ "$comparator_sha" != "fccc84374ddafa93f247587b2d8c36035c1d42c07bdbf7e032eef81f12313914" ]
then
  echo "reused binary sha mismatch" >&2
  record reused-bin.exit 2
  exit 2
fi
record reused-bin.exit 0
echo "PHASE reused_bin_ok"

if [ ! -f crates/oo-gateway/examples/a276_empty_result.rs ]; then
  echo "evidence example missing from the archive copy" >&2
  record example-present.exit 2
  exit 2
fi
record example-present.exit 0

echo "=== libclang tools ==="
mkdir -p /cargo-home/bin /evidence/libclang
set +e
rustc --edition 2021 -D unsafe_code /tools/sha256.rs -o /cargo-home/bin/sha256 \
  > /evidence/sha256-tool.stdout 2> /evidence/sha256-tool.stderr
sha_tool=$?
set -e
record sha256-tool.exit "$sha_tool"
echo "PHASE sha256_tool_exit=$sha_tool"
if [ "$sha_tool" -ne 0 ]; then
  exit "$sha_tool"
fi
set +e
rustc --edition 2021 -D unsafe_code /tools/select-debs.rs -o /cargo-home/bin/select-debs \
  > /evidence/select-debs-tool.stdout 2> /evidence/select-debs-tool.stderr
select_tool=$?
set -e
record select-debs-tool.exit "$select_tool"
echo "PHASE select_debs_tool_exit=$select_tool"
if [ "$select_tool" -ne 0 ]; then
  exit "$select_tool"
fi
if [ ! -s /evidence/libclang/libclang-dir.txt ] || [ ! -d /cargo-home/a276-clang-root/usr/lib ]; then
  set +e
  bash /extract-libclang.sh > /evidence/libclang/extract.log 2>&1
  extract_exit=$?
  set -e
  record libclang.exit "$extract_exit"
  echo "PHASE libclang_exit=$extract_exit"
  if [ "$extract_exit" -ne 0 ]; then
    exit "$extract_exit"
  fi
else
  echo "libclang_root=reused" > /evidence/libclang/extract.log
  record libclang.exit 0
  echo "PHASE libclang_exit=0"
fi
libdir=$(cat /evidence/libclang/libclang-dir.txt)
multiarch=/cargo-home/a276-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a276-clang-root/usr/lib/llvm-19/lib/clang/19/include
real=$(find /cargo-home/a276-clang-root/usr/lib -name 'libclang-19.so.19' -print -quit)
devso=$(find /cargo-home/a276-clang-root/usr/lib -name 'libclang.so' -print -quit)
if [ -n "$devso" ]; then
  libdir=$(dirname "$devso")
fi
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a276-clang-root/usr/share/pkgconfig"
{
  echo "LIBCLANG_PATH=$LIBCLANG_PATH"
  echo "LD_LIBRARY_PATH=$LD_LIBRARY_PATH"
  test -f "$llvm_inc/stdbool.h" && echo "stdbool.h=present" || echo "stdbool.h=absent"
} > /evidence/libclang/libclang.txt
if [ ! -f "$llvm_inc/stdbool.h" ] || [ -z "$real" ]; then
  echo "libclang headers or shared library missing" >&2
  record libclang-load.exit 1
  exit 1
fi
/lib/ld-linux-aarch64.so.1 --list "$real" > /evidence/libclang/libclang-ldd.txt 2>&1 || true
if grep -Eq 'not found|error while loading' /evidence/libclang/libclang-ldd.txt; then
  echo "libclang did not load" >&2
  record libclang-load.exit 1
  exit 1
fi
record libclang-load.exit 0
echo "PHASE libclang_load_ok"

sha256sum Cargo.lock crates/oo-gateway/Cargo.toml > /evidence/lock-before.txt

echo "=== cargo fetch query-pg ==="
mkdir -p /target/a276-live
set +e
cargo fetch --manifest-path crates/oo-gateway/Cargo.toml --locked --offline \
  > /evidence/fetch-query-pg-offline.stdout 2> /evidence/fetch-query-pg-offline.stderr
offline_fetch=$?
set -e
record fetch-query-pg-offline.exit "$offline_fetch"
echo "PHASE query_pg_offline_fetch_exit=$offline_fetch"
fetch_exit=$offline_fetch
if [ "$offline_fetch" -ne 0 ]; then
  set +e
  cargo fetch --manifest-path crates/oo-gateway/Cargo.toml --locked \
    > /evidence/fetch-query-pg.stdout 2> /evidence/fetch-query-pg.stderr
  fetch_exit=$?
  set -e
else
  cp /evidence/fetch-query-pg-offline.stdout /evidence/fetch-query-pg.stdout
  cp /evidence/fetch-query-pg-offline.stderr /evidence/fetch-query-pg.stderr
fi
record fetch-query-pg.exit "$fetch_exit"
echo "PHASE query_pg_fetch_exit=$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  exit "$fetch_exit"
fi

echo "=== cargo build query-pg ==="
set +e
cargo build --manifest-path crates/oo-gateway/Cargo.toml --locked --offline --bin query-pg \
  > /evidence/build-query-pg-offline.stdout 2> /evidence/build-query-pg-offline.stderr
offline_build=$?
set -e
record build-query-pg-offline.exit "$offline_build"
echo "PHASE query_pg_offline_build_exit=$offline_build"
build_exit=$offline_build
if [ "$offline_build" -ne 0 ]; then
  set +e
  cargo build --manifest-path crates/oo-gateway/Cargo.toml --locked --bin query-pg \
    > /evidence/build-query-pg.stdout 2> /evidence/build-query-pg.stderr
  build_exit=$?
  set -e
else
  cp /evidence/build-query-pg-offline.stdout /evidence/build-query-pg.stdout
  cp /evidence/build-query-pg-offline.stderr /evidence/build-query-pg.stderr
fi
record build-query-pg.exit "$build_exit"
echo "PHASE query_pg_build_exit=$build_exit"
if [ "$build_exit" -ne 0 ]; then
  exit "$build_exit"
fi

echo "=== cargo rustc example ==="
set +e
cargo rustc --manifest-path crates/oo-gateway/Cargo.toml --locked --offline \
  --example a276_empty_result -- -D warnings \
  > /evidence/build-example.stdout 2> /evidence/build-example.stderr
example_exit=$?
set -e
record build-example.exit "$example_exit"
echo "PHASE example_exit=$example_exit"
if [ "$example_exit" -ne 0 ]; then
  exit "$example_exit"
fi

sha256sum Cargo.lock crates/oo-gateway/Cargo.toml > /evidence/lock-after.txt
if ! cmp -s /evidence/lock-before.txt /evidence/lock-after.txt; then
  echo "workspace lock or gateway manifest changed" >&2
  record lock-unchanged.exit 2
  exit 2
fi
record lock-unchanged.exit 0

if [ ! -x /target/a276-live/debug/query-pg ] || [ ! -x /target/a276-live/debug/examples/a276_empty_result ]; then
  echo "built probe binaries missing" >&2
  record probe-bins.exit 2
  exit 2
fi
sha256sum /target/a276-live/debug/query-pg /target/a276-live/debug/examples/a276_empty_result \
  > /evidence/probe-bin-sha256.txt
record probe-bins.exit 0

mkdir -p /evidence/out
unset LIBCLANG_PATH LD_LIBRARY_PATH BINDGEN_EXTRA_CLANG_ARGS PKG_CONFIG_PATH
echo "=== tls probe ==="
set +e
/target/a276-live/debug/examples/a276_empty_result \
  > /evidence/probe.stdout 2> /evidence/probe.stderr
probe_exit=$?
set -e
record probe.exit "$probe_exit"
echo "PHASE probe_exit=$probe_exit"

secret_hit=0
for secret in /secrets/login.pass /secrets/admin.pass; do
  if grep -R -F -q -f "$secret" /evidence/out /evidence/probe.stdout /evidence/probe.stderr; then
    secret_hit=1
  fi
done
if [ "$secret_hit" -ne 0 ]; then
  echo "secret-in-output" >&2
  record secret-scan.exit 2
  exit 2
fi
record secret-scan.exit 0

if [ "$probe_exit" -ne 0 ]; then
  exit "$probe_exit"
fi
if [ ! -f /evidence/out/oracle.txt ]; then
  echo "oracle missing" >&2
  exit 2
fi
cp /evidence/out/oracle.txt /evidence/oracle.txt
for needle in \
  'zero_query_exit=0' \
  'zero_adapter_exit=0' \
  'zero_comparator_exit=0' \
  'zero_verdict=match' \
  'zero_returned_rows=0' \
  'nonzero_query_exit=0' \
  'nonzero_adapter_exit=0' \
  'nonzero_comparator_exit=1' \
  'nonzero_verdict=mismatch' \
  'nonzero_kind=rows' \
  'nonzero_returned_rows=1'
do
  if ! grep -F -q "$needle" /evidence/oracle.txt; then
    echo "oracle missing $needle" >&2
    exit 2
  fi
done
printf '%s\n' '{"kind":"not_found","completion":"complete","warnings":[]}' > /evidence/not-found.expected
if ! cmp -s /evidence/out/zero.adapter.stdout /evidence/not-found.expected; then
  echo "zero-row adapter stdout is not the not_found object" >&2
  exit 2
fi
if ! grep -F -q '"kind":"rows"' /evidence/out/nonzero.adapter.stdout; then
  echo "nonzero adapter did not emit rows" >&2
  exit 2
fi
echo "PHASE live_ok"
exit 0
