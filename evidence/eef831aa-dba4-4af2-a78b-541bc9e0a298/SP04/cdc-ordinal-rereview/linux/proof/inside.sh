#!/bin/bash
# Review the fixed CDC-D01 archive. No product edit. No lock generation.
# Author suite and Clippy run before the review overlays are copied.
# No Python. No PostgreSQL.
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

ASSEMBLE=9b9940554767b88b7d5eef81047cf7590c884e8094a498416a27cfa097621840
ORDINAL_TEST=d3503329dbff0802e4739557761c3c0c62cb1b85a7e2134c1c02deba47c42b9f
CHILD_LOCK=4f2abdc20f88225bba1b91c7b7dbc1af26102e08144e45dc6f16e04664b1f26a
ROOT_LOCK=16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a
A284_SHA=19c1faeaa5c7503e09e5f522f757a43a8f2602e57dd3644b79f1409b7bc24a20
A298_SHA=719dfc83e2c0c4ca35a05954e9b79a71449e936267ba99ade706f8d7d6bee0bc

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

python_present=0
if command -v python3 >/dev/null 2>&1 || command -v python >/dev/null 2>&1; then
  python_present=1
fi
printf '%s\n' "$python_present" > /evidence/python-present.flag
echo "python_present_flag=$python_present"

fail_pin() {
  echo "$1" >&2
  printf '%s\n' 2 > /evidence/sealed.exit
  exit 2
}
pin_one() {
  local file="$1"
  local want="$2"
  local got
  got=$(sha256sum "$file" | awk '{print $1}')
  if [ "$got" != "$want" ]; then
    fail_pin "hash mismatch $file got $got want $want"
  fi
}
pin_one src/assemble.rs "$ASSEMBLE"
pin_one tests/ordinal_range.rs "$ORDINAL_TEST"
pin_one Cargo.lock "$CHILD_LOCK"
pin_one /rootlock/Cargo.lock "$ROOT_LOCK"
if [ ! -f tests/ordinal_range.rs ]; then
  fail_pin "fixed archive is missing ordinal_range.rs"
fi
printf '%s\n' 0 > /evidence/sealed.exit
find Cargo.toml Cargo.lock CONTRACT.md src tests -type f | sort | xargs sha256sum > /evidence/source-sha256.txt
find src tests -type f | sort > /evidence/src-before.txt
sha256sum src/assemble.rs | awk '{print $1}' > /evidence/assemble.sha256
sha256sum tests/ordinal_range.rs | awk '{print $1}' > /evidence/ordinal-test.sha256
sha256sum Cargo.lock | awk '{print $1}' > /evidence/child-lock.sha256
sha256sum /rootlock/Cargo.lock | awk '{print $1}' > /evidence/root-lock.sha256

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
  printf '%s\n' not_run > /evidence/smoke-test.exit
  printf '%s\n' not_run > /evidence/clippy-before.exit
  printf '%s\n' not_run > /evidence/a284.exit
  printf '%s\n' not_run > /evidence/a298.exit
  printf '%s\n' not_run > /evidence/clippy.exit
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
  printf '%s\n' not_run > /evidence/smoke-test.exit
  printf '%s\n' not_run > /evidence/clippy-before.exit
  printf '%s\n' not_run > /evidence/a284.exit
  printf '%s\n' not_run > /evidence/a298.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  exit "$fetch_exit"
fi

if [ -f tests/a284_independent.rs ] || [ -f tests/a298_ordinal.rs ]; then
  echo "review overlay was already in the archive" >&2
  printf '%s\n' 2 > /evidence/overlay-absent.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/overlay-absent.exit

echo "=== author cargo test before overlay ==="
set +e
cargo test --locked --offline -- --test-threads=1 > /evidence/smoke-test.stdout 2> /evidence/smoke-test.stderr
smoke_exit=$?
set -e
printf '%s\n' "$smoke_exit" > /evidence/smoke-test.exit
echo "PHASE smoke_exit=$smoke_exit"
awk '/^test result:/ { passed += $4 } END { print passed+0 }' /evidence/smoke-test.stdout > /evidence/smoke-count.txt
smoke_count=$(cat /evidence/smoke-count.txt)
echo "smoke_passed_count=$smoke_count"
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
  if ! grep -q "$name" /evidence/smoke-test.stdout; then
    echo "suite missing $name" >&2
    missing_name=1
  fi
