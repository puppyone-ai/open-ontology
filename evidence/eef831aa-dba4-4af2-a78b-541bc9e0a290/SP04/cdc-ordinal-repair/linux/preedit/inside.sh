#!/bin/bash
# Author 27 tests on the sealed assembler, then the ordinal overlay.
# The overlay is copied only after that smoke. Copy uses cp -R.
# The archive stays read-only. No lock generation. No Python.
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

echo "=== identity ==="
rustc -vV
echo "rustc_vv_exit=$?"
cargo -V
echo "cargo_v_exit=$?"
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
if [ ! -f Cargo.lock ] || [ ! -f src/lib.rs ] || [ ! -f src/main.rs ] || [ ! -f src/assemble.rs ]; then
  echo "product crate is incomplete" >&2
  printf '%s\n' 2 > /evidence/source.exit
  exit 2
fi
printf '%s\n' present > /evidence/lock.exit
find Cargo.toml Cargo.lock CONTRACT.md src tests -type f | sort | xargs sha256sum > /evidence/source-sha256.txt
assemble_sha=$(sha256sum src/assemble.rs | awk '{print $1}')
lock_sha=$(sha256sum Cargo.lock | awk '{print $1}')
printf '%s\n' "$assemble_sha" > /evidence/assemble.sha256
printf '%s\n' "$lock_sha" > /evidence/child-lock.sha256
if [ "$assemble_sha" != ac92b4300e76a33e8a1b9185269d4750b6355c9955136f7765b95c4715aafe8a ]; then
  echo "assemble.rs is not the sealed assembler" >&2
  printf '%s\n' 2 > /evidence/sealed.exit
  exit 2
fi
if [ "$lock_sha" != 4f2abdc20f88225bba1b91c7b7dbc1af26102e08144e45dc6f16e04664b1f26a ]; then
  echo "child lock is not the sealed lock" >&2
  printf '%s\n' 2 > /evidence/sealed.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/sealed.exit
{
  sha256sum Cargo.toml Cargo.lock CONTRACT.md
  find src tests -type f | sort | xargs sha256sum
} > /evidence/src-before.txt

python_present=0
if command -v python3 >/dev/null 2>&1 || command -v python >/dev/null 2>&1; then
  python_present=1
fi
printf '%s\n' "$python_present" > /evidence/python-present.flag
echo "python_present_flag=$python_present"

cmp_one() {
  if ! cmp -s "$1" "$2"; then
    echo "blob mismatch $1" >&2
    printf '%s\n' 2 > /evidence/blob-cmp.exit
    exit 2
  fi
}
while IFS= read -r rel; do
  cmp_one "$rel" "/blobs/$rel"
done < <(cd /blobs && find . -type f | sed 's|^\./||' | sort)
printf '%s\n' 0 > /evidence/blob-cmp.exit
if [ -f tests/ordinal_range.rs ] || [ -f /tool/tests/ordinal_range.rs ]; then
  echo "overlay present before smoke" >&2
  printf '%s\n' 2 > /evidence/overlay-absent.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/overlay-absent.exit

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
  printf '%s\n' not_run > /evidence/test.exit
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
  printf '%s\n' not_run > /evidence/test.exit
  exit "$fetch_exit"
fi

echo "=== pre-overlay cargo test ==="
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
  payload_budget_is_exact_and_the_next_event_is_not_stored
do
  if ! grep -q "$name" /evidence/smoke-test.stdout; then
    echo "smoke missing $name" >&2
    missing_name=1
  fi
done
printf '%s\n' "$missing_name" > /evidence/smoke-names.exit
if grep -q 'ordinal_range' /evidence/smoke-test.stdout; then
  echo "overlay name appeared in the pre-overlay smoke" >&2
  printf '%s\n' 2 > /evidence/overlay-absent.exit
  exit 2
fi
if [ "$smoke_exit" -ne 0 ] || [ "$smoke_count" -ne 27 ] || [ "$missing_name" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/test.exit
  if [ "$smoke_exit" -ne 0 ]; then
    exit "$smoke_exit"
  fi
  exit 1
fi

echo "=== ordinal overlay on the unfixed assembler ==="
cp /overlay/ordinal_range.rs /work/cdc-transaction/tests/ordinal_range.rs
if [ -f /tool/tests/ordinal_range.rs ]; then
  echo "overlay leaked into the archive" >&2
  printf '%s\n' 2 > /evidence/overlay-present.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/overlay-present.exit
sha256sum /overlay/ordinal_range.rs > /evidence/overlay.sha256
set +e
cargo test --locked --offline --test ordinal_range -- --test-threads=1 > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
set -e
printf '%s\n' "$test_exit" > /evidence/test.exit
echo "PHASE test_exit=$test_exit"
awk '/^test result:/ { print }' /evidence/test.stdout > /evidence/red-summary.txt
{
  sha256sum Cargo.toml Cargo.lock CONTRACT.md
  find src tests -type f ! -name 'ordinal_range.rs' | sort | xargs sha256sum
} > /evidence/src-after.txt
if cmp -s /evidence/src-before.txt /evidence/src-after.txt; then
  printf '%s\n' 0 > /evidence/product-unchanged.exit
else
  printf '%s\n' 2 > /evidence/product-unchanged.exit
  echo "product bytes changed inside the container" >&2
fi
sha256sum /rootlock/Cargo.lock | awk '{print $1}' > /evidence/root-lock.sha256
if [ "$test_exit" -eq 0 ]; then
  echo "ordinal test passed on the unfixed assembler; red was not reproduced" >&2
  printf '%s\n' unexpected_pass > /evidence/red-marker.txt
  exit 4
fi
printf '%s\n' expected_fail > /evidence/red-marker.txt
worst=$test_exit
if [ "$(cat /evidence/product-unchanged.exit)" -ne 0 ]; then
  worst=2
fi
echo "worst_phase_exit=$worst"
exit "$worst"
