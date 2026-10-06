#!/bin/bash
# Repaired assembler on the fixed archive, then author tests, Clippy, and the a284 overlay.
# The archive stays read-only. Two repair files are copied onto the work tree.
# No lock generation. No Python. No PostgreSQL.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target/tool
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1
export CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0
export PATH=/usr/local/cargo/bin:/usr/local/bin:$PATH
printf '%s\n' no > /evidence/cargo-update-invoked.txt
SEALED_ASSEMBLE=ac92b4300e76a33e8a1b9185269d4750b6355c9955136f7765b95c4715aafe8a
REPAIR_ASSEMBLE=9b9940554767b88b7d5eef81047cf7590c884e8094a498416a27cfa097621840
REPAIR_TEST=d3503329dbff0802e4739557761c3c0c62cb1b85a7e2134c1c02deba47c42b9f
A284_SHA=19c1faeaa5c7503e09e5f522f757a43a8f2602e57dd3644b79f1409b7bc24a20
SEALED_LOCK=4f2abdc20f88225bba1b91c7b7dbc1af26102e08144e45dc6f16e04664b1f26a

echo "=== identity ==="
rustc -vV
cargo -V
rustup show
uname -a
cat /etc/os-release
id
if ! rustc -vV | grep -q '^release: 1.94.0$'; then
  echo "toolchain is not 1.94.0" >&2
  printf '%s\n' 2 > /evidence/version.exit
  exit 2
fi
if ! rustc -vV | grep -q '^host: aarch64-unknown-linux-gnu$'; then
  echo "host is not linux arm64" >&2
  printf '%s\n' 2 > /evidence/version.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/version.exit

rm -rf /work/cdc-transaction /work/tmp
mkdir -p /work/tmp /work/cdc-transaction
export TMPDIR=/work/tmp
cp -R /tool/. /work/cdc-transaction/
find /work -name '._*' -delete
find /work -name '.DS_Store' -delete
printf '%s\n' no-root-toolchain > /evidence/toolchain-aside.txt
cd /work/cdc-transaction
sealed=$(sha256sum /tool/src/assemble.rs | awk '{print $1}')
printf '%s\n' "$sealed" > /evidence/sealed-assemble.sha256
if [ "$sealed" != "$SEALED_ASSEMBLE" ]; then
  echo "archive assembler is not the sealed file" >&2
  printf '%s\n' 2 > /evidence/sealed.exit
  exit 2
fi
if [ -f /tool/tests/ordinal_range.rs ]; then
  echo "archive already contains ordinal_range.rs" >&2
  printf '%s\n' 2 > /evidence/sealed.exit
  exit 2
fi
cp /repair/assemble.rs src/assemble.rs
cp /repair/ordinal_range.rs tests/ordinal_range.rs
repair=$(sha256sum src/assemble.rs | awk '{print $1}')
test_sha=$(sha256sum tests/ordinal_range.rs | awk '{print $1}')
printf '%s\n' "$repair" > /evidence/repair-assemble.sha256
printf '%s\n' "$test_sha" > /evidence/ordinal-test.sha256
if [ "$repair" != "$REPAIR_ASSEMBLE" ] || [ "$test_sha" != "$REPAIR_TEST" ]; then
  echo "repair bytes are not the pinned files" >&2
  printf '%s\n' 2 > /evidence/sealed.exit
  exit 2
fi
if [ "$repair" = "$sealed" ]; then
  echo "repair did not change assemble.rs" >&2
  printf '%s\n' 2 > /evidence/sealed.exit
  exit 2
fi
lock_sha=$(sha256sum Cargo.lock | awk '{print $1}')
printf '%s\n' "$lock_sha" > /evidence/child-lock.sha256
if [ "$lock_sha" != "$SEALED_LOCK" ]; then
  echo "child lock changed" >&2
  printf '%s\n' 2 > /evidence/sealed.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/sealed.exit
find Cargo.toml Cargo.lock CONTRACT.md src tests -type f | sort | xargs sha256sum > /evidence/source-sha256.txt

python_present=0
if command -v python3 >/dev/null 2>&1 || command -v python >/dev/null 2>&1; then
  python_present=1
fi
printf '%s\n' "$python_present" > /evidence/python-present.flag
echo "python_present_flag=$python_present"

