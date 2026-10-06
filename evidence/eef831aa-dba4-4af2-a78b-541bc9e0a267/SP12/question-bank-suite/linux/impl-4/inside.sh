#!/bin/bash
# Compile the suite on the sealed 567d621e archive, then test and clippy.
# The compiler does not spawn caller code. sql-runner is a separate prebuild.
# Origin: a256 impl-5 inside, which records a225 impl-4.
# evidence/eef831aa-dba4-4af2-a78b-541bc9e0a256/SP12/question-bank/linux/impl-5/inside.sh
# Delta: owner a267, crate benchmarks/question-bank-suite, keep the fixed
# benches, prebuild sql-runner plus adapter and comparator, rustc the query-pg
# test double, cargo fmt before the tests. No PostgreSQL.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
cd /src

record() {
  printf '%s\n' "$2" > "/evidence/$1"
}

fixed_hash() {
  sha256sum \
    /src/Cargo.lock \
    /src/Cargo.toml \
    /src/benchmarks/question-bank/Cargo.toml \
    /src/benchmarks/question-bank/Cargo.lock \
    /src/benchmarks/question-bank/src/lib.rs \
    /src/benchmarks/sql-runner/Cargo.toml \
    /src/benchmarks/sql-runner/Cargo.lock \
    /src/benchmarks/sql-runner/src/lib.rs \
    /src/benchmarks/query-result-adapter/Cargo.toml \
    /src/benchmarks/query-result-adapter/Cargo.lock \
    /src/benchmarks/query-result-adapter/src/lib.rs \
    /src/benchmarks/result-comparator/Cargo.toml \
    /src/benchmarks/result-comparator/Cargo.lock \
    /src/benchmarks/result-comparator/src/lib.rs \
    /src/crates/oo-core/Cargo.toml \
    /src/crates/oo-core/src/lib.rs
}

check_fixed() {
  fixed_hash > /evidence/fixed-after.sha256
  if ! cmp -s /evidence/fixed-before.sha256 /evidence/fixed-after.sha256; then
    echo "fixed archive files changed" >&2
    record fixed-match.exit 2
    return 2
  fi
  record fixed-match.exit 0
}

snapshot() {
  mkdir -p /evidence/crate-snapshot
  cp -a /src/benchmarks/question-bank-suite/. /evidence/crate-snapshot/
  rm -rf /evidence/crate-snapshot/target
}

finish() {
  local code=$1
  snapshot
  if ! check_fixed; then
    exit 2
  fi
  exit "$code"
}

echo "=== identity ==="
rustc -vV
cargo -V
rustup show
uname -a
cat /etc/os-release
printf '%s\n' "$(rustc -vV | awk '/^release:/{print $2}')" > /evidence/rustc-release.txt
if [ "$(cat /evidence/rustc-release.txt)" != "1.94.0" ]; then
  echo "rustc is not 1.94.0" >&2
  record rustc-release.exit 2
  exit 2
fi
record rustc-release.exit 0
if [ -f rust-toolchain.toml ]; then
  echo "repo toolchain file still active" >&2
  exit 2
fi
if [ ! -f rust-toolchain.toml.repo ]; then
  echo "repo toolchain file missing from archive copy" >&2
  exit 2
fi

fixed_hash > /evidence/fixed-before.sha256

cd /src/benchmarks/question-bank-suite
if [ -f Cargo.lock ]; then
  echo "crate lock was copied in before generation" >&2
  record source-sha256.exit 2
  finish 2
fi
if ! sha256sum -c /evidence/source-sha256.txt > /evidence/source-sha256.check 2>&1; then
  echo "copied sources drifted" >&2
  record source-sha256.exit 2
  finish 2
fi
record source-sha256.exit 0

echo "=== rustup components ==="
set +e
rustup component add rustfmt clippy --toolchain 1.94.0 > /evidence/component.stdout 2> /evidence/component.stderr
component_exit=$?
set -e
record component.exit "$component_exit"
echo "PHASE component_exit=$component_exit"
if [ "$component_exit" -ne 0 ]; then
  record fmt.exit not_run
  record test.exit not_run
  record clippy.exit not_run
  finish "$component_exit"
fi

echo "=== cargo fmt apply ==="
set +e
cargo fmt > /evidence/fmt-apply.stdout 2> /evidence/fmt-apply.stderr
fmt_apply_exit=$?
set -e
record fmt-apply.exit "$fmt_apply_exit"
echo "PHASE fmt_apply_exit=$fmt_apply_exit"
if [ "$fmt_apply_exit" -ne 0 ]; then
  record fmt.exit "$fmt_apply_exit"
  record test.exit not_run
  record clippy.exit not_run
  finish "$fmt_apply_exit"
