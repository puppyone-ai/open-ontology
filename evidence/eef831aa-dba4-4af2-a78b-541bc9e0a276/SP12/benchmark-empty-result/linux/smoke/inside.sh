#!/bin/bash
# Focused pre-edit tests of the three benches, then the current proof.
# The proof is the red not_found mismatch. No source overlay. No PostgreSQL.
# Origin: a267 smoke inside and a267 impl-4 prebuild.
# evidence/eef831aa-dba4-4af2-a78b-541bc9e0a267/SP12/question-bank-suite/linux/smoke/inside.sh
# sha256 0e3f411c0d327f03311d1e2387593c884cd9c64f0007ed3e848ae48d32c92a3d
# evidence/eef831aa-dba4-4af2-a78b-541bc9e0a267/SP12/question-bank-suite/linux/impl-4/inside.sh
# sha256 254c804a8f22e312e4b9fa703fceb157a83bd05e8dd1a9ac1abb79fd4bc755cd
# Delta: owner a276, three owned suites, red re-run of the compiled bank.
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

echo "=== pre-edit marker ==="
if grep -R -n 'not_found_if_complete_empty' \
  benchmarks/query-result-adapter benchmarks/sql-runner benchmarks/question-bank-suite
then
  echo "opt-in already present" >&2
  record preedit-marker.exit 2
  exit 2
fi
printf '%s\n' 'empty_result_opt_in=absent' > /evidence/preedit-marker.txt
record preedit-marker.exit 0

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
export CARGO_TARGET_DIR=/target/a276-deps
mkdir -p /target/a276-deps
cd /src
build_dep benchmarks/sql-runner/Cargo.toml sql-runner sql-runner || exit $?
build_dep benchmarks/query-result-adapter/Cargo.toml query-result-adapter query-result-adapter || exit $?
build_dep benchmarks/result-comparator/Cargo.toml result-comparator result-comparator || exit $?

echo "=== rustc query-pg test double ==="
set +e
rustc --edition 2021 --deny warnings \
  /src/benchmarks/question-bank-suite/tests/support/query_pg.rs \
  -o /target/a276-deps/query-pg \
  > /evidence/build-query-pg.stdout 2> /evidence/build-query-pg.stderr
query_pg_exit=$?
set -e
record build-query-pg.exit "$query_pg_exit"
echo "PHASE query_pg_exit=$query_pg_exit"
if [ "$query_pg_exit" -ne 0 ]; then
  exit "$query_pg_exit"
fi

export CARGO_TARGET_DIR=/target
export OO_A267_SQL_RUNNER=/target/a276-deps/debug/sql-runner
export OO_A267_ADAPTER=/target/a276-deps/debug/query-result-adapter
export OO_A267_COMPARATOR=/target/a276-deps/debug/result-comparator
export OO_A267_QUERY_PG=/target/a276-deps/query-pg
for bin in "$OO_A267_SQL_RUNNER" "$OO_A267_ADAPTER" "$OO_A267_COMPARATOR" "$OO_A267_QUERY_PG"; do
  if [ ! -x "$bin" ]; then
    echo "missing prebuild $bin" >&2
    exit 2
  fi
done

cd /src/benchmarks/question-bank-suite
echo "=== cargo test proof (expects the current not_found mismatch) ==="
set +e
cargo test --locked --offline --test proof -- --test-threads=1 \
  > /evidence/test-proof.stdout 2> /evidence/test-proof.stderr
proof_exit=$?
set -e
record test-proof.exit "$proof_exit"
echo "PHASE test_proof_exit=$proof_exit"
if [ "$proof_exit" -ne 0 ]; then
  exit "$proof_exit"
fi

echo "=== raw red re-run of the compiled bank ==="
suite_json=$(find /tmp -path '/tmp/oo-a267-*-proof/suite/suite.json' | head -1)
if [ -z "$suite_json" ]; then
  echo "compiled suite was not left by the proof" >&2
  record red-runner.exit 2
  exit 2
fi
suite_dir=$(dirname "$suite_json")
mkdir -p /evidence/red
cp "$suite_dir/questions/pub-absent/gold.json" /evidence/red/gold.json
cp "$suite_dir/questions/pub-absent/binding.json" /evidence/red/binding.json
set +e
"$OO_A267_SQL_RUNNER" --suite "$suite_json" --run-dir /evidence/red/run \
  > /evidence/red/runner.stdout 2> /evidence/red/runner.stderr
red_exit=$?
set -e
record red-runner.exit "$red_exit"
echo "PHASE red_runner_exit=$red_exit"
if [ -f /evidence/red/run/case-pub-absent/trial-0/adapter.stdout ]; then
  cp /evidence/red/run/case-pub-absent/trial-0/adapter.stdout /evidence/red/adapter.stdout
fi
if [ -f /evidence/red/run/case-pub-absent/trial-0/comparator.stdout ]; then
  cp /evidence/red/run/case-pub-absent/trial-0/comparator.stdout /evidence/red/comparator.stdout
fi
if [ -f /evidence/red/run/report.json ]; then
  cp /evidence/red/run/report.json /evidence/red/report.json
fi
if [ "$red_exit" -ne 1 ]; then
  echo "red runner exit was $red_exit, expected 1" >&2
  exit 2
fi
exit 0
