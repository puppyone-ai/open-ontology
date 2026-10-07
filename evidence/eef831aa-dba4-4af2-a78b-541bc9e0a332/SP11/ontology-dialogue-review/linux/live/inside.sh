#!/bin/bash
# Live phase on the same immutable archive. One private PostgreSQL is already up.
set -u
echo "oo_task=sp11-ontology-dialogue-review-live"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a332"
echo "oo_fixed=63566b60aabdf29a29bfe55c4866edf3c4081cfd"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
uname -a
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_BUILD_JOBS=1
export CARGO_TERM_COLOR=never
export HOME=/tmp
mkdir -p /evidence/linux/live/phases /cargo-home/bin /tmp
cd /src

record() {
  printf '%s\n' "$2" > "/evidence/linux/live/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    exit "$2"
  fi
}

got=$(rustc --version)
printf '%s\n' "$got" > /evidence/linux/live/rustc-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
record toolchain 0
if [ -f rust-toolchain.toml ] || [ ! -f tools/ontology-dialogue/src/execute.rs ]; then
  echo "archive identity failed" >&2
  record source 2
fi
record source 0

set +e
sha_tool=0
select_tool=0
if [ ! -x /cargo-home/bin/sha256 ]; then
  rustc --edition 2021 -O -o /cargo-home/bin/sha256 /review/sha256.rs
  sha_tool=$?
fi
if [ ! -x /cargo-home/bin/select-debs ]; then
  rustc --edition 2021 -O -o /cargo-home/bin/select-debs /review/select-debs.rs
  select_tool=$?
fi
rustc --edition 2021 -O -o /cargo-home/bin/artifact_check /review/artifact_check.rs
artifact_tool=$?
rustc --edition 2021 -O -o /cargo-home/bin/leak_scan /review/leak_scan.rs
leak_tool=$?
set -e
if [ "$sha_tool" -ne 0 ] || [ "$select_tool" -ne 0 ] || [ "$artifact_tool" -ne 0 ] || [ "$leak_tool" -ne 0 ]; then
  record review-tools 1
fi
export PATH="/cargo-home/bin:${PATH}"
record review-tools 0

echo "=== answer-citations ==="
set +e
cargo fetch --manifest-path tools/answer-citations/Cargo.toml --locked \
  > /evidence/linux/live/fetch-citations.stdout 2> /evidence/linux/live/fetch-citations.stderr
cit_fetch=$?
set -e
record fetch-citations "$cit_fetch"
set +e
cargo build --manifest-path tools/answer-citations/Cargo.toml --locked --offline \
  > /evidence/linux/live/build-citations.stdout 2> /evidence/linux/live/build-citations.stderr
cit_build=$?
set -e
record build-citations "$cit_build"

echo "=== execute_approval with answer-citations binary ==="
set +e
OO_A300_ANSWER_CITATIONS=/target/debug/answer-citations \
  cargo test --manifest-path tools/ontology-dialogue/Cargo.toml --locked --offline --test execute_approval \
  > /evidence/linux/live/test-citations.stdout 2> /evidence/linux/live/test-citations.stderr
cit_test=$?
set -e
record test-citations "$cit_test"

echo "=== libclang ==="
set +e
bash /extract-libclang.sh > /evidence/linux/live/libclang-extract.log 2>&1
extract_exit=$?
set -e
record libclang "$extract_exit"
libdir=$(cat /evidence/linux/live/libclang-dir.txt)
multiarch=/cargo-home/a332-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a332-clang-root/usr/lib/llvm-19/lib/clang/19/include
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a332-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a332-clang-root/usr/share/pkgconfig"
{
  echo "LIBCLANG_PATH=$LIBCLANG_PATH"
  test -f "$llvm_inc/stdbool.h" && echo "stdbool.h=present" || echo "stdbool.h=absent"
  ls -l "$libdir"/libclang.so* || true
} > /evidence/linux/live/libclang.txt
if [ ! -f "$llvm_inc/stdbool.h" ]; then
  record libclang-includes 1
fi
record libclang-includes 0
real=$(find /cargo-home/a332-clang-root/usr/lib -name 'libclang-19.so.19' -print -quit)
devso=$(find /cargo-home/a332-clang-root/usr/lib -name 'libclang.so' -print -quit)
if [ -n "$devso" ]; then
  export LIBCLANG_PATH="$(dirname "$devso")"
