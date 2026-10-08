#!/bin/bash
# Focused scorer tests, then one ontology-dialogue propose with the approved policy id.
# Does not start PostgreSQL, does not reload the 100k chain, and does not call pg-builder.
set -u
echo "oo_task=sp12-product-benchmark-propose"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a389"
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TERM_COLOR=never
export HOME=/tmp
export TMPDIR=/cargo-home/tmp
export CARGO_BUILD_JOBS=1
unset HTTP_PROXY HTTPS_PROXY ALL_PROXY NO_PROXY http_proxy https_proxy all_proxy no_proxy OPENAI_API_KEY OPENAI_BASE_URL OPENAI_API_BASE || true
LEAF=/evidence/linux/propose
REVIEW=/evidence
mkdir -p "$LEAF/phases" /cargo-home/tmp /cargo-home/bin

record() {
  printf '%s\n' "$2" > "$LEAF/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
}

got=$(rustc --version)
printf '%s\n' "$got" > "$LEAF/rustc-version.txt"
case "$got" in
  "rustc 1.94.0 "*) record toolchain 0 ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2; printf '%s\n' 2 > "$LEAF/focused.exit"; exit 2 ;;
esac
printf '%s\n' "no" > "$LEAF/cargo-update-invoked.txt"
uname -m > "$LEAF/uname.txt"

export CARGO_TARGET_DIR=/target/product-benchmark
echo "=== cargo test --locked --offline ==="
set +e
cargo test --manifest-path /crate/Cargo.toml --locked --offline -- --test-threads=1 \
  > "$LEAF/test.log" 2>&1
test_exit=$?
set +e
record test "$test_exit"
sum_field() {
  grep '^test result:' "$LEAF/test.log" \
    | sed -n "s/.* \\([0-9][0-9]*\\) $1.*/\\1/p" \
    | awk '{s+=$1} END{print s+0}'
}
printf 'passed=%s failed=%s ignored=%s\n' "$(sum_field passed)" "$(sum_field failed)" "$(sum_field ignored)" \
  > "$LEAF/test-count.txt"
count_exit=0
if [ "$(cat "$LEAF/test-count.txt")" != "passed=7 failed=0 ignored=0" ]; then
  echo "unexpected test count" >&2
  count_exit=1
fi
for name in abc_and_empty freeze_matches_pre_edit_hash full_synthetic_loop_passes_without_integration http_200_with_wrong_cells_is_not_e2e citation_ok_does_not_satisfy_the_false_control accepted_hidden_query_is_not_a_counted_rejection missing_producer_file_does_not_pass; do
  if ! grep -F "$name ... ok" "$LEAF/test.log" >/dev/null; then
    echo "missing passing test $name" >&2
    count_exit=1
  fi
done
record test-count "$count_exit"

echo "=== rustup component add clippy ==="
set +e
rustup component add clippy --toolchain 1.94.0 > "$LEAF/clippy-component.log" 2>&1
component_exit=$?
set +e
record clippy-component "$component_exit"
cargo clippy --version > "$LEAF/clippy-version.txt" 2>&1 || true
clippy_exit=2
if [ "$component_exit" -eq 0 ]; then
  set +e
  cargo clippy --manifest-path /crate/Cargo.toml --locked --offline --all-targets -- -D warnings \
    > "$LEAF/clippy.log" 2>&1
  clippy_exit=$?
  set +e
fi
record clippy "$clippy_exit"

echo "=== prep ==="
set +e
rustc --edition 2024 -D warnings /crate/linux/bench/prep.rs -o /cargo-home/bin/oo-a389-prep \
  > "$LEAF/prep-build.log" 2>&1
prep_build=$?
set +e
record prep-build "$prep_build"

if [ ! -f "$REVIEW/live/builder/dialogue-config.pol-a317.json" ]; then
  cp "$REVIEW/score-input/dialogue-config.json" "$REVIEW/live/builder/dialogue-config.pol-a317.json"
fi
if [ ! -f "$REVIEW/bench/phases/propose.pol-a317.exit" ]; then
  cp "$REVIEW/score-input/dialogue-proposal.exit" "$REVIEW/bench/phases/propose.pol-a317.exit"
fi
if [ -f "$REVIEW/bench/dialogue-proposal.stderr" ] && [ ! -f "$REVIEW/bench/dialogue-proposal.pol-a317.stderr" ]; then
  cp "$REVIEW/bench/dialogue-proposal.stderr" "$REVIEW/bench/dialogue-proposal.pol-a317.stderr"
fi

