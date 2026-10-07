#!/bin/bash
# Fixed archive 96afe70 plus the ontology-dialogue overlay.
# One Rust 1.94 builder, jobs=1. The private PostgreSQL is a sibling container.
set -u
echo "oo_task=sp11-ontology-dialogue"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a300"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_BUILD_JOBS=1
export CARGO_TERM_COLOR=never
export HOME=/tmp
mkdir -p /evidence/linux/proof-3/phases /tmp
cd /src
if [ -f rust-toolchain.toml ]; then
  mv rust-toolchain.toml /tmp/rust-toolchain.toml
fi
rustc -Vv
cargo -V
got=$(rustc --version)
printf '%s\n' "$got"
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; exit 2 ;;
esac
if [ ! -f tools/ontology-dialogue/Cargo.lock ] || [ ! -f tools/ontology-dialogue/src/lib.rs ]; then
  echo "overlay is missing the dialogue crate" >&2
  exit 2
fi
if [ ! -f Cargo.lock ] || [ ! -f crates/oo-gateway/src/bin/query-pg.rs ]; then
  echo "fixed archive is missing query-pg" >&2
  exit 2
fi

record() {
  printf '%s\n' "$2" > "/evidence/linux/proof-3/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    exit "$2"
  fi
}

echo "=== lock checksum subset ==="
rustc --edition 2021 -O -o /tmp/lock-assemble /evidence/linux/lock-assemble.rs
set +e
/tmp/lock-assemble --check \
  tools/description-provider/Cargo.lock \
  Cargo.lock \
  tools/answer-citations/Cargo.lock \
  tools/ontology-dialogue/Cargo.lock \
  > /evidence/linux/proof-3/lock-check.stdout 2> /evidence/linux/proof-3/lock-check.stderr
lock_exit=$?
set -e
record lock-check "$lock_exit"

echo "=== cargo fetch ontology-dialogue ==="
set +e
cargo fetch --manifest-path tools/ontology-dialogue/Cargo.toml --locked \
  > /evidence/linux/proof-3/fetch-dialogue.stdout 2> /evidence/linux/proof-3/fetch-dialogue.stderr
fetch_exit=$?
set -e
record fetch-dialogue "$fetch_exit"

echo "=== cargo test ontology-dialogue ==="
set +e
cargo test --manifest-path tools/ontology-dialogue/Cargo.toml --locked --offline \
  > /evidence/linux/proof-3/test-dialogue.stdout 2> /evidence/linux/proof-3/test-dialogue.stderr
test_exit=$?
set -e
record test-dialogue "$test_exit"

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy --toolchain 1.94.0 \
    > /evidence/linux/proof-3/clippy-component.stdout 2> /evidence/linux/proof-3/clippy-component.stderr
  clippy_add=$?
  set -e
  record clippy-component "$clippy_add"
else
  record clippy-component 0
fi

echo "=== cargo clippy ontology-dialogue ==="
set +e
cargo clippy --manifest-path tools/ontology-dialogue/Cargo.toml --locked --offline --all-targets -- -D warnings \
  > /evidence/linux/proof-3/clippy-dialogue.stdout 2> /evidence/linux/proof-3/clippy-dialogue.stderr
clippy_exit=$?
set -e
record clippy-dialogue "$clippy_exit"

echo "=== answer-citations ==="
set +e
cargo fetch --manifest-path tools/answer-citations/Cargo.toml --locked \
  > /evidence/linux/proof-3/fetch-citations.stdout 2> /evidence/linux/proof-3/fetch-citations.stderr
cit_fetch=$?
set -e
record fetch-citations "$cit_fetch"
set +e
cargo build --manifest-path tools/answer-citations/Cargo.toml --locked --offline \
  > /evidence/linux/proof-3/build-citations.stdout 2> /evidence/linux/proof-3/build-citations.stderr
cit_build=$?
set -e
record build-citations "$cit_build"

echo "=== execute_approval with answer-citations ==="
set +e
OO_A300_ANSWER_CITATIONS=/target/debug/answer-citations \
  cargo test --manifest-path tools/ontology-dialogue/Cargo.toml --locked --offline --test execute_approval \
  > /evidence/linux/proof-3/test-citations.stdout 2> /evidence/linux/proof-3/test-citations.stderr
cit_test=$?
set -e
record test-citations "$cit_test"