fi
export LD_LIBRARY_PATH="${LIBCLANG_PATH}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
if [ -z "$real" ]; then
  record libclang-load 1
fi
/lib/ld-linux-aarch64.so.1 --list "$real" > /evidence/linux/live/libclang-ldd.txt 2>&1 || true
if grep -Eq 'not found|error while loading' /evidence/linux/live/libclang-ldd.txt; then
  record libclang-load 1
fi
if ! grep -q 'libLLVM' /evidence/linux/live/libclang-ldd.txt; then
  record libclang-load 1
fi
record libclang-load 0

echo "=== cargo fetch workspace ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked \
  > /evidence/linux/live/fetch-workspace.stdout 2> /evidence/linux/live/fetch-workspace.stderr
root_fetch=$?
set -e
record fetch-workspace "$root_fetch"

echo "=== build query-pg install-column-acl provision-mapping-reader ==="
set +e
cargo build --manifest-path Cargo.toml --locked --offline \
  --bin query-pg --bin install-column-acl --bin provision-mapping-reader \
  > /evidence/linux/live/build-query.stdout 2> /evidence/linux/live/build-query.stderr
query_build=$?
set -e
record build-query "$query_build"

echo "=== pg-onboarding ==="
set +e
cargo fetch --manifest-path tools/pg-onboarding/Cargo.toml --locked \
  > /evidence/linux/live/fetch-onboarding.stdout 2> /evidence/linux/live/fetch-onboarding.stderr
on_fetch=$?
set -e
record fetch-onboarding "$on_fetch"
set +e
cargo build --manifest-path tools/pg-onboarding/Cargo.toml --locked --offline \
  > /evidence/linux/live/build-onboarding.stdout 2> /evidence/linux/live/build-onboarding.stderr
on_build=$?
set -e
record build-onboarding "$on_build"

echo "=== live dialogue ==="
set +e
OO_A300_LIVE=1 \
OO_A300_PG_HOST=oo-a332-pg \
OO_A300_PG_CA=/run/oo-a332/fixture/ca.crt \
OO_A300_ADMIN_PASSWORD_FILE=/run/oo-a332/fixture/admin.pass \
OO_A300_LOGIN_PASSWORD_FILE=/run/oo-a332/fixture/login.pass \
OO_A300_QUERY_PG=/target/debug/query-pg \
OO_A300_INSTALL_COLUMN_ACL=/target/debug/install-column-acl \
OO_A300_PROVISION_MAPPING_READER=/target/debug/provision-mapping-reader \
OO_A300_PG_ONBOARDING=/target/debug/pg-onboarding \
OO_A300_ANSWER_CITATIONS=/target/debug/answer-citations \
OO_A300_ORACLE=/evidence/linux/live/oracle.txt \
  cargo test --manifest-path tools/ontology-dialogue/Cargo.toml --locked --offline --test live -- --nocapture \
  > /evidence/linux/live/test-live.stdout 2> /evidence/linux/live/test-live.stderr
live_exit=$?
set -e
printf '%s\n' "$live_exit" > /evidence/linux/live/phases/test-live.exit
echo "phase_exit=$live_exit phase=test-live"

set +e
/cargo-home/bin/artifact_check \
  > /evidence/linux/live/artifact-check.stdout 2> /evidence/linux/live/artifact-check.stderr
artifact_exit=$?
set -e
printf '%s\n' "$artifact_exit" > /evidence/linux/live/phases/artifact-check.exit
echo "phase_exit=$artifact_exit phase=artifact-check"
if [ "$live_exit" -ne 0 ]; then
  exit "$live_exit"
fi
if [ "$artifact_exit" -ne 0 ]; then
  exit "$artifact_exit"
fi

set +e
/cargo-home/bin/leak_scan /run/oo-a332/fixture/admin.pass /evidence \
  > /evidence/linux/live/leak-admin.stdout 2> /evidence/linux/live/leak-admin.stderr
leak_admin=$?
set -e
record leak-admin "$leak_admin"
set +e
/cargo-home/bin/leak_scan /run/oo-a332/fixture/login.pass /evidence \
  > /evidence/linux/live/leak-login.stdout 2> /evidence/linux/live/leak-login.stderr
leak_login=$?
set -e
record leak-login "$leak_login"

printf '%s\n' "no" > /evidence/linux/live/python-invoked.txt
echo "live_inside_done"