echo "=== registry triples ==="
rustc --edition 2024 -D warnings /tools/lock_subset.rs -o /usr/local/bin/lock_subset
lock_compile=$?
printf '%s\n' "$lock_compile" > /evidence/lock-compile.exit
if [ "$lock_compile" -ne 0 ]; then
  echo "lock_subset did not compile" >&2
  exit "$lock_compile"
fi
set +e
/usr/local/bin/lock_subset Cargo.lock /rootlock/Cargo.lock > /evidence/lock-subset.stdout 2> /evidence/lock-subset.stderr
lock_exit=$?
set -e
printf '%s\n' "$lock_exit" > /evidence/lock-subset.exit
echo "PHASE lock_subset_exit=$lock_exit"
awk '$1=="triple" { print $2, $3, $4, $5 }' /evidence/lock-subset.stdout > /evidence/lock-triples.txt
if ! cmp -s /evidence/lock-triples.txt /oracle/registry-triples.txt; then
  echo "registry triple list drifted" >&2
  printf '%s\n' 2 > /evidence/lock-compare.exit
  lock_exit=2
else
  printf '%s\n' 0 > /evidence/lock-compare.exit
fi
grep -q 'summary registry_triples=11 missing=0 mismatch=0 path_packages=1 direct_pins_present=true' /evidence/lock-subset.stdout || {
  echo "registry summary is not 11/0/0 path 1" >&2
  printf '%s\n' 2 > /evidence/lock-summary.exit
  lock_exit=2
}
if [ ! -f /evidence/lock-summary.exit ]; then
  printf '%s\n' 0 > /evidence/lock-summary.exit
