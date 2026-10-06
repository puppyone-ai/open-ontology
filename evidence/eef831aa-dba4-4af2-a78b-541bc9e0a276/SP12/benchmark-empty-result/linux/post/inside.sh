#!/bin/bash
# Fmt, focused tests, clippy, and the green 5-question pipeline.
# The raw runner is expected to exit 1: not_found passes, reannotation remains, the gate stays failed.
# Origin: a276 smoke inside.sh in this session.
# evidence/eef831aa-dba4-4af2-a78b-541bc9e0a276/SP12/benchmark-empty-result/linux/smoke/inside.sh
# Delta: opt-in must be present, rustfmt and clippy, green counts instead of the red mismatch.
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

echo "=== opt-in marker ==="
if ! grep -R -n 'not_found_if_complete_empty' \
  benchmarks/query-result-adapter benchmarks/sql-runner benchmarks/question-bank-suite \
  > /evidence/opt-in-marker.txt
then
  echo "opt-in missing from overlay" >&2
  record opt-in-marker.exit 2
  exit 2
fi
record opt-in-marker.exit 0

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

echo "=== rustup components ==="
set +e
rustup component add rustfmt clippy --toolchain 1.94.0 > /evidence/component.stdout 2> /evidence/component.stderr
component_exit=$?
set -e
record component.exit "$component_exit"
echo "PHASE component_exit=$component_exit"
if [ "$component_exit" -ne 0 ]; then
  exit "$component_exit"
fi

fmt_one() {
  local manifest="$1"
  local name="$2"
  echo "=== cargo fmt $name ==="
  set +e
  cargo fmt --manifest-path "$manifest" --all > "/evidence/fmt-${name}-apply.stdout" 2> "/evidence/fmt-${name}-apply.stderr"
  local apply=$?
  set -e
  record "fmt-${name}-apply.exit" "$apply"
  echo "PHASE fmt_${name}_apply_exit=$apply"
  if [ "$apply" -ne 0 ]; then
    return "$apply"
  fi
  set +e
  cargo fmt --manifest-path "$manifest" --all -- --check > "/evidence/fmt-${name}.stdout" 2> "/evidence/fmt-${name}.stderr"
  local check=$?
  set -e
  record "fmt-${name}.exit" "$check"
  echo "PHASE fmt_${name}_exit=$check"
  if [ "$check" -ne 0 ]; then
    return "$check"
  fi
}

fmt_one benchmarks/query-result-adapter/Cargo.toml adapter || exit $?
fmt_one benchmarks/sql-runner/Cargo.toml sql-runner || exit $?
fmt_one benchmarks/question-bank-suite/Cargo.toml suite || exit $?
set +e
rustfmt --edition 2021 --check benchmarks/question-bank-suite/tests/support/query_pg.rs \
  > /evidence/stub-fmt.stdout 2> /evidence/stub-fmt.stderr
stub_fmt=$?
set -e
record stub-fmt.exit "$stub_fmt"
echo "PHASE stub_fmt_exit=$stub_fmt"
if [ "$stub_fmt" -ne 0 ]; then
  exit "$stub_fmt"
fi

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
sha256sum "$OO_A267_SQL_RUNNER" "$OO_A267_ADAPTER" "$OO_A267_COMPARATOR" "$OO_A267_QUERY_PG" \
  > /evidence/bin-sha256.txt

cd /src/benchmarks/question-bank-suite
echo "=== cargo test proof ==="
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

echo "=== raw green re-run of the compiled bank ==="
suite_json=$(find /tmp -path '/tmp/oo-a267-*-proof/suite/suite.json' | head -1)
if [ -z "$suite_json" ]; then
  echo "compiled suite was not left by the proof" >&2
  record green-runner.exit 2
  exit 2
fi
suite_dir=$(dirname "$suite_json")
mkdir -p /evidence/green
cp "$suite_dir/questions/pub-absent/gold.json" /evidence/green/gold.json
cp "$suite_dir/questions/pub-absent/binding.json" /evidence/green/binding.json
cp "$suite_dir/questions/pub-table/binding.json" /evidence/green/table-binding.json
set +e
"$OO_A267_SQL_RUNNER" --suite "$suite_json" --run-dir /evidence/green/run \
  > /evidence/green/runner.stdout 2> /evidence/green/runner.stderr
green_exit=$?
set -e
record green-runner.exit "$green_exit"
echo "PHASE green_runner_exit=$green_exit"
if [ -f /evidence/green/run/case-pub-absent/trial-0/adapter.stdout ]; then
  cp /evidence/green/run/case-pub-absent/trial-0/adapter.stdout /evidence/green/adapter.stdout
fi
if [ -f /evidence/green/run/case-pub-absent/trial-0/comparator.stdout ]; then
  cp /evidence/green/run/case-pub-absent/trial-0/comparator.stdout /evidence/green/comparator.stdout
fi
if [ -f /evidence/green/run/case-pub-table/trial-0/adapter.stdout ]; then
  cp /evidence/green/run/case-pub-table/trial-0/adapter.stdout /evidence/green/table-adapter.stdout
fi
if [ -f /evidence/green/run/report.json ]; then
  cp /evidence/green/run/report.json /evidence/green/report.json
fi
if [ "$green_exit" -ne 1 ]; then
  echo "green runner exit was $green_exit, expected 1" >&2
  exit 2
fi
report=/evidence/green/report.json
for needle in '"question_count":5' '"trial_count":10' '"pass_count":8' '"fail_count":0' '"reannotation_count":2' '"passed":false'; do
  if ! grep -F -q "$needle" "$report"; then
    echo "report missing $needle" >&2
    exit 2
  fi
done
if grep -F -q 'pw-a267-secret' "$report"; then
  echo "report contains the fixture password" >&2
  exit 2
fi
printf '%s\n' '{"kind":"not_found","completion":"complete","warnings":[]}' > /evidence/green/adapter.expected
if ! cmp -s /evidence/green/adapter.stdout /evidence/green/adapter.expected; then
  echo "absent adapter stdout is not the not_found object" >&2
  exit 2
fi
if ! grep -F -q '"empty_result":"not_found_if_complete_empty"' /evidence/green/binding.json; then
  echo "absent binding omitted the opt-in" >&2
  exit 2
fi
if grep -F -q 'empty_result' /evidence/green/table-binding.json; then
  echo "table binding gained empty_result" >&2
  exit 2
fi
if ! grep -F -q '"kind":"not_found"' /evidence/green/gold.json; then
  echo "gold lost not_found" >&2
  exit 2
fi
if ! grep -F -q '"coefficient":"120"' /evidence/green/table-adapter.stdout; then
  echo "table adapter lost coefficient 120" >&2
  exit 2
fi
if grep -F -q '1.20' /evidence/green/table-adapter.stdout; then
  echo "table adapter contains 1.20" >&2
  exit 2
fi

cd /src
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
  if [ "$code" -ne 0 ]; then
    return "$code"
  fi
}
clippy_one benchmarks/query-result-adapter/Cargo.toml adapter || exit $?
clippy_one benchmarks/sql-runner/Cargo.toml sql-runner || exit $?
clippy_one benchmarks/question-bank-suite/Cargo.toml suite || exit $?
echo "PHASE post_ok"
exit 0
