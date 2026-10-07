#!/bin/bash
# Author tests and Clippy, then the independent oracle, on the fixed archive.
# Linux arm64 Rust 1.94. The source mount is read-only. No cargo update.
# No product overlay. No PostgreSQL. Clippy install uses the bridge network.
# The lock has no registry packages, so fetch, test, and Clippy stay offline.
set -u
echo "oo_task=sp05-query-result-export-review-proof"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a331"
echo "oo_fixed=15558917e2c5be12b74dd1d64fb542b7d05a03af"
echo "oo_platform=linux/arm64"
echo "phase=proof"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
mkdir -p /evidence/linux/proof/phases /cargo-home/bin
cd /src
printf '%s\n' "not-invoked" > /evidence/linux/proof/cargo-update-invoked.txt
MANIFEST=tools/query-result-export/Cargo.toml
EV=/evidence/linux/proof
first=0

record() {
  mkdir -p "$EV/phases"
  printf '%s\n' "$2" > "$EV/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
}

remember() {
  if [ "$first" -eq 0 ] && [ "$1" -ne 0 ]; then
    first=$1
  fi
}

fail() {
  record "$1" "$2"
  printf '%s\n' "$2" > "$EV/proof.exit"
  exit "$2"
}

got=$(rustc --version)
printf '%s\n' "$got" | tee "$EV/rustc-version.txt"
rustc -Vv > "$EV/rustc-verbose.txt"
cargo -V | tee "$EV/cargo-version.txt"
rustup show > "$EV/rustup-show.txt"
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; fail toolchain 2 ;;
esac
record toolchain 0

if [ ! -x /cargo-home/bin/sha256 ]; then
  rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256
  record sha256-tool $?
else
  record sha256-tool 0
fi
export PATH="/cargo-home/bin:${PATH}"
sha=$(cat /src/.fixed-base-sha)
printf '%s\n' "$sha" > "$EV/fixed-base.txt"
if [ "$sha" != "15558917e2c5be12b74dd1d64fb542b7d05a03af" ]; then
  echo "archive sha mismatch" >&2
  fail source 2
fi
if [ ! -f "$MANIFEST" ] || [ ! -f tools/query-result-export/src/lib.rs ]; then
  echo "committed exporter is absent" >&2
  fail source 2
fi
sha256 dir "$EV/package-sha256.txt" /src/tools/query-result-export
hash_exit=$?
record package-hash "$hash_exit"
if [ "$hash_exit" -ne 0 ]; then
  fail package-hash "$hash_exit"
fi
record source 0

echo "=== rustup component add clippy ==="
set +e
rustup component add clippy --toolchain 1.94.0 > "$EV/component.stdout" 2> "$EV/component.stderr"
component_exit=$?
set +e
printf '%s\n' "$component_exit" > "$EV/component.exit"
echo "PHASE component_exit=$component_exit"
if [ "$component_exit" -ne 0 ]; then
  fail component "$component_exit"
fi
record component 0

echo "=== cargo fetch --locked --offline ==="
set +e
cargo fetch --manifest-path "$MANIFEST" --locked --offline > "$EV/fetch.stdout" 2> "$EV/fetch.stderr"
fetch_exit=$?
set +e
printf '%s\n' "$fetch_exit" > "$EV/fetch.exit"
echo "PHASE fetch_exit=$fetch_exit"
record fetch "$fetch_exit"
remember "$fetch_exit"

echo "=== cargo test --locked --offline ==="
set +e
cargo test --manifest-path "$MANIFEST" --locked --offline --no-fail-fast -- --test-threads=1 > "$EV/test.stdout" 2> "$EV/test.stderr"
test_exit=$?
set +e
printf '%s\n' "$test_exit" > "$EV/test.exit"
echo "PHASE test_exit=$test_exit"
record test "$test_exit"
remember "$test_exit"
grep -E 'test result:|running [0-9]+ test' "$EV/test.stdout" > "$EV/test-summary.txt" || true

echo "=== cargo clippy --locked --offline ==="
set +e
cargo clippy --manifest-path "$MANIFEST" --locked --offline --all-targets -- -D warnings > "$EV/clippy.stdout" 2> "$EV/clippy.stderr"
clippy_exit=$?
set +e
printf '%s\n' "$clippy_exit" > "$EV/clippy.exit"
echo "PHASE clippy_exit=$clippy_exit"
record clippy "$clippy_exit"
remember "$clippy_exit"

echo "=== cargo build --locked --offline ==="
set +e
cargo build --manifest-path "$MANIFEST" --locked --offline > "$EV/build.stdout" 2> "$EV/build.stderr"
build_exit=$?
set +e
printf '%s\n' "$build_exit" > "$EV/build.exit"
echo "PHASE build_exit=$build_exit"
record build "$build_exit"
remember "$build_exit"

BIN=/target/debug/query-result-export
if [ "$build_exit" -ne 0 ] || [ ! -x "$BIN" ]; then
  echo "exporter binary is absent" >&2
  printf '%s\n' "$first" > "$EV/proof.exit"
  exit "$first"
fi

echo "=== review oracle ==="
set +e
rustc --edition 2021 --deny warnings /evidence/oracle/review_oracle.rs -o /cargo-home/bin/review-oracle > "$EV/oracle-rustc.stdout" 2> "$EV/oracle-rustc.stderr"
oracle_rustc=$?
set +e
printf '%s\n' "$oracle_rustc" > "$EV/oracle-rustc.exit"
echo "PHASE oracle_rustc=$oracle_rustc"
record oracle-rustc "$oracle_rustc"
if [ "$oracle_rustc" -ne 0 ]; then
  remember "$oracle_rustc"
  printf '%s\n' "$first" > "$EV/proof.exit"
  exit "$first"
fi
mkdir -p "$EV/oracle-out"
set +e
/cargo-home/bin/review-oracle \
  "$BIN" \
  /src/tools/query-result-export/tests/fixtures/query-clerk-order_doc-star.stdout \
  /evidence/oracle/hand-clerk.csv \
  /src/tools/query-result-export/tests/fixtures/hand-formula.jsonl \
  /cargo-home/bin/sha256 \
  "$EV/oracle-out" \
  > "$EV/oracle.stdout" 2> "$EV/oracle.stderr"
oracle_exit=$?
set +e
printf '%s\n' "$oracle_exit" > "$EV/oracle.exit"
echo "PHASE oracle_exit=$oracle_exit"
record oracle "$oracle_exit"
remember "$oracle_exit"
printf '%s\n' "$first" > "$EV/proof.exit"
if [ "$first" -eq 0 ]; then
  echo "proof_done"
fi
exit "$first"
