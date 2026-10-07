#!/bin/bash
# Post-edit focused test, Clippy, and live fixture export on the overlay copy.
# Linux arm64 Rust 1.94. The source mount is read-only. No cargo update.
# No apt. No PostgreSQL. Clippy install uses the bridge network.
# The lock has no registry packages, so fetch, test, and Clippy stay offline.
set -u
echo "oo_task=sp05-query-result-export-postedit"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a325"
echo "oo_fixed=e8d297f95b32052ea587fe8ad67b6e8493a00300"
echo "oo_platform=linux/arm64"
echo "phase=postedit"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
mkdir -p /evidence/linux/postedit/phases /cargo-home/bin
cd /src
printf '%s\n' "not-invoked" > /evidence/linux/postedit/cargo-update-invoked.txt
MANIFEST=tools/query-result-export/Cargo.toml
EV=/evidence/linux/postedit

record() {
  mkdir -p "$EV/phases"
  printf '%s\n' "$2" > "$EV/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
}

fail() {
  record "$1" "$2"
  printf '%s\n' "$2" > "$EV/postedit.exit"
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
if [ "$sha" != "e8d297f95b32052ea587fe8ad67b6e8493a00300" ]; then
  echo "archive sha mismatch" >&2
  fail source 2
fi
if [ ! -f "$MANIFEST" ] || [ ! -f tools/query-result-export/src/lib.rs ]; then
  echo "query-result-export overlay is absent" >&2
  fail source 2
fi
sha256 dir "$EV/package-sha256.txt" /src/tools/query-result-export
hash_exit=$?
record package-hash "$hash_exit"
if [ "$hash_exit" -ne 0 ]; then
  fail package-hash "$hash_exit"
fi
cmp -s \
  /src/tools/query-result-export/tests/fixtures/query-clerk-order_doc-star.stdout \
  /src/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a226/SP11/reviewed-pg-composition/live/steps/query-clerk-order_doc-star.stdout
printf '%s\n' "$?" > "$EV/archive-fixture-cmp.exit"
if [ "$(cat "$EV/archive-fixture-cmp.exit")" != "0" ]; then
  echo "overlay fixture differs from the archived compiled receipt" >&2
  fail archive-fixture 2
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
if [ "$fetch_exit" -ne 0 ]; then
  fail fetch "$fetch_exit"
fi
record fetch 0

echo "=== cargo test --locked --offline ==="
set +e
cargo test --manifest-path "$MANIFEST" --locked --offline --no-fail-fast -- --test-threads=1 > "$EV/test.stdout" 2> "$EV/test.stderr"
test_exit=$?
set +e
printf '%s\n' "$test_exit" > "$EV/test.exit"
echo "PHASE test_exit=$test_exit"
record test "$test_exit"

echo "=== cargo clippy --locked --offline ==="
set +e
cargo clippy --manifest-path "$MANIFEST" --locked --offline --all-targets -- -D warnings > "$EV/clippy.stdout" 2> "$EV/clippy.stderr"
clippy_exit=$?
set +e
printf '%s\n' "$clippy_exit" > "$EV/clippy.exit"
echo "PHASE clippy_exit=$clippy_exit"
record clippy "$clippy_exit"

if [ "$test_exit" -ne 0 ]; then
  printf '%s\n' "$test_exit" > "$EV/postedit.exit"
  exit "$test_exit"
fi
if [ "$clippy_exit" -ne 0 ]; then
  printf '%s\n' "$clippy_exit" > "$EV/postedit.exit"
  exit "$clippy_exit"
fi

echo "=== live clerk export ==="
set +e
cargo build --manifest-path "$MANIFEST" --locked --offline > "$EV/build.stdout" 2> "$EV/build.stderr"
build_exit=$?
set +e
printf '%s\n' "$build_exit" > "$EV/build.exit"
echo "PHASE build_exit=$build_exit"
if [ "$build_exit" -ne 0 ]; then
  fail build "$build_exit"
fi
record build 0
BIN=/target/debug/query-result-export
if [ ! -x "$BIN" ]; then
  echo "exporter binary is absent" >&2
  fail live 2
fi
mkdir -p /tmp/live-out
cat > /tmp/live-request.json <<'EOF'
{"version":"oo.query-result-export/v1","source_file":"/src/tools/query-result-export/tests/fixtures/query-clerk-order_doc-star.stdout","output_dir":"/tmp/live-out","columns":["order_id","customer_id","memo","posted","amount","clerk_note"]}
EOF
set +e
"$BIN" --request /tmp/live-request.json > "$EV/live.stdout" 2> "$EV/live.stderr"
live_exit=$?
set +e
printf '%s\n' "$live_exit" > "$EV/live.exit"
echo "PHASE live_exit=$live_exit"
if [ "$live_exit" -ne 0 ]; then
  fail live "$live_exit"
fi
if [ -s "$EV/live.stdout" ] || [ -s "$EV/live.stderr" ]; then
  echo "complete export wrote stdio" >&2
  fail live-stdio 2
fi
cmp -s /tmp/live-out/export.csv /src/tools/query-result-export/tests/fixtures/golden-clerk.csv
printf '%s\n' "$?" > "$EV/live-csv-cmp.exit"
cmp -s /tmp/live-out/export.source.jsonl /src/tools/query-result-export/tests/fixtures/query-clerk-order_doc-star.stdout
printf '%s\n' "$?" > "$EV/live-source-cmp.exit"
if [ "$(cat "$EV/live-csv-cmp.exit")" != "0" ] || [ "$(cat "$EV/live-source-cmp.exit")" != "0" ]; then
  echo "live bytes differ from the hand golden or the source" >&2
  fail live-cmp 2
fi
grep -F '"complete":true' /tmp/live-out/export.sidecar.json >/dev/null
printf '%s\n' "$?" > "$EV/live-complete-grep.exit"
grep -F '"xlsx":"unsupported"' /tmp/live-out/export.sidecar.json >/dev/null
printf '%s\n' "$?" > "$EV/live-xlsx-grep.exit"
if [ "$(cat "$EV/live-complete-grep.exit")" != "0" ] || [ "$(cat "$EV/live-xlsx-grep.exit")" != "0" ]; then
  echo "live sidecar flags mismatch" >&2
  fail live-sidecar 2
fi
set +e
"$BIN" --request /tmp/live-request.json > "$EV/second.stdout" 2> "$EV/second.stderr"
second_exit=$?
set +e
printf '%s\n' "$second_exit" > "$EV/second.exit"
echo "PHASE second_exit=$second_exit"
printf '%s\n' "query-result-export: path" > /tmp/expect-path.txt
cmp -s "$EV/second.stderr" /tmp/expect-path.txt
printf '%s\n' "$?" > "$EV/second-stderr-cmp.exit"
if [ "$second_exit" -ne 4 ] || [ "$(cat "$EV/second-stderr-cmp.exit")" != "0" ] || [ -s "$EV/second.stdout" ]; then
  echo "second write was not refused" >&2
  fail second 2
fi
cmp -s /tmp/live-out/export.csv /src/tools/query-result-export/tests/fixtures/golden-clerk.csv
printf '%s\n' "$?" > "$EV/second-csv-kept.exit"
if [ "$(cat "$EV/second-csv-kept.exit")" != "0" ]; then
  fail second-kept 2
fi
mkdir -p /tmp/live-partial
sed 's/"truncated":false/"truncated":true/' \
  /src/tools/query-result-export/tests/fixtures/query-clerk-order_doc-star.stdout \
  > /tmp/partial.stdout
cat > /tmp/partial-request.json <<'EOF'
{"version":"oo.query-result-export/v1","source_file":"/tmp/partial.stdout","output_dir":"/tmp/live-partial","columns":["order_id","customer_id","memo","posted","amount","clerk_note"]}
EOF
set +e
"$BIN" --request /tmp/partial-request.json > "$EV/partial.stdout" 2> "$EV/partial.stderr"
partial_exit=$?
set +e
printf '%s\n' "$partial_exit" > "$EV/partial.exit"
echo "PHASE partial_exit=$partial_exit"
printf '%s\n' "query-result-export: partial" > /tmp/expect-partial.txt
cmp -s "$EV/partial.stderr" /tmp/expect-partial.txt
printf '%s\n' "$?" > "$EV/partial-stderr-cmp.exit"
cmp -s /tmp/live-partial/export.csv /src/tools/query-result-export/tests/fixtures/golden-clerk.csv
printf '%s\n' "$?" > "$EV/partial-csv-cmp.exit"
grep -F '"complete":false' /tmp/live-partial/export.sidecar.json >/dev/null
printf '%s\n' "$?" > "$EV/partial-complete-grep.exit"
grep -F '"partial":true' /tmp/live-partial/export.sidecar.json >/dev/null
printf '%s\n' "$?" > "$EV/partial-flag-grep.exit"
if [ "$partial_exit" -ne 1 ] \
  || [ "$(cat "$EV/partial-stderr-cmp.exit")" != "0" ] \
  || [ -s "$EV/partial.stdout" ] \
  || [ "$(cat "$EV/partial-csv-cmp.exit")" != "0" ] \
  || [ "$(cat "$EV/partial-complete-grep.exit")" != "0" ] \
  || [ "$(cat "$EV/partial-flag-grep.exit")" != "0" ]; then
  echo "partial export was not flagged" >&2
  fail partial 2
fi
mkdir -p "$EV/live"
cp /tmp/live-request.json "$EV/live/request.json"
cp /tmp/live-out/export.csv "$EV/live/export.csv"
cp /tmp/live-out/export.sidecar.json "$EV/live/export.sidecar.json"
cp /tmp/live-out/export.source.jsonl "$EV/live/export.source.jsonl"
cp /tmp/live-partial/export.sidecar.json "$EV/live/partial.sidecar.json"
sha256 file \
  "$EV/live/export.csv" \
  "$EV/live/export.sidecar.json" \
  "$EV/live/export.source.jsonl" \
  "$EV/live/partial.sidecar.json" \
  > "$EV/live-sha256.txt"
record live 0
printf '%s\n' 0 > "$EV/postedit.exit"
echo "postedit_done"
