#!/bin/bash
# Review proof. The product archive is mounted read-only. No cargo update.
set -u
phase() {
  printf '%s\n' "$2" > "/evidence/${1}.exit"
}
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_BUILD_JOBS=1
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_TERM_COLOR=never
export PATH="/usr/local/cargo/bin:/cargo-home/bin:${PATH}"

rustc --version > /evidence/rustc.txt 2>&1
phase rustc $?
if [ -f /src/rust-toolchain.toml ]; then
  echo "toolchain file still present" >&2
  phase toolchain 2
  exit 2
fi
phase toolchain 0
if [ -e /src/benchmarks/sql-runner ] || [ ! -f /src/benchmarks/result-comparator/src/lib.rs ]; then
  echo "archive shape" >&2
  phase archive-shape 2
  exit 2
fi
phase archive-shape 0

(
  cd /src
  sha256sum \
    benchmarks/result-comparator/src/lib.rs \
    benchmarks/result-comparator/src/compare.rs \
    benchmarks/result-comparator/src/parse.rs \
    benchmarks/result-comparator/src/main.rs \
    benchmarks/result-comparator/CONTRACT.md \
    benchmarks/query-result-adapter/src/lib.rs \
    benchmarks/query-result-adapter/src/parse.rs \
    benchmarks/query-result-adapter/src/main.rs \
    benchmarks/query-result-adapter/CONTRACT.md \
    benchmarks/trial-statistics/src/lib.rs \
    benchmarks/trial-statistics/src/stats.rs \
    benchmarks/trial-statistics/src/parse.rs \
    benchmarks/trial-statistics/src/main.rs \
    benchmarks/trial-statistics/CONTRACT.md \
    Cargo.toml \
    Cargo.lock
) > /evidence/product-sha256.txt
phase product-sha256 0

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy --toolchain 1.94.0 \
    > /evidence/clippy-component.stdout 2> /evidence/clippy-component.stderr
  code=$?
  set -e
  phase clippy-component "$code"
  if [ "$code" -ne 0 ]; then
    exit "$code"
  fi
else
  echo "clippy_component=already_installed" > /evidence/clippy-component.stdout
  phase clippy-component 0
fi

export CARGO_TARGET_DIR=/target/review
if [ ! -f /harness/Cargo.lock ]; then
  set +e
  cargo generate-lockfile --offline --manifest-path /harness/Cargo.toml \
    > /evidence/harness-lock.stdout 2> /evidence/harness-lock.stderr
  code=$?
  set -e
  phase harness-lock "$code"
  if [ "$code" -ne 0 ]; then
    exit "$code"
  fi
else
  phase harness-lock 0
fi
set +e
cargo clippy --manifest-path /harness/Cargo.toml --locked --offline -- -D warnings \
  > /evidence/clippy-harness.stdout 2> /evidence/clippy-harness.stderr
clip=$?
set -e
printf '%s\n' "$clip" > /evidence/clippy-harness.exit
if [ "$clip" -ne 0 ]; then
  exit "$clip"
fi
set +e
cargo build --manifest-path /harness/Cargo.toml --locked --offline \
  > /evidence/build-harness.stdout 2> /evidence/build-harness.stderr
code=$?
set -e
phase build-harness "$code"
if [ "$code" -ne 0 ]; then
  exit "$code"
fi
if [ ! -x /target/review/debug/a242-review ]; then
  echo "review binary missing" >&2
  phase review-bin 2
  exit 2
fi
phase review-bin 0

set +e
cargo fetch --locked --offline --manifest-path /src/Cargo.toml \
  > /evidence/fetch-workspace-offline.stdout 2> /evidence/fetch-workspace-offline.stderr
off=$?
set -e
printf '%s\n' "$off" > /evidence/fetch-workspace-offline.exit
if [ "$off" -ne 0 ]; then
  set +e
  cargo fetch --locked --manifest-path /src/Cargo.toml \
    > /evidence/fetch-workspace.stdout 2> /evidence/fetch-workspace.stderr
  on=$?
  set -e
  printf '%s\n' "$on" > /evidence/fetch-workspace.exit
  if [ "$on" -ne 0 ]; then
    exit "$on"
  fi
else
  printf '%s\n' 0 > /evidence/fetch-workspace.exit
fi

mkdir -p /cargo-home/bin /evidence/libclang
if [ ! -x /cargo-home/bin/sha256 ]; then
  set +e
  rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256
  code=$?
  set -e
  phase sha256-tool "$code"
  if [ "$code" -ne 0 ]; then
    exit "$code"
  fi
else
  phase sha256-tool 0
fi
if [ ! -x /cargo-home/bin/select-debs ]; then
  set +e
  rustc --edition 2021 /tools/select-debs.rs -o /cargo-home/bin/select-debs
  code=$?
  set -e
  phase select-debs "$code"
  if [ "$code" -ne 0 ]; then
    exit "$code"
  fi
else
  phase select-debs 0
fi
if [ ! -s /evidence/libclang/libclang-dir.txt ] || [ ! -d /cargo-home/a242-clang-root/usr/lib ]; then
  set +e
  bash /extract-libclang.sh > /evidence/libclang/extract.log 2>&1
  extract_exit=$?
  set -e
  phase libclang "$extract_exit"
  if [ "$extract_exit" -ne 0 ]; then
    exit "$extract_exit"
  fi