fi

echo "=== cargo fmt check ==="
set +e
cargo fmt -- --check > /evidence/fmt.stdout 2> /evidence/fmt.stderr
fmt_exit=$?
set -e
record fmt.exit "$fmt_exit"
echo "PHASE fmt_exit=$fmt_exit"
if [ "$fmt_exit" -ne 0 ]; then
  record test.exit not_run
  record clippy.exit not_run
  finish "$fmt_exit"
fi

echo "=== rustfmt query-pg test double ==="
set +e
rustfmt --edition 2021 --check tests/support/query_pg.rs > /evidence/stub-fmt.stdout 2> /evidence/stub-fmt.stderr
stub_fmt_exit=$?
set -e
if [ "$stub_fmt_exit" -ne 0 ]; then
  set +e
  rustfmt --edition 2021 tests/support/query_pg.rs > /evidence/stub-fmt-apply.stdout 2> /evidence/stub-fmt-apply.stderr
  stub_apply_exit=$?
  set -e
  record stub-fmt-apply.exit "$stub_apply_exit"
  if [ "$stub_apply_exit" -ne 0 ]; then
    record stub-fmt.exit "$stub_apply_exit"
    record test.exit not_run
    record clippy.exit not_run
    finish "$stub_apply_exit"
  fi
  set +e
  rustfmt --edition 2021 --check tests/support/query_pg.rs > /evidence/stub-fmt.stdout 2> /evidence/stub-fmt.stderr
  stub_fmt_exit=$?
  set -e
fi
record stub-fmt.exit "$stub_fmt_exit"
echo "PHASE stub_fmt_exit=$stub_fmt_exit"
if [ "$stub_fmt_exit" -ne 0 ]; then
  record test.exit not_run
  record clippy.exit not_run
  finish "$stub_fmt_exit"
fi

find . -type f ! -name Cargo.lock -print | sort | while IFS= read -r file; do
  sha256sum "$file"
done > /evidence/source-sha256-after.txt

echo "=== cargo generate-lockfile ==="
set +e
cargo generate-lockfile --offline > /evidence/lock.stdout 2> /evidence/lock.stderr
lock_exit=$?
set -e
record lock-offline.exit "$lock_exit"
echo "PHASE lock_offline_exit=$lock_exit"
if [ "$lock_exit" -ne 0 ]; then
  echo "=== cargo generate-lockfile online ==="
  set +e
  cargo generate-lockfile > /evidence/lock-online.stdout 2> /evidence/lock-online.stderr
  lock_exit=$?
  set -e
  record lock-online.exit "$lock_exit"
fi
record lock.exit "$lock_exit"
echo "PHASE lock_exit=$lock_exit"
if [ "$lock_exit" -ne 0 ]; then
  record test.exit not_run
  record clippy.exit not_run
  finish "$lock_exit"
fi

