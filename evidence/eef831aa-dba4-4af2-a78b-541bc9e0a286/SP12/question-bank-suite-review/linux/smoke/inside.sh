#!/bin/bash
# MSRV focused tests of the three benches, then clippy, old-byte boundary,
# the frozen author proof, and an independent fresh hand bank.
# The archive at /src is c551afea. The archive at /old is f57e430f.
# No product overlay. No PostgreSQL.
set -u
export PATH=/usr/local/cargo/bin:/usr/local/bin:/usr/bin:/bin
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
cd /src

record() {
  printf '%s\n' "$2" > "/evidence/$1"
}

status=0
keep() {
  if [ "$status" -eq 0 ] && [ "$1" -ne 0 ]; then
    status=$1
  fi
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
if [ -f rust-toolchain.toml ] || [ -f /old/rust-toolchain.toml ]; then
  echo "repo toolchain file still active" >&2
  record toolchain-absent.exit 2
  exit 2
fi
record toolchain-absent.exit 0
if [ ! -f rust-toolchain.toml.repo ] || [ ! -f /old/rust-toolchain.toml.repo ]; then
  echo "repo toolchain file missing from an archive copy" >&2
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

echo "=== seam marker ==="
if ! grep -q 'not_found_if_complete_empty' benchmarks/query-result-adapter/src/parse.rs; then
  echo "fixed adapter is missing the opt-in" >&2
  record seam-marker.exit 2
  exit 2
fi
if grep -q 'not_found_if_complete_empty' /old/benchmarks/query-result-adapter/src/parse.rs; then
  echo "parent adapter already has the opt-in" >&2
  record seam-marker.exit 2
  exit 2
fi
printf '%s\n' 'fixed=present parent=absent' > /evidence/seam-marker.txt
record seam-marker.exit 0

echo "=== pins ==="
sha256sum \
  benchmarks/query-result-adapter/src/lib.rs \
  benchmarks/query-result-adapter/src/parse.rs \
  benchmarks/query-result-adapter/Cargo.toml \
  benchmarks/query-result-adapter/Cargo.lock \
  benchmarks/sql-runner/src/lib.rs \
  benchmarks/sql-runner/src/parse.rs \
  benchmarks/sql-runner/Cargo.toml \
  benchmarks/sql-runner/Cargo.lock \
  benchmarks/question-bank-suite/src/lib.rs \
  benchmarks/question-bank-suite/src/compile.rs \
  benchmarks/question-bank-suite/CONTRACT.md \
  benchmarks/question-bank-suite/Cargo.toml \
  benchmarks/question-bank-suite/Cargo.lock \
  > /evidence/archive-sha256.txt
pin_lock() {
  local file="$1"
  local tag="$2"
  awk -v tag="$tag" '
    $0=="name = \"serde\"" {p=1}
    $0=="name = \"serde_json\"" {p=2}
    p==1 && $0 ~ /^version = / {print tag, "serde", $0}
    p==1 && $0 ~ /^checksum = / {print tag, "serde", $0; p=0}
    p==2 && $0 ~ /^version = / {print tag, "serde_json", $0}
    p==2 && $0 ~ /^checksum = / {print tag, "serde_json", $0; p=0}
  ' "$file"
}
pin_lock benchmarks/query-result-adapter/Cargo.lock adapter > /evidence/lock-pins.txt
pin_lock benchmarks/sql-runner/Cargo.lock sql-runner >> /evidence/lock-pins.txt
pin_lock benchmarks/question-bank-suite/Cargo.lock suite >> /evidence/lock-pins.txt
if ! grep -q 'adapter serde version = "1.0.229"' /evidence/lock-pins.txt \
  || ! grep -q 'adapter serde checksum = "4148590afebada386688f18773da617792bf2ef03ffc1e4cbd2b1d45b023e0ba"' /evidence/lock-pins.txt \
  || ! grep -q 'adapter serde_json version = "1.0.151"' /evidence/lock-pins.txt \
  || ! grep -q 'adapter serde_json checksum = "c841b55ecdae098c80dcae9cf767f6f8a0c2cdb3416bbef72181df4d0fe73f14"' /evidence/lock-pins.txt \
  || ! grep -q 'sql-runner serde version = "1.0.229"' /evidence/lock-pins.txt \
  || ! grep -q 'suite serde_json version = "1.0.151"' /evidence/lock-pins.txt
then
  echo "lock pins drifted" >&2
  record lock-pins.exit 2
  exit 2
fi
record lock-pins.exit 0

fetch_one() {
  local manifest="$1"
  local name="$2"
  echo "=== cargo fetch $name locked offline ==="
  set +e
  cargo fetch --manifest-path "$manifest" --locked --offline > "/evidence/fetch-${name}-offline.stdout" 2> "/evidence/fetch-${name}-offline.stderr"
  local offline_exit=$?
  set -e
  record "fetch-${name}-offline.exit" "$offline_exit"
  echo "PHASE ${name}_offline_fetch_exit=$offline_exit"
  local fetch_exit=$offline_exit
  if [ "$offline_exit" -ne 0 ]; then
    echo "=== cargo fetch $name locked online ==="
    set +e
    cargo fetch --manifest-path "$manifest" --locked > "/evidence/fetch-${name}.stdout" 2> "/evidence/fetch-${name}.stderr"
    fetch_exit=$?
    set -e
  else
    cp "/evidence/fetch-${name}-offline.stdout" "/evidence/fetch-${name}.stdout"
    cp "/evidence/fetch-${name}-offline.stderr" "/evidence/fetch-${name}.stderr"
  fi
  record "fetch-${name}.exit" "$fetch_exit"
  echo "PHASE ${name}_fetch_exit=$fetch_exit"
  if [ "$fetch_exit" -ne 0 ]; then
    return "$fetch_exit"
  fi
}

fetch_one benchmarks/query-result-adapter/Cargo.toml adapter || exit $?
fetch_one benchmarks/sql-runner/Cargo.toml sql-runner || exit $?
fetch_one benchmarks/question-bank-suite/Cargo.toml suite || exit $?
fetch_one benchmarks/result-comparator/Cargo.toml comparator || exit $?
fetch_one benchmarks/question-bank/Cargo.toml question-bank || exit $?

run_tests() {
  local name="$1"
  shift
  echo "=== cargo test $name ==="
  set +e
  cargo test "$@" > "/evidence/test-${name}.stdout" 2> "/evidence/test-${name}.stderr"
  local code=$?
  set -e
  record "test-${name}.exit" "$code"
  echo "PHASE test_${name}_exit=$code"
  if [ "$code" -ne 0 ]; then
    return "$code"
  fi
}

run_tests adapter --manifest-path benchmarks/query-result-adapter/Cargo.toml --locked --offline -- --test-threads=1 || exit $?
run_tests sql-runner --manifest-path benchmarks/sql-runner/Cargo.toml --locked --offline -- --test-threads=1 || exit $?
run_tests suite-focused --manifest-path benchmarks/question-bank-suite/Cargo.toml --locked --offline --test cli --test compile --test sha256 -- --test-threads=1 || exit $?

echo "=== clippy ==="
set +e
cargo clippy --version > /evidence/clippy-version.stdout 2> /evidence/clippy-version.stderr
clippy_ver=$?
set -e
record clippy-version.exit "$clippy_ver"
if [ "$clippy_ver" -ne 0 ]; then
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/component.stdout 2> /evidence/component.stderr
  component_exit=$?
  set -e
  record component.exit "$component_exit"
  echo "PHASE component_exit=$component_exit"
  if [ "$component_exit" -ne 0 ]; then
    keep "$component_exit"
  fi
fi

clippy_one() {
  local manifest="$1"
  local name="$2"
  echo "=== cargo clippy $name ==="
  set +e
  cargo clippy --manifest-path "$manifest" --locked --offline --all-targets -- -D warnings \
    > "/evidence/clippy-${name}.stdout" 2> "/evidence/clippy-${name}.stderr"
  local code=$?
  set -e
  record "clippy-${name}.exit" "$code"
  echo "PHASE clippy_${name}_exit=$code"
  keep "$code"
}

clippy_one benchmarks/query-result-adapter/Cargo.toml adapter
clippy_one benchmarks/sql-runner/Cargo.toml sql-runner
clippy_one benchmarks/question-bank-suite/Cargo.toml suite

build_dep() {
  local manifest="$1"
  local name="$2"
  local bin="$3"
  local target_dir="$4"
  echo "=== cargo build $name offline ==="
  set +e
  CARGO_TARGET_DIR="$target_dir" cargo build --manifest-path "$manifest" --locked --offline --bin "$bin" \
    > "/evidence/build-${name}-offline.stdout" 2> "/evidence/build-${name}-offline.stderr"
  local offline=$?
  set -e
  record "build-${name}-offline.exit" "$offline"
  echo "PHASE ${name}_offline_build_exit=$offline"
  local code=$offline
  if [ "$offline" -ne 0 ]; then
    echo "=== cargo build $name online ==="
    set +e
    CARGO_TARGET_DIR="$target_dir" cargo build --manifest-path "$manifest" --locked --bin "$bin" \
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

echo "=== build integrated binaries ==="
mkdir -p /target/a286-deps /target/a286-old /target/a286-proof /target/a286-bin /target/a286-oracle
build_dep benchmarks/sql-runner/Cargo.toml sql-runner sql-runner /target/a286-deps || exit $?
build_dep benchmarks/query-result-adapter/Cargo.toml query-result-adapter query-result-adapter /target/a286-deps || exit $?
build_dep benchmarks/result-comparator/Cargo.toml result-comparator result-comparator /target/a286-deps || exit $?
build_dep benchmarks/question-bank-suite/Cargo.toml question-bank-suite question-bank-suite /target/a286-deps || exit $?

echo "=== build pre-seam adapter ==="
build_dep /old/benchmarks/query-result-adapter/Cargo.toml old-adapter query-result-adapter /target/a286-old || exit $?

NEW=/target/a286-deps/debug/query-result-adapter
OLD_BIN=/target/a286-old/debug/query-result-adapter
RUNNER=/target/a286-deps/debug/sql-runner
COMPARATOR=/target/a286-deps/debug/result-comparator
SUITE_BIN=/target/a286-deps/debug/question-bank-suite
for bin in "$NEW" "$OLD_BIN" "$RUNNER" "$COMPARATOR" "$SUITE_BIN"; do
  if [ ! -x "$bin" ]; then
    echo "missing binary $bin" >&2
    record bins.exit 2
    exit 2
  fi
done
sha256sum "$NEW" "$OLD_BIN" "$RUNNER" "$COMPARATOR" "$SUITE_BIN" > /evidence/bin-sha256.txt
record bins.exit 0

echo "=== lock subset ==="
set +e
rustc --edition 2021 -D warnings /tools/lock_subset.rs -o /target/a286-bin/lock_subset \
  > /evidence/build-lock-subset.stdout 2> /evidence/build-lock-subset.stderr
lock_build=$?
set -e
record build-lock-subset.exit "$lock_build"
echo "PHASE lock_subset_build_exit=$lock_build"
if [ "$lock_build" -ne 0 ]; then
  keep "$lock_build"
else
  set +e
  /target/a286-bin/lock_subset \
    /src/Cargo.lock \
    /src/benchmarks/query-result-adapter/Cargo.lock \
    /src/benchmarks/sql-runner/Cargo.lock \
    /src/benchmarks/question-bank-suite/Cargo.lock \
    /src/benchmarks/question-bank/Cargo.lock \
    /src/benchmarks/result-comparator/Cargo.lock \
    > /evidence/lock-subset.stdout 2> /evidence/lock-subset.stderr
  lock_exit=$?
  set -e
  record lock-subset.exit "$lock_exit"
  echo "PHASE lock_subset_exit=$lock_exit"
  keep "$lock_exit"
fi

echo "=== lock bytes versus parent ==="
set +e
cmp /src/benchmarks/query-result-adapter/Cargo.lock /old/benchmarks/query-result-adapter/Cargo.lock \
  > /evidence/cmp-adapter-lock.stdout 2> /evidence/cmp-adapter-lock.stderr
cmp_adapter=$?
cmp /src/benchmarks/sql-runner/Cargo.lock /old/benchmarks/sql-runner/Cargo.lock \
  > /evidence/cmp-runner-lock.stdout 2> /evidence/cmp-runner-lock.stderr
cmp_runner=$?
cmp /src/benchmarks/question-bank-suite/Cargo.lock /old/benchmarks/question-bank-suite/Cargo.lock \
  > /evidence/cmp-suite-lock.stdout 2> /evidence/cmp-suite-lock.stderr
cmp_suite=$?
set -e
record cmp-adapter-lock.exit "$cmp_adapter"
record cmp-runner-lock.exit "$cmp_runner"
record cmp-suite-lock.exit "$cmp_suite"
echo "PHASE cmp_adapter=$cmp_adapter cmp_runner=$cmp_runner cmp_suite=$cmp_suite"
keep "$cmp_adapter"
keep "$cmp_runner"
keep "$cmp_suite"

echo "=== boundary ==="
set +e
rustc --edition 2021 -D warnings /oracle/boundary.rs -o /target/a286-bin/boundary \
  > /evidence/build-boundary.stdout 2> /evidence/build-boundary.stderr
boundary_build=$?
set -e
record build-boundary.exit "$boundary_build"
echo "PHASE boundary_build_exit=$boundary_build"
if [ "$boundary_build" -ne 0 ]; then
  keep "$boundary_build"
else
  set +e
  OO_A286_NEW_ADAPTER="$NEW" OO_A286_OLD_ADAPTER="$OLD_BIN" /target/a286-bin/boundary \
    > /evidence/boundary.stdout 2> /evidence/boundary.stderr
  boundary_exit=$?
  set -e
  record boundary.exit "$boundary_exit"
  echo "PHASE boundary_exit=$boundary_exit"
  keep "$boundary_exit"
fi

echo "=== author proof double ==="
set +e
rustc --edition 2021 --deny warnings \
  /src/benchmarks/question-bank-suite/tests/support/query_pg.rs \
  -o /target/a286-proof/query-pg \
  > /evidence/build-author-query-pg.stdout 2> /evidence/build-author-query-pg.stderr
author_double=$?
set -e
record build-author-query-pg.exit "$author_double"
echo "PHASE author_query_pg_exit=$author_double"
if [ "$author_double" -ne 0 ]; then
  keep "$author_double"
else
  export CARGO_TARGET_DIR=/target
  export OO_A267_SQL_RUNNER="$RUNNER"
  export OO_A267_ADAPTER="$NEW"
  export OO_A267_COMPARATOR="$COMPARATOR"
  export OO_A267_QUERY_PG=/target/a286-proof/query-pg
  echo "=== cargo test proof ==="
  set +e
  cargo test --manifest-path benchmarks/question-bank-suite/Cargo.toml --locked --offline --test proof -- --test-threads=1 \
    > /evidence/test-proof.stdout 2> /evidence/test-proof.stderr
  proof_exit=$?
  set -e
  record test-proof.exit "$proof_exit"
  echo "PHASE test_proof_exit=$proof_exit"
  keep "$proof_exit"
fi

echo "=== fresh query double ==="
set +e
rustc --edition 2021 -D warnings /oracle/query_pg.rs -o /target/a286-bin/query-pg \
  > /evidence/build-fresh-query-pg.stdout 2> /evidence/build-fresh-query-pg.stderr
fresh_double=$?
set -e
record build-fresh-query-pg.exit "$fresh_double"
echo "PHASE fresh_query_pg_exit=$fresh_double"
if [ "$fresh_double" -ne 0 ]; then
  keep "$fresh_double"
else
  echo "=== fresh bank lock ==="
  set +e
  CARGO_TARGET_DIR=/target/a286-oracle cargo generate-lockfile --manifest-path /oracle/Cargo.toml --offline \
    > /evidence/fresh-lock-offline.stdout 2> /evidence/fresh-lock-offline.stderr
  fresh_lock=$?
  set -e
  record fresh-lock-offline.exit "$fresh_lock"
  echo "PHASE fresh_lock_offline_exit=$fresh_lock"
  if [ "$fresh_lock" -ne 0 ]; then
    set +e
    CARGO_TARGET_DIR=/target/a286-oracle cargo generate-lockfile --manifest-path /oracle/Cargo.toml \
      > /evidence/fresh-lock.stdout 2> /evidence/fresh-lock.stderr
    fresh_lock=$?
    set -e
    record fresh-lock.exit "$fresh_lock"
    echo "PHASE fresh_lock_online_exit=$fresh_lock"
  else
    cp /evidence/fresh-lock-offline.stdout /evidence/fresh-lock.stdout
    cp /evidence/fresh-lock-offline.stderr /evidence/fresh-lock.stderr
    record fresh-lock.exit 0
  fi
  if [ "$fresh_lock" -ne 0 ]; then
    keep "$fresh_lock"
  else
    echo "=== fresh bank ==="
    set +e
    CARGO_TARGET_DIR=/target/a286-oracle \
      OO_A286_SUITE="$SUITE_BIN" \
      OO_A286_SQL_RUNNER="$RUNNER" \
      OO_A286_ADAPTER="$NEW" \
      OO_A286_COMPARATOR="$COMPARATOR" \
      OO_A286_QUERY_PG=/target/a286-bin/query-pg \
      OO_A286_FRESH_DIR=/evidence/fresh \
      cargo run --manifest-path /oracle/Cargo.toml --locked --offline --bin a286-fresh-bank \
      > /evidence/fresh-bank.stdout 2> /evidence/fresh-bank.stderr
    fresh_exit=$?
    set -e
    record fresh-bank.exit "$fresh_exit"
    echo "PHASE fresh_bank_exit=$fresh_exit"
    keep "$fresh_exit"
  fi
fi

echo "=== bench tree hashes ==="
set +e
rustc --edition 2021 -D warnings /tools/sha256_manifest.rs -o /target/a286-bin/sha256_manifest \
  > /evidence/build-sha256-manifest.stdout 2> /evidence/build-sha256-manifest.stderr
manifest_build=$?
set -e
record build-sha256-manifest.exit "$manifest_build"
if [ "$manifest_build" -eq 0 ]; then
  mkdir -p /evidence/source
  set +e
  /target/a286-bin/sha256_manifest /src/benchmarks/query-result-adapter /evidence/source/adapter.sha256 \
    > /evidence/hash-adapter.stdout 2> /evidence/hash-adapter.stderr
  hash_adapter=$?
  /target/a286-bin/sha256_manifest /src/benchmarks/sql-runner /evidence/source/runner.sha256 \
    > /evidence/hash-runner.stdout 2> /evidence/hash-runner.stderr
  hash_runner=$?
  /target/a286-bin/sha256_manifest /src/benchmarks/question-bank-suite /evidence/source/suite.sha256 \
    > /evidence/hash-suite.stdout 2> /evidence/hash-suite.stderr
  hash_suite=$?
  set -e
  record hash-adapter.exit "$hash_adapter"
  record hash-runner.exit "$hash_runner"
  record hash-suite.exit "$hash_suite"
  if [ "$hash_adapter" -ne 0 ] || [ "$hash_runner" -ne 0 ] || [ "$hash_suite" -ne 0 ]; then
    record bench-tree-hash.exit 1
    keep 1
  else
    record bench-tree-hash.exit 0
  fi
else
  record bench-tree-hash.exit "$manifest_build"
  keep "$manifest_build"
fi

echo "PHASE smoke_status=$status"
exit "$status"