fi
if [ "$lock_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/fetch.exit
  printf '%s\n' not_run > /evidence/test.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  printf '%s\n' not_run > /evidence/a284.exit
  exit "$lock_exit"
fi

echo "=== cargo fetch locked offline ==="
set +e
cargo fetch --locked --offline > /evidence/fetch-offline.stdout 2> /evidence/fetch-offline.stderr
offline_fetch_exit=$?
set -e
printf '%s\n' "$offline_fetch_exit" > /evidence/fetch-offline.exit
echo "PHASE offline_fetch_exit=$offline_fetch_exit"
fetch_exit=$offline_fetch_exit
if [ "$offline_fetch_exit" -ne 0 ]; then
  echo "=== cargo fetch locked online ==="
  set +e
  cargo fetch --locked > /evidence/fetch.stdout 2> /evidence/fetch.stderr
  fetch_exit=$?
  set -e
else
  cp /evidence/fetch-offline.stdout /evidence/fetch.stdout
  cp /evidence/fetch-offline.stderr /evidence/fetch.stderr
fi
printf '%s\n' "$fetch_exit" > /evidence/fetch.exit
echo "PHASE fetch_exit=$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/test.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  printf '%s\n' not_run > /evidence/a284.exit
  exit "$fetch_exit"
fi

echo "=== repaired cargo test ==="
set +e
cargo test --locked --offline -- --test-threads=1 > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
set -e
printf '%s\n' "$test_exit" > /evidence/test.exit
echo "PHASE test_exit=$test_exit"
awk '/^test result:/ { passed += $4 } END { print passed+0 }' /evidence/test.stdout > /evidence/test-count.txt
test_count=$(cat /evidence/test-count.txt)
echo "test_passed_count=$test_count"
missing_name=0
for name in \
  preimage_keeps_null_distinct_from_unavailable \
  timestamp_matches_the_core_stored_spelling \
  known_answers \
  hand_lsn_and_position \
  lsn_rejects_noncanonical_and_wide_digits \
  happy_path_exits_zero_and_keeps_applied_null \
  missing_end_exits_three_without_ack \
  usage_row_lsn_duplicate_key_and_long_line \
  hand_two_table_update_delete_reinsert \
  interleaved_commit_order_is_end_order \
  row_lsn_is_not_a_commit_position \
  unselected_rows_count_and_are_omitted \
  selected_siblings_keep_original_sequences \
  eof_keeps_open_txn_and_reports_completed_separately \
  end_too_early_keeps_the_pending_event \
  rollback_drops_pending_and_replays \
  identical_replay_is_idempotent_and_conflict_refuses \
  commit_position_is_unique_and_resets_only_on_a_barrier \
  epoch_barrier_changes_the_position_high_half \
  count_overflow_and_bad_lsn_do_not_wrap \
  sequence_and_collection_faults \
  ordinal_null_and_duplicate_keys \
  open_set_stops_at_sixteen \
  empty_transaction_and_rollback_after_complete \
  line_of_exactly_one_mebibyte_is_accepted \
  txn_of_200000_completes_and_the_next_needs_spool \
  payload_budget_is_exact_and_the_next_event_is_not_stored \
  ordinal_32767_completes_for_each_in_range_shape \
  ordinals_above_32767_refuse_without_storing \
  unavailable_list_above_32767_is_ordinal_range \
  unselected_high_ordinal_refuses_and_in_range_is_omitted \
  ordinal_65536_is_malformed_and_stores_nothing \
  stored_event_survives_refusal_and_fresh_retry_keeps_the_hash \
  cli_ordinal_32767_exits_zero \
  cli_high_ordinals_exit_ordinal_range_with_no_complete \
  cli_ordinal_65536_is_malformed \
  cli_unselected_and_shapes_match_the_bound
do
  if ! grep -q "$name" /evidence/test.stdout; then
    echo "suite missing $name" >&2
    missing_name=1
  fi
done
printf '%s\n' "$missing_name" > /evidence/test-names.exit
if [ "$test_exit" -ne 0 ] || [ "$test_count" -ne 37 ] || [ "$missing_name" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/clippy.exit
  printf '%s\n' not_run > /evidence/a284.exit
  if [ "$test_exit" -ne 0 ]; then
    exit "$test_exit"
  fi
  exit 1
fi

echo "=== rustup component add clippy ==="
set +e
rustup component add clippy --toolchain 1.94.0 > /evidence/clippy-component.stdout 2> /evidence/clippy-component.stderr
component_exit=$?
set -e
printf '%s\n' "$component_exit" > /evidence/clippy-component.exit
echo "PHASE clippy_component_exit=$component_exit"
if [ "$component_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/clippy.exit
  printf '%s\n' not_run > /evidence/a284.exit
  exit "$component_exit"
fi

echo "=== cargo clippy ==="
set +e
cargo clippy --locked --offline --all-targets -- -D warnings > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"
if [ "$clippy_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/a284.exit
  exit "$clippy_exit"
fi

echo "=== a284 independent overlay ==="
a284_sha=$(sha256sum /a284/a284_independent.rs | awk '{print $1}')
printf '%s\n' "$a284_sha" > /evidence/a284-overlay.sha256
if [ "$a284_sha" != "$A284_SHA" ]; then
  echo "a284 overlay hash drifted" >&2
  printf '%s\n' 2 > /evidence/a284.exit
  exit 2
fi
cp /a284/a284_independent.rs tests/a284_independent.rs
set +e
cargo test --locked --offline --test a284_independent -- --test-threads=1 > /evidence/a284.stdout 2> /evidence/a284.stderr
a284_exit=$?
set -e
printf '%s\n' "$a284_exit" > /evidence/a284.exit
echo "PHASE a284_exit=$a284_exit"
awk '/^test result:/ { passed += $4 } END { print passed+0 }' /evidence/a284.stdout > /evidence/a284-count.txt
if ! grep -q '^test ordinals_32768_and_65535_are_outside_the_contract ... ok$' /evidence/a284.stdout; then
  echo "a284 ordinal assertion did not pass" >&2
  if [ "$a284_exit" -eq 0 ]; then
    a284_exit=1
    printf '%s\n' 1 > /evidence/a284.exit
  fi
fi

mismatch=0
while IFS= read -r rel; do
  case "$rel" in
    src/assemble.rs|tests/ordinal_range.rs|tests/a284_independent.rs) continue ;;
  esac
  if ! cmp -s "$rel" "/tool/$rel"; then
    echo "unexpected product change $rel" >&2
    mismatch=1
  fi
done < <(find Cargo.toml Cargo.lock CONTRACT.md src tests -type f | sed 's|^\./||' | sort)
printf '%s\n' "$mismatch" > /evidence/product-scope.exit
sha256sum /rootlock/Cargo.lock | awk '{print $1}' > /evidence/root-lock.sha256
if ! cmp -s Cargo.lock /tool/Cargo.lock; then
  echo "child lock changed during the run" >&2
  printf '%s\n' 2 > /evidence/lock-unchanged.exit
  mismatch=1
else
  printf '%s\n' 0 > /evidence/lock-unchanged.exit
fi
worst=0
if [ "$a284_exit" -ne 0 ]; then worst=$a284_exit; fi
if [ "$mismatch" -ne 0 ]; then worst=2; fi
echo "worst_phase_exit=$worst"
exit "$worst"
