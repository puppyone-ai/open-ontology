#!/bin/bash
# Linux arm64 Rust 1.94 compile of the fixed source-conformance archive,
# then independent CLI probes. One cargo, one target dir, no database.
# Recipe origin: sealed author linux/repair/inside.sh, adapted to this prefix.
set -u
echo "oo_task=DL01-W1-37-coordinator-fixed"
echo "oo_uuid=coordinator-nightly-fixed"
echo "oo_platform=linux/arm64"
export RUSTUP_TOOLCHAIN=1.94.0
export RUSTUP_HOME=/usr/local/rustup
export CARGO_HOME=/cargo
export CARGO_TARGET_DIR=/target/suite
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1
export HOME=/tmp
export TMPDIR=/target/tmp
EV=/evidence
FIX=/harness/fixtures
BIN=/target/suite/debug/source-conformance
mkdir -p "$EV/phases" "$EV/probes" /target/tmp
cd /src/tools/source-conformance

record() {
  printf '%s\n' "$2" > "$EV/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
}

got=$(rustc --version)
printf '%s\n' "$got" | tee "$EV/rustc-version.txt"
rustc --version --verbose > "$EV/rustc-verbose.txt"
cargo --version > "$EV/cargo-version.txt"
command -v rustc > "$EV/rustc-path.txt"
case "$got" in
  "rustc 1.94.0 "*) record toolchain 0 ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2; printf '%s\n' 2 > "$EV/smoke.exit"; exit 2 ;;
esac
if ! rustc -vV | grep -q '^host: aarch64-unknown-linux-gnu$'; then
  echo "host is not linux arm64" >&2
  record platform 2
  printf '%s\n' 2 > "$EV/smoke.exit"
  exit 2
fi
record platform 0
if [ -e /src/tools/source-support ] || [ -e /src/tools/source-semantics ]; then
  echo "source-support or source-semantics present in compile tree; refusing" >&2
  record source-support-absent 2
  printf '%s\n' 2 > "$EV/smoke.exit"
  exit 2
fi
printf '%s\n' "absent-from-archive" > "$EV/source-support.txt"
record source-support-absent 0
if [ "$(pwd)" != "/src/tools/source-conformance" ]; then
  echo "refusing to compile outside the fixed archive" >&2
  record overlay-pwd 2
  printf '%s\n' 2 > "$EV/smoke.exit"
  exit 2
fi
record overlay-pwd 0
printf '%s\n' "no" > "$EV/cargo-update-invoked.txt"
sha256sum /src/docs/database-support/support-matrix.yaml > "$EV/matrix-archive.sha256"
sha256sum /src/evidence/gpt-minimal-closure/REPORT.md > "$EV/historical-report.sha256"
sha256sum /src/Cargo.toml /src/Cargo.lock /src/crates/oo-core/Cargo.toml /src/crates/oo-builder/Cargo.toml /src/contracts/schema/oos-v1.json > "$EV/base-files.sha256"
sha256sum /src/tools/source-conformance/src/identity.rs > "$EV/identity-rs.sha256"

echo "=== cargo fetch --locked ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked > "$EV/phases/fetch.stdout" 2>"$EV/phases/fetch.stderr"
fetch_exit=$?
set -e
record fetch "$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  printf '%s\n' "$fetch_exit" > "$EV/smoke.exit"
  exit "$fetch_exit"
fi

echo "=== cargo build --locked --offline --bin source-conformance ==="
set +e
cargo build --manifest-path Cargo.toml --locked --offline --bin source-conformance \
  > "$EV/phases/build.stdout" 2>"$EV/phases/build.stderr"
build_exit=$?
set -e
record build "$build_exit"

echo "=== cargo test --locked --offline ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -- --test-threads=1 \
  > "$EV/phases/test.stdout" 2>"$EV/phases/test.stderr"
test_exit=$?
set -e
record test "$test_exit"
sum_field() {
  grep '^test result:' "$EV/phases/test.stdout" \
    | sed -n "s/.* \\([0-9][0-9]*\\) $1.*/\\1/p" \
    | awk '{s+=$1} END{print s+0}'
}
printf 'passed=%s failed=%s ignored=%s\n' "$(sum_field passed)" "$(sum_field failed)" "$(sum_field ignored)" \
  > "$EV/test-count.txt"

echo "=== clippy ==="
set +e
cargo clippy --version > "$EV/clippy-version.txt" 2>"$EV/phases/clippy-version.stderr"
clippy_ver=$?
set -e
if [ "$clippy_ver" -ne 0 ]; then
  set +e
  rustup component add clippy --toolchain 1.94.0 > "$EV/phases/clippy-component.stdout" 2>"$EV/phases/clippy-component.stderr"
  component_exit=$?
  set -e
  record clippy-component "$component_exit"
  set +e
  cargo clippy --version > "$EV/clippy-version.txt" 2>"$EV/phases/clippy-version.stderr"
  set -e