done
printf '%s\n' "$missing_name" > /evidence/smoke-names.exit
if [ "$smoke_exit" -ne 0 ] || [ "$smoke_count" -ne 37 ] || [ "$missing_name" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/clippy-before.exit
  printf '%s\n' not_run > /evidence/a284.exit
  printf '%s\n' not_run > /evidence/a298.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  if [ "$smoke_exit" -ne 0 ]; then
    exit "$smoke_exit"
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
  printf '%s\n' not_run > /evidence/clippy-before.exit
  printf '%s\n' not_run > /evidence/a284.exit
  printf '%s\n' not_run > /evidence/a298.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  exit "$component_exit"
fi

echo "=== cargo clippy before overlay ==="
set +e
cargo clippy --locked --offline --all-targets -- -D warnings > /evidence/clippy-before.stdout 2> /evidence/clippy-before.stderr
clippy_before=$?
set -e
printf '%s\n' "$clippy_before" > /evidence/clippy-before.exit
echo "PHASE clippy_before_exit=$clippy_before"
if [ "$clippy_before" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/a284.exit
  printf '%s\n' not_run > /evidence/a298.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  exit "$clippy_before"
fi

echo "=== copy review overlays ==="
a284_sha=$(sha256sum /a284/a284_independent.rs | awk '{print $1}')
a298_sha=$(sha256sum /overlay/a298_ordinal.rs | awk '{print $1}')
printf '%s\n' "$a284_sha" > /evidence/a284-overlay.sha256
printf '%s\n' "$a298_sha" > /evidence/a298-overlay.sha256
if [ "$a284_sha" != "$A284_SHA" ] || [ "$a298_sha" != "$A298_SHA" ]; then
  echo "overlay hash drifted" >&2
  printf '%s\n' 2 > /evidence/overlay-present.exit
  exit 2
fi
cp /a284/a284_independent.rs tests/a284_independent.rs
cp /overlay/a298_ordinal.rs tests/a298_ordinal.rs
printf '%s\n' 0 > /evidence/overlay-present.exit

echo "=== unchanged a284 overlay ==="
set +e
cargo test --locked --offline --test a284_independent -- --test-threads=1 > /evidence/a284.stdout 2> /evidence/a284.stderr
a284_exit=$?
set -e
printf '%s\n' "$a284_exit" > /evidence/a284.exit
echo "PHASE a284_exit=$a284_exit"
awk '/^test result:/ { passed += $4 } END { print passed+0 }' /evidence/a284.stdout > /evidence/a284-count.txt
a284_missing=0
for name in \
  sha256_known_vectors \
  begin_data_end_count_filter_and_attestations \
  interleaved_end_order_keeps_each_sequence \
  rollback_does_not_complete_and_replay_is_idempotent \
  duplicate_payload_conflicts_and_identical_does_not_double \
  eof_pending_keeps_completed_and_open \
  lineage_reset_clears_floor_and_keeps_position_identity \
  count_overflow_does_not_wrap_and_early_end_keeps_pending \
  row_lsn_is_not_a_commit_fallback \
  u16_decoder_accepts_32768_and_65535 \
  ordinal_32767_completes_inside_the_contract \
  ordinals_32768_and_65535_are_outside_the_contract \
  ordinal_65536_is_malformed_because_it_does_not_fit_u16 \
  value_byte_budget_is_separate_from_line_length_and_the_event_cap \
  event_cap_200000_completes_with_zero_value_bytes
do
  if ! grep -q "^test ${name} \\.\\.\\. ok$" /evidence/a284.stdout; then
    echo "a284 missing ok $name" >&2
    a284_missing=1
  fi
done
printf '%s\n' "$a284_missing" > /evidence/a284-names.exit
a284_count=$(cat /evidence/a284-count.txt)
if [ "$a284_exit" -ne 0 ] || [ "$a284_count" -ne 15 ] || [ "$a284_missing" -ne 0 ]; then
  if [ "$a284_exit" -eq 0 ]; then
    a284_exit=1
    printf '%s\n' 1 > /evidence/a284.exit
  fi
fi

echo "=== independent a298 overlay ==="
set +e
cargo test --locked --offline --test a298_ordinal -- --test-threads=1 > /evidence/a298.stdout 2> /evidence/a298.stderr
a298_exit=$?
set -e
printf '%s\n' "$a298_exit" > /evidence/a298.exit
echo "PHASE a298_exit=$a298_exit"
awk '/^test result:/ { passed += $4 } END { print passed+0 }' /evidence/a298.stdout > /evidence/a298-count.txt
a298_missing=0
for name in \
  fips_and_pinned_preimage \
  api_32767_selected_unselected_key_and_cells \
  api_32768_and_65535_refuse_before_seen_or_buffer \
  api_range_before_key_order_and_unavailable_mismatch \
  api_65536_is_malformed_and_stores_nothing \
  api_stored_event_remains_and_fresh_retry_matches_hash \
  api_existing_malformed_fault_stays \
  cli_32767_positive_and_high_ordinals_do_not_complete \
  cli_65536_malformed_and_stored_in_range_remains
do
  if ! grep -q "^test ${name} \\.\\.\\. ok$" /evidence/a298.stdout; then
    echo "a298 missing ok $name" >&2
    a298_missing=1
  fi
done
printf '%s\n' "$a298_missing" > /evidence/a298-names.exit
a298_count=$(cat /evidence/a298-count.txt)
if [ "$a298_exit" -ne 0 ] || [ "$a298_count" -ne 9 ] || [ "$a298_missing" -ne 0 ]; then
  if [ "$a298_exit" -eq 0 ]; then
    a298_exit=1
    printf '%s\n' 1 > /evidence/a298.exit
  fi
fi

echo "=== cargo clippy after overlay ==="
set +e
cargo clippy --locked --offline --all-targets -- -D warnings > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"

mismatch=0
while IFS= read -r rel; do
  case "$rel" in
    tests/a284_independent.rs|tests/a298_ordinal.rs) continue ;;
  esac
  if ! cmp -s "$rel" "/tool/$rel"; then
    echo "unexpected product change $rel" >&2
    mismatch=1
  fi
done < <(find Cargo.toml Cargo.lock CONTRACT.md src tests -type f | sed 's|^\./||' | sort)
printf '%s\n' "$mismatch" > /evidence/product-scope.exit
if ! cmp -s Cargo.lock /tool/Cargo.lock; then
  echo "child lock changed during the run" >&2
  printf '%s\n' 2 > /evidence/lock-unchanged.exit
  mismatch=1
else
  printf '%s\n' 0 > /evidence/lock-unchanged.exit
fi
find src tests -type f | sort > /evidence/src-after.txt

worst=0
if [ "$a284_exit" -ne 0 ]; then worst=$a284_exit; fi
if [ "$a298_exit" -ne 0 ]; then worst=$a298_exit; fi
if [ "$clippy_exit" -ne 0 ]; then worst=$clippy_exit; fi
if [ "$mismatch" -ne 0 ]; then worst=2; fi
echo "worst_phase_exit=$worst"
exit "$worst"