else
  echo "libclang_root=reused" > /evidence/libclang/extract.log
  phase libclang 0
fi
libdir=$(cat /evidence/libclang/libclang-dir.txt)
multiarch=/cargo-home/a242-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a242-clang-root/usr/lib/llvm-19/lib/clang/19/include
real=$(find /cargo-home/a242-clang-root/usr/lib -name 'libclang-19.so.19' -print -quit)
devso=$(find /cargo-home/a242-clang-root/usr/lib -name 'libclang.so' -print -quit)
if [ -n "$devso" ]; then
  libdir=$(dirname "$devso")
fi
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
{
  echo "LIBCLANG_PATH=$LIBCLANG_PATH"
  test -f "$llvm_inc/stdbool.h" && echo "stdbool.h=present" || echo "stdbool.h=absent"
} > /evidence/libclang/libclang.txt
if [ ! -f "$llvm_inc/stdbool.h" ] || [ -z "$real" ]; then
  phase libclang-load 1
  exit 1
fi
/lib/ld-linux-aarch64.so.1 --list "$real" > /evidence/libclang/libclang-ldd.txt 2>&1 || true
if grep -Eq 'not found|error while loading' /evidence/libclang/libclang-ldd.txt; then
  phase libclang-load 1
  exit 1
fi
phase libclang-load 0

export CARGO_TARGET_DIR=/target/bench
for name in result-comparator query-result-adapter trial-statistics; do
  set +e
  cargo clippy --manifest-path "/src/benchmarks/${name}/Cargo.toml" --locked --offline --all-targets -- -D warnings \
    > "/evidence/clippy-${name}.stdout" 2> "/evidence/clippy-${name}.stderr"
  code=$?
  set -e
  printf '%s\n' "$code" > "/evidence/clippy-${name}.exit"
  if [ "$code" -ne 0 ]; then
    exit "$code"
  fi
  set +e
  cargo build --manifest-path "/src/benchmarks/${name}/Cargo.toml" --locked --offline \
    > "/evidence/build-${name}.stdout" 2> "/evidence/build-${name}.stderr"
  code=$?
  set -e
  printf '%s\n' "$code" > "/evidence/build-${name}.exit"
  if [ "$code" -ne 0 ]; then
    exit "$code"
  fi
done

export CARGO_TARGET_DIR=/target/workspace
set +e
cargo test --manifest-path /src/Cargo.toml --locked --offline \
  -p oo-gateway --test query_pg_cli --no-run \
  > /evidence/norun.stdout 2> /evidence/norun.stderr
norun=$?
set -e
printf '%s\n' "$norun" > /evidence/norun.exit
if [ "$norun" -ne 0 ]; then
  exit "$norun"
fi
if [ ! -x /target/workspace/debug/query-pg ]; then
  echo "query-pg binary missing" >&2
  phase query-pg-bin 2
  exit 2
fi
phase query-pg-bin 0

mkdir -p /evidence/live
set +e
env OO_A198_LIVE=1 \
  cargo test --manifest-path /src/Cargo.toml --locked --offline \
  -p oo-gateway --test query_pg_cli -- --exact live_query_pg_against_private_postgres --test-threads=1 --nocapture \
  > /evidence/live/fixture.stdout 2> /evidence/live/fixture.stderr
live=$?
set -e
printf '%s\n' "$live" > /evidence/live/fixture.exit
if [ "$live" -ne 0 ]; then
  exit "$live"
fi

set +e
env EVIDENCE=/evidence \
  QUERY_PG=/target/workspace/debug/query-pg \
  ADAPTER=/target/bench/debug/query-result-adapter \
  COMPARATOR=/target/bench/debug/result-comparator \
  TRIALS=/target/bench/debug/trial-statistics \
  PASSWORD_FILE=/run/oo-a198/fixture/login.pass \
  ADMIN_FILE=/run/oo-a198/fixture/admin.pass \
  CA_FILE=/run/oo-a198/fixture/ca.crt \
  HOST=oo-a242-pg \
  /target/review/debug/a242-review \
  > /evidence/harness.stdout 2> /evidence/harness.stderr
review=$?
set -e
printf '%s\n' "$review" > /evidence/harness.exit
if [ "$review" -ne 0 ]; then
  exit "$review"
fi

(
  cd /src
  sha256sum \
    benchmarks/result-comparator/src/lib.rs \
    benchmarks/result-comparator/src/compare.rs \
    benchmarks/result-comparator/src/parse.rs \
    benchmarks/result-comparator/src/main.rs \
    benchmarks/result-comparator/CONTRACT.md \
    benchmarks/query-result-adapter/src/lib.rs \
    benchmarks/query-result-adapter/src/parse.rs \
    benchmarks/query-result-adapter/src/main.rs \
    benchmarks/query-result-adapter/CONTRACT.md \
    benchmarks/trial-statistics/src/lib.rs \
    benchmarks/trial-statistics/src/stats.rs \
    benchmarks/trial-statistics/src/parse.rs \
    benchmarks/trial-statistics/src/main.rs \
    benchmarks/trial-statistics/CONTRACT.md \
    Cargo.toml \
    Cargo.lock
) > /evidence/product-sha256.after
if ! cmp -s /evidence/product-sha256.txt /evidence/product-sha256.after; then
  echo "product bytes changed" >&2
  phase product-after 2
  exit 2
fi
phase product-after 0
echo proof_ok