propose_exit=1
if [ "$prep_build" -eq 0 ]; then
  set +e
  /cargo-home/bin/oo-a389-prep propose-config \
    "$REVIEW/live/builder/wide-pack.json" \
    "$REVIEW/live/builder/question.json" \
    /relay/cert.pem \
    /run/llm-token \
    grok-4.7 \
    https://127.0.0.1:8443 \
    "$REVIEW/live/run/queries/wide-count.request.json" \
    "$REVIEW/live/builder/dialogue-config.approved.json" \
    > "$LEAF/propose-config.log" 2>&1
  prep_exit=$?
  set +e
  record propose-config "$prep_exit"
else
  record propose-config 1
  prep_exit=1
fi

echo "=== dialogue binary ==="
export CARGO_TARGET_DIR=/target/dialogue
set +e
cargo fetch --manifest-path /dialogue-src/tools/ontology-dialogue/Cargo.toml --locked \
  > "$LEAF/fetch-dialogue.log" 2>&1
fetch_exit=$?
set +e
record fetch-dialogue "$fetch_exit"
build_exit=1
if [ "$fetch_exit" -eq 0 ]; then
  set +e
  cargo build --manifest-path /dialogue-src/tools/ontology-dialogue/Cargo.toml --locked --offline --bin ontology-dialogue \
    > "$LEAF/build-dialogue.log" 2>&1
  build_exit=$?
  set +e
fi
record build-dialogue "$build_exit"
if [ "$build_exit" -eq 0 ]; then
  cp /target/dialogue/debug/ontology-dialogue /cargo-home/bin/ontology-dialogue
fi

if [ "$prep_exit" -eq 0 ] && [ -x /cargo-home/bin/ontology-dialogue ]; then
  wc -c /run/llm-token | awk '{print $1}' > "$LEAF/bearer-file-bytes.txt"
  set +e
  /cargo-home/bin/ontology-dialogue propose --config "$REVIEW/live/builder/dialogue-config.approved.json" \
    > "$LEAF/dialogue-proposal.stdout" 2> "$LEAF/dialogue-proposal.stderr"
  propose_exit=$?
  set +e
  printf '%s\n' "$propose_exit" > "$REVIEW/score-input/dialogue-proposal.exit"
  cp "$REVIEW/live/builder/dialogue-config.approved.json" "$REVIEW/score-input/dialogue-config.json"
  cp "$LEAF/dialogue-proposal.stderr" "$REVIEW/bench/dialogue-proposal.approved.stderr"
  if [ -s "$LEAF/dialogue-proposal.stdout" ]; then
    cp "$LEAF/dialogue-proposal.stdout" "$REVIEW/score-input/dialogue-proposal.json"
  fi
else
  propose_exit=1
fi
record propose-approved "$propose_exit"
printf '%s\n' "$propose_exit" > "$REVIEW/bench/phases/propose-approved.exit"

export CARGO_TARGET_DIR=/target/product-benchmark
set +e
cargo build --manifest-path /crate/Cargo.toml --locked --offline --bin product-benchmark \
  > "$LEAF/build-score.log" 2>&1
score_build=$?
set +e
record build-score "$score_build"
score_exit=1
if [ "$score_build" -eq 0 ]; then
  set +e
  /target/product-benchmark/debug/product-benchmark score \
    --freeze /crate/FREEZE.json \
    --input "$REVIEW/score-input" \
    --out "$REVIEW/report" \
    > "$LEAF/score.log" 2>&1
  score_exit=$?
  set +e
fi
record score-approved "$score_exit"
printf '%s\n' "$score_exit" > "$REVIEW/bench/phases/score-approved.exit"
if [ ! -f "$REVIEW/bench/bench.exit.before-approved" ]; then
  cp "$REVIEW/bench/bench.exit" "$REVIEW/bench/bench.exit.before-approved"
fi
printf '%s\n' "$score_exit" > "$REVIEW/bench/bench.exit"

final=0
for phase in toolchain test test-count clippy-component clippy prep-build propose-config; do
  code=$(cat "$LEAF/phases/$phase.exit")
  if [ "$code" -ne 0 ] && [ "$final" -eq 0 ]; then
    final=$code
  fi
done
if [ "$build_exit" -ne 0 ] && [ "$final" -eq 0 ]; then
  final=$build_exit
fi
printf '%s\n' "$final" > "$LEAF/focused.exit"
printf '%s\n' "$score_exit" > "$LEAF/propose.exit"
echo "propose_exit=$propose_exit score_exit=$score_exit focused_exit=$final"
exit "$final"