fi
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline --all-targets --no-deps -- -D warnings \
  > "$EV/phases/clippy.stdout" 2>"$EV/phases/clippy.stderr"
clippy_exit=$?
set -e
record clippy "$clippy_exit"

if [ -x "$BIN" ]; then
  sha256sum "$BIN" > "$EV/binary.sha256"
  wc -c < "$BIN" > "$EV/binary.size"
else
  printf '%s\n' "binary missing" > "$EV/binary.sha256"
fi

echo "=== independent probes ==="
J=$EV/probes
run_one() {
  name=$1
  shift
  printf '%s\n' "$BIN $*" > "$J/$name.argv"
  if [ ! -x "$BIN" ]; then
    printf '%s\n' "binary missing" > "$J/$name.stderr"
    printf '%s\n' 127 > "$J/$name.exit"
    echo "probe=$name exit=127"
    return
  fi
  set +e
  "$BIN" "$@" > "$J/$name.stdout" 2>"$J/$name.stderr"
  code=$?
  set -e
  printf '%s\n' "$code" > "$J/$name.exit"
  echo "probe=$name exit=$code"
}
run_one map map
run_one usage-bare
run_one usage-check check
run_one schema-matrix check --matrix "$FIX/schema-matrix/matrix.yaml"
run_one schema-package check --matrix "$FIX/schema-package/matrix.yaml" --package "$FIX/schema-package/package"
run_one hash-mismatch check --matrix "$FIX/hash-mismatch/matrix.yaml" --package "$FIX/hash-mismatch/package"
run_one hash-case check --matrix "$FIX/hash-case/matrix.yaml" --package "$FIX/hash-case/package"
run_one target-mismatch check --matrix "$FIX/target-mismatch/matrix.yaml" --package "$FIX/target-mismatch/package"
run_one positive check --matrix "$FIX/positive/matrix.yaml" --package "$FIX/positive/package"
run_one designed-bare check --matrix "$FIX/designed-bare/matrix.yaml"
run_one streak14 check --matrix "$FIX/streak14/matrix.yaml" --nightly "$FIX/streak14/nightly.json"
run_one missed-day check --matrix "$FIX/missed-day/matrix.yaml" --nightly "$FIX/missed-day/nightly.json"
run_one downgrade check --matrix "$FIX/downgrade/matrix.yaml" --package "$FIX/downgrade/package" --nightly "$FIX/downgrade/nightly.json"
run_one manual-only check --matrix "$FIX/manual-only/matrix.yaml" --nightly "$FIX/manual-only/nightly.json"
run_one cloud-container check --matrix "$FIX/cloud-container/matrix.yaml" --package "$FIX/cloud-container/package"
run_one silent-fallback check --matrix "$FIX/silent-fallback/matrix.yaml" --package "$FIX/silent-fallback/package"
run_one drift check --matrix "$FIX/drift/matrix.yaml" --package "$FIX/drift/package"
run_one drift-nightly check --matrix "$FIX/drift/matrix.yaml" --package "$FIX/drift/package" --nightly "$FIX/drift/nightly.json"
run_one empty check --matrix "$FIX/empty/matrix.yaml" --package "$FIX/empty/package"
run_one missing check --matrix "$FIX/missing/matrix.yaml" --package "$FIX/missing/package"
run_one missing-nightly check --matrix "$FIX/missing/matrix.yaml" --package "$FIX/missing/package" --nightly "$FIX/missing/nightly.json"
run_one missing-valid check --matrix "$FIX/missing-valid/matrix.yaml" --package "$FIX/missing-valid/package"
run_one missing-valid-nightly check --matrix "$FIX/missing-valid/matrix.yaml" --package "$FIX/missing-valid/package" --nightly "$FIX/missing-valid/nightly.json"
run_one unknown check --matrix "$FIX/unknown/matrix.yaml" --package "$FIX/unknown/package"
run_one malformed check --matrix "$FIX/malformed/matrix.yaml" --package "$FIX/malformed/package"
run_one malformed-nightly check --matrix "$FIX/malformed/matrix.yaml" --package "$FIX/malformed/package" --nightly "$FIX/malformed/nightly.json"
run_one patch check --matrix "$FIX/patch/matrix.yaml" --package "$FIX/patch/package"
run_one major check --matrix "$FIX/major/matrix.yaml" --package "$FIX/major/package"
run_one prefix check --matrix "$FIX/prefix/matrix.yaml" --package "$FIX/prefix/package"
run_one suffix-same check --matrix "$FIX/suffix-same/matrix.yaml" --package "$FIX/suffix-same/package"
run_one suffix-mismatch check --matrix "$FIX/suffix-mismatch/matrix.yaml" --package "$FIX/suffix-mismatch/package"
run_one suffix-confusion check --matrix "$FIX/suffix-confusion/matrix.yaml" --package "$FIX/suffix-confusion/package"
run_one mysql-suffix check --matrix "$FIX/mysql-suffix/matrix.yaml" --package "$FIX/mysql-suffix/package"
run_one mysql-prefix check --matrix "$FIX/mysql-prefix/matrix.yaml" --package "$FIX/mysql-prefix/package"
run_one compat check --matrix "$FIX/compat/matrix.yaml" --package "$FIX/compat/package"
run_one whitespace check --matrix "$FIX/whitespace/matrix.yaml" --package "$FIX/whitespace/package"
run_one oracle-alias check --matrix "$FIX/oracle-alias/matrix.yaml" --package "$FIX/oracle-alias/package"
run_one inherited-pg check --matrix "$FIX/inherited-pg/matrix.yaml" --package "$FIX/inherited-pg/package"
run_one inherited-mysql check --matrix "$FIX/inherited-mysql/matrix.yaml" --package "$FIX/inherited-mysql/package"
run_one driver-bad check --matrix "$FIX/driver-bad/matrix.yaml" --package "$FIX/driver-bad/package"
run_one driver-unbound check --matrix "$FIX/driver-unbound/matrix.yaml" --package "$FIX/driver-unbound/package"
run_one dup-service-last-match check --matrix "$FIX/dup-service-last-match/matrix.yaml" --package "$FIX/dup-service-last-match/package"
run_one dup-service-last-drift check --matrix "$FIX/dup-service-last-drift/matrix.yaml" --package "$FIX/dup-service-last-drift/package"
run_one dup-service-nightly check --matrix "$FIX/dup-service-last-match/matrix.yaml" --package "$FIX/dup-service-last-match/package" --nightly "$FIX/dup-service-last-match/nightly.json"
run_one dup-exact-last-match check --matrix "$FIX/dup-exact-last-match/matrix.yaml" --package "$FIX/dup-exact-last-match/package"
run_one dup-exact-last-drift check --matrix "$FIX/dup-exact-last-drift/matrix.yaml" --package "$FIX/dup-exact-last-drift/package"
run_one dup-service-same check --matrix "$FIX/dup-service-same/matrix.yaml" --package "$FIX/dup-service-same/package"
run_one dup-tested-drift check --matrix "$FIX/dup-tested-drift/matrix.yaml" --package "$FIX/dup-tested-drift/package"
run_one dup-escaped check --matrix "$FIX/dup-escaped/matrix.yaml" --package "$FIX/dup-escaped/package"
run_one dup-escaped-v check --matrix "$FIX/dup-escaped-v/matrix.yaml" --package "$FIX/dup-escaped-v/package"
run_one dup-nested-service check --matrix "$FIX/dup-nested-service/matrix.yaml" --package "$FIX/dup-nested-service/package"
run_one dup-nested-identity check --matrix "$FIX/dup-nested-identity/matrix.yaml" --package "$FIX/dup-nested-identity/package"
run_one dup-buried check --matrix "$FIX/dup-buried/matrix.yaml" --package "$FIX/dup-buried/package"
run_one current-matrix check --matrix /src/docs/database-support/support-matrix.yaml --historical-pg /src/evidence/gpt-minimal-closure
cp /src/docs/database-support/support-matrix.yaml /tmp/matrix-copy.yaml
sha256sum /tmp/matrix-copy.yaml > "$EV/matrix-copy-before.sha256"
run_one matrix-copy check --matrix /tmp/matrix-copy.yaml
sha256sum /tmp/matrix-copy.yaml > "$EV/matrix-copy-after.sha256"
cmp -s /tmp/matrix-copy.yaml /src/docs/database-support/support-matrix.yaml
printf '%s\n' "$?" > "$EV/matrix-copy-cmp.exit"

final=0
if [ "$build_exit" -ne 0 ]; then
  final=$build_exit
elif [ "$test_exit" -ne 0 ]; then
  final=$test_exit
elif [ "$clippy_exit" -ne 0 ]; then
  final=$clippy_exit
fi
printf '%s\n' "$final" > "$EV/smoke.exit"
echo "PHASE build=$build_exit test=$test_exit clippy=$clippy_exit smoke=$final"
exit "$final"
