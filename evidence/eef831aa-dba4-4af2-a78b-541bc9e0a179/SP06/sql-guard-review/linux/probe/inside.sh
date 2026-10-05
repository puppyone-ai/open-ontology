#!/bin/bash
# Fresh oracle on the fixed archive plus one review test. No cargo update.
set -u
echo "oo_task=sp06-sql-guard-review-probe"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a179"
echo "oo_fixed=03512f91324ac33e772e9702fe806e8e04037ec4"
echo "phase=oracle"
uname -a
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
cd /src

record() {
  mkdir -p /evidence/linux/probe/phases
  printf '%s\n' "$2" > "/evidence/linux/probe/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/probe/probe.exit
    exit "$2"
  fi
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/linux/probe/rustc-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
if [ ! -f crates/oo-gateway/tests/sql_guard_a179_review.rs ]; then
  echo "review overlay is absent" >&2
  record overlay 2
fi
if grep -n 'unsafe' crates/oo-gateway/tests/sql_guard_a179_review.rs > /evidence/linux/probe/unsafe-lines.txt; then
  record unsafe 1
fi
record unsafe 0

ROOT=/cargo-home/a179-clang-root
libdir=$(cat /evidence/linux/smoke/libclang-dir.txt)
multiarch="$ROOT/usr/lib/aarch64-linux-gnu"
llvm_inc="$ROOT/usr/lib/llvm-19/lib/clang/19/include"
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="$ROOT/usr/bin:${PATH}"

: > /evidence/linux/probe/crate-manifests-raw.txt
while read -r name version; do
  [ -n "${name:-}" ] || continue
  toml=$(find /cargo-home/registry/src -path "*/${name}-${version}/Cargo.toml" -print -quit)
  {
    echo "## $name $version"
    if [ -n "$toml" ]; then
      grep -E '^(license|rust-version|links) ' "$toml" || true
    else
      echo "missing"
    fi
  } >> /evidence/linux/probe/crate-manifests-raw.txt
done < /evidence/linux/lock/added-pairs.txt
pg_src=$(find /cargo-home/registry/src -path '*/pg_query-6.2.1/src/lib.rs' -print -quit)
pg_dir=$(dirname "$(dirname "$pg_src")")
grep -n 'PG_VERSION_NUM 170007' "$pg_dir/libpg_query/src/postgres/include/pg_config.h" \
  > /evidence/linux/probe/pg-version-num.txt
grep -R -c 'unsafe' "$pg_dir/src" > /evidence/linux/probe/pg-query-unsafe-count.txt || true
grep -R -l 'Portions Copyright' "$pg_dir/libpg_query/src/postgres" | head -3 \
  > /evidence/linux/probe/pg-copyright-sample.txt
record pin-notes 0

echo "=== cargo test review oracle ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --test sql_guard_a179_review -- --test-threads=8 \
  > /evidence/linux/probe/oracle-test.log 2>&1
test_exit=$?
set -e
record oracle-test "$test_exit"

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/linux/probe/clippy-component.log 2>&1
  clippy_add=$?
  set -e
  record clippy-component "$clippy_add"
else
  echo "clippy_component=already_installed" > /evidence/linux/probe/clippy-component.log
  record clippy-component 0
fi

echo "=== cargo clippy ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-gateway --all-targets -- -D warnings \
  > /evidence/linux/probe/clippy.log 2>&1
clippy_exit=$?
set -e
record clippy "$clippy_exit"

printf '%s\n' 0 > /evidence/linux/probe/probe.exit
echo "probe_ok"