echo "=== libclang ==="
set +e
bash /evidence/linux/libclang-extract.sh > /evidence/linux/proof-3/libclang-extract.log 2>&1
extract_exit=$?
set -e
record libclang "$extract_exit"
rawdir=$(cat /evidence/linux/libclang-dir.txt)
if [ ! -d /cargo-home/a300-clang-root/usr/lib ]; then
  cp -a /tmp/clang-root /cargo-home/a300-clang-root
fi
libdir="/cargo-home/a300-clang-root${rawdir#/tmp/clang-root}"
printf '%s\n' "$libdir" > /evidence/linux/proof-3/libclang-dir.txt
multiarch=/cargo-home/a300-clang-root/usr/lib/aarch64-linux-gnu
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I/cargo-home/a300-clang-root/usr/lib/llvm-19/lib/clang/19/include -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/a300-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a300-clang-root/usr/share/pkgconfig"
{
  echo "LIBCLANG_PATH=$LIBCLANG_PATH"
  ls -l "$libdir"/libclang.so* || true
} > /evidence/linux/proof-3/libclang.txt
if ! /lib/ld-linux-aarch64.so.1 --list "${multiarch}/libclang-19.so.19" > /evidence/linux/proof-3/libclang-ldd.txt 2>&1; then
  record libclang-load 1
fi
if grep -q 'not found' /evidence/linux/proof-3/libclang-ldd.txt; then
  record libclang-load 1
fi
record libclang-load 0

echo "=== cargo fetch workspace ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked \
  > /evidence/linux/proof-3/fetch-workspace.stdout 2> /evidence/linux/proof-3/fetch-workspace.stderr
root_fetch=$?
set -e
record fetch-workspace "$root_fetch"

echo "=== build query-pg install-column-acl provision-mapping-reader ==="
set +e
cargo build --manifest-path Cargo.toml --locked --offline \
  --bin query-pg --bin install-column-acl --bin provision-mapping-reader \
  > /evidence/linux/proof-3/build-query.stdout 2> /evidence/linux/proof-3/build-query.stderr
query_build=$?
set -e
record build-query "$query_build"

echo "=== pg-onboarding ==="
set +e
cargo fetch --manifest-path tools/pg-onboarding/Cargo.toml --locked \
  > /evidence/linux/proof-3/fetch-onboarding.stdout 2> /evidence/linux/proof-3/fetch-onboarding.stderr
on_fetch=$?
set -e
record fetch-onboarding "$on_fetch"
set +e
cargo build --manifest-path tools/pg-onboarding/Cargo.toml --locked --offline \
  > /evidence/linux/proof-3/build-onboarding.stdout 2> /evidence/linux/proof-3/build-onboarding.stderr
on_build=$?
set -e
record build-onboarding "$on_build"

echo "=== live dialogue ==="
set +e
OO_A300_LIVE=1 \
OO_A300_PG_HOST=oo-a300-pg \
OO_A300_PG_CA=/run/oo-a300/fixture/ca.crt \
OO_A300_ADMIN_PASSWORD_FILE=/run/oo-a300/fixture/admin.pass \
OO_A300_LOGIN_PASSWORD_FILE=/run/oo-a300/fixture/login.pass \
OO_A300_QUERY_PG=/target/debug/query-pg \
OO_A300_INSTALL_COLUMN_ACL=/target/debug/install-column-acl \
OO_A300_PROVISION_MAPPING_READER=/target/debug/provision-mapping-reader \
OO_A300_PG_ONBOARDING=/target/debug/pg-onboarding \
OO_A300_ANSWER_CITATIONS=/target/debug/answer-citations \
OO_A300_ORACLE=/evidence/linux/proof-3/live/oracle.txt \
  cargo test --manifest-path tools/ontology-dialogue/Cargo.toml --locked --offline --test live -- --nocapture \
  > /evidence/linux/proof-3/test-live.stdout 2> /evidence/linux/proof-3/test-live.stderr
live_exit=$?
set -e
record test-live "$live_exit"
if [ ! -f /evidence/linux/proof-3/live/oracle.txt ] || ! grep -q '^live_done=1$' /evidence/linux/proof-3/live/oracle.txt; then
  echo "live oracle missing" >&2
  record live-oracle 1
fi
record live-oracle 0

echo "=== sha256 manifest ==="
rustc --edition 2021 -O -o /tmp/sha256_manifest /evidence/tools/sha256_manifest.rs
/tmp/sha256_manifest /evidence /evidence/linux/proof-3/MANIFEST.sha256 proof \
  > /evidence/linux/proof-3/manifest.stdout
record manifest 0
echo "proof_inside_done"