awk '
  function flush() {
    if (checksum != "") print name " " version " " checksum
    name = ""
    version = ""
    checksum = ""
  }
  /^\[\[package\]\]/ { flush(); next }
  $1 == "name" { gsub(/"/, "", $3); name = $3 }
  $1 == "version" { gsub(/"/, "", $3); version = $3 }
  $1 == "checksum" { gsub(/"/, "", $3); checksum = $3 }
  END { flush() }
' /src/Cargo.lock | sort > /tmp/root-pins.txt
awk '
  function flush() {
    if (checksum != "") print name " " version " " checksum
    name = ""
    version = ""
    checksum = ""
  }
  /^\[\[package\]\]/ { flush(); next }
  $1 == "name" { gsub(/"/, "", $3); name = $3 }
  $1 == "version" { gsub(/"/, "", $3); version = $3 }
  $1 == "checksum" { gsub(/"/, "", $3); checksum = $3 }
  END { flush() }
' Cargo.lock | sort > /tmp/crate-pins.txt
comm -13 /tmp/root-pins.txt /tmp/crate-pins.txt > /evidence/lock-drift.txt
if [ -s /evidence/lock-drift.txt ]; then
  echo "suite lock drifted from the fixed registry pins" >&2
  record lock-drift.exit 2
  record test.exit not_run
  record clippy.exit not_run
  finish 2
fi
record lock-drift.exit 0
if ! grep -q 'serde 1.0.229 4148590afebada386688f18773da617792bf2ef03ffc1e4cbd2b1d45b023e0ba' /tmp/crate-pins.txt \
  || ! grep -q 'serde_json 1.0.151 c841b55ecdae098c80dcae9cf767f6f8a0c2cdb3416bbef72181df4d0fe73f14' /tmp/crate-pins.txt
then
  echo "serde pins missing from the generated lock" >&2
  record lock-drift.exit 2
  record test.exit not_run
  record clippy.exit not_run
  finish 2
fi

build_dep() {
  local manifest="$1"
  local name="$2"
  local bin="$3"
  echo "=== cargo build $name offline ==="
  set +e
  cargo build --manifest-path "$manifest" --locked --offline --bin "$bin" \
    > "/evidence/build-${name}-offline.stdout" 2> "/evidence/build-${name}-offline.stderr"
  local offline=$?
  set -e
  record "build-${name}-offline.exit" "$offline"
  echo "PHASE ${name}_offline_build_exit=$offline"
  local code=$offline
  if [ "$offline" -ne 0 ]; then
    echo "=== cargo build $name online ==="
    set +e
    cargo build --manifest-path "$manifest" --locked --bin "$bin" \
      > "/evidence/build-${name}.stdout" 2> "/evidence/build-${name}.stderr"
    code=$?
    set -e
    record "build-${name}-online.exit" "$code"
  else
    cp "/evidence/build-${name}-offline.stdout" "/evidence/build-${name}.stdout"
    cp "/evidence/build-${name}-offline.stderr" "/evidence/build-${name}.stderr"
  fi
  record "build-${name}.exit" "$code"
  echo "PHASE ${name}_build_exit=$code"
  if [ "$code" -ne 0 ]; then
    return "$code"
  fi
}

echo "=== prebuild runner binaries ==="
export CARGO_TARGET_DIR=/target/a267-deps
mkdir -p /target/a267-deps
cd /src
build_dep benchmarks/sql-runner/Cargo.toml sql-runner sql-runner || {
  record test.exit not_run
  record clippy.exit not_run
  finish $?
}
build_dep benchmarks/query-result-adapter/Cargo.toml query-result-adapter query-result-adapter || {
  record test.exit not_run
  record clippy.exit not_run
  finish $?
}
build_dep benchmarks/result-comparator/Cargo.toml result-comparator result-comparator || {
  record test.exit not_run
  record clippy.exit not_run
  finish $?
}

echo "=== rustc query-pg test double ==="
set +e
rustc --edition 2021 --deny warnings \
  /src/benchmarks/question-bank-suite/tests/support/query_pg.rs \
  -o /target/a267-deps/query-pg \
  > /evidence/build-query-pg.stdout 2> /evidence/build-query-pg.stderr
query_pg_exit=$?
set -e
record build-query-pg.exit "$query_pg_exit"
echo "PHASE query_pg_exit=$query_pg_exit"
if [ "$query_pg_exit" -ne 0 ]; then
  record test.exit not_run
  record clippy.exit not_run
  finish "$query_pg_exit"
fi

export CARGO_TARGET_DIR=/target
export OO_A267_SQL_RUNNER=/target/a267-deps/debug/sql-runner
export OO_A267_ADAPTER=/target/a267-deps/debug/query-result-adapter
export OO_A267_COMPARATOR=/target/a267-deps/debug/result-comparator
export OO_A267_QUERY_PG=/target/a267-deps/query-pg
for bin in "$OO_A267_SQL_RUNNER" "$OO_A267_ADAPTER" "$OO_A267_COMPARATOR" "$OO_A267_QUERY_PG"; do
  if [ ! -x "$bin" ]; then
    echo "missing prebuild $bin" >&2
    record test.exit not_run
    record clippy.exit not_run
    finish 2
  fi
done

cd /src/benchmarks/question-bank-suite
echo "=== cargo test ==="
set +e
cargo test --locked --offline -- --test-threads=1 > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
set -e
record test.exit "$test_exit"
echo "PHASE test_exit=$test_exit"

echo "=== cargo clippy ==="
set +e
cargo clippy --locked --offline --all-targets -- -D warnings > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
record clippy.exit "$clippy_exit"
echo "PHASE clippy_exit=$clippy_exit"

worst=0
if [ "$test_exit" -ne 0 ]; then worst=$test_exit; fi
if [ "$clippy_exit" -ne 0 ]; then worst=$clippy_exit; fi
echo "worst_phase_exit=$worst"
finish "$worst"
