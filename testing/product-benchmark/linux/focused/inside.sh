#!/bin/bash
# Focused tests and Clippy for the zero-dependency product-benchmark scorer.
# No PostgreSQL. No cargo update. No root lock. Cargo stays --locked --offline.
# Clippy component install follows the a349 focused inside recipe. The pinned
# image does not ship cargo-clippy.
set -u
echo "oo_task=sp12-product-benchmark-focused"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a389"
echo "phase=focused"
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target/product-benchmark
export CARGO_TERM_COLOR=never
export HOME=/tmp
export TMPDIR=/cargo-home/tmp
export CARGO_BUILD_JOBS=1
mkdir -p /evidence/phases /cargo-home/tmp /target/product-benchmark
cd /crate

record() {
  mkdir -p /evidence/phases
  printf '%s\n' "$2" > "/evidence/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
}

got=$(rustc --version)
printf '%s\n' "$got" > /evidence/rustc-version.txt
case "$got" in
  "rustc 1.94.0 "*) record toolchain 0 ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2; printf '%s\n' 2 > /evidence/focused.exit; exit 2 ;;
esac
printf '%s\n' "no" > /evidence/cargo-update-invoked.txt
uname -m > /evidence/uname.txt

echo "=== cargo test --locked --offline ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -- --test-threads=1 \
  > /evidence/test.log 2>&1
test_exit=$?
set +e
record test "$test_exit"

sum_field() {
  grep '^test result:' /evidence/test.log \
    | sed -n "s/.* \\([0-9][0-9]*\\) $1.*/\\1/p" \
    | awk '{s+=$1} END{print s+0}'
}
printf 'passed=%s failed=%s ignored=%s\n' "$(sum_field passed)" "$(sum_field failed)" "$(sum_field ignored)" \
  > /evidence/test-count.txt
count_exit=0
if [ "$(cat /evidence/test-count.txt)" != "passed=7 failed=0 ignored=0" ]; then
  echo "unexpected test count" >&2
  count_exit=1
fi
for name in abc_and_empty freeze_matches_pre_edit_hash full_synthetic_loop_passes_without_integration http_200_with_wrong_cells_is_not_e2e citation_ok_does_not_satisfy_the_false_control accepted_hidden_query_is_not_a_counted_rejection missing_producer_file_does_not_pass; do
  if ! grep -F "$name ... ok" /evidence/test.log >/dev/null; then
    echo "missing passing test $name" >&2
    count_exit=1
  fi
done
record test-count "$count_exit"

echo "=== rustup component add clippy ==="
set +e
rustup component add clippy --toolchain 1.94.0 > /evidence/clippy-component.log 2>&1
component_exit=$?
set +e
record clippy-component "$component_exit"
cargo clippy --version > /evidence/clippy-version.txt 2>&1 || true

clippy_exit=2
if [ "$component_exit" -eq 0 ]; then
  echo "=== cargo clippy --locked --offline ==="
  set +e
  cargo clippy --manifest-path Cargo.toml --locked --offline --all-targets -- -D warnings \
    > /evidence/clippy.log 2>&1
  clippy_exit=$?
  set +e
fi
record clippy "$clippy_exit"

update_exit=0
if grep -F 'cargo update' /evidence/test.log /evidence/clippy.log >/dev/null 2>&1; then
  echo "cargo update text appeared in a log" >&2
  update_exit=1
fi
record cargo-update-text "$update_exit"

final=0
for phase in toolchain test test-count clippy-component clippy cargo-update-text; do
  code=$(cat "/evidence/phases/$phase.exit")
  if [ "$code" -ne 0 ] && [ "$final" -eq 0 ]; then
    final=$code
  fi
done
printf '%s\n' "$final" > /evidence/focused.exit
if [ "$final" -eq 0 ]; then
  echo "focused=ok"
else
  echo "focused=fail exit=$final" >&2
fi
exit "$final"
