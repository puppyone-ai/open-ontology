#!/bin/bash
# Pre-overlay smoke of the sealed 15 tests, then the independent overlay.
# Copy uses cp -R. The archive stays read-only. No lock generation.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1
export PATH=/usr/local/cargo/bin:/usr/local/bin:$PATH

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
printf '%s\n' 0 > /evidence/version.exit

rm -rf /work/src /work/tests /work/Cargo.toml /work/Cargo.lock /work/CONTRACT.md
mkdir -p /work/src /work/tests
cp /src/tools/session-transition/Cargo.toml /src/tools/session-transition/CONTRACT.md /work/
cp -R /src/tools/session-transition/src/. /work/src/
cp -R /src/tools/session-transition/tests/. /work/tests/
if [ ! -f /src/tools/session-transition/Cargo.lock ]; then
  echo "archive is missing Cargo.lock" >&2
  printf '%s\n' missing > /evidence/lock.exit
  exit 2
fi
cp /src/tools/session-transition/Cargo.lock /work/Cargo.lock
printf '%s\n' present > /evidence/lock.exit
cd /work
find Cargo.toml Cargo.lock CONTRACT.md src tests -type f | sort | xargs sha256sum > /evidence/source-sha256.txt
find src -type f | sort | xargs sha256sum > /evidence/src-before.txt

python_absent=0
if command -v python3 >/dev/null 2>&1 || command -v python >/dev/null 2>&1; then
  python_absent=1
fi
printf '%s\n' "$python_absent" > /evidence/python-present.flag
echo "python_present_flag=$python_absent"

cmp_one() {
  if ! cmp -s "$1" "$2"; then
    echo "blob mismatch $1" >&2
    printf '%s\n' 2 > /evidence/blob-cmp.exit
    exit 2
  fi
}
cmp_one src/lib.rs /blobs/src/lib.rs
cmp_one src/main.rs /blobs/src/main.rs
cmp_one src/model.rs /blobs/src/model.rs
cmp_one src/reduce.rs /blobs/src/reduce.rs
cmp_one src/sha256.rs /blobs/src/sha256.rs
cmp_one tests/cli_roundtrip.rs /blobs/tests/cli_roundtrip.rs
cmp_one tests/handcomputed.rs /blobs/tests/handcomputed.rs
cmp_one Cargo.toml /blobs/Cargo.toml
cmp_one Cargo.lock /blobs/Cargo.lock
cmp_one CONTRACT.md /blobs/CONTRACT.md
printf '%s\n' 0 > /evidence/blob-cmp.exit
if [ -f tests/a272_handcomputed.rs ] || [ -f /src/tools/session-transition/tests/a272_handcomputed.rs ]; then
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
/usr/local/bin/lock_subset /work/Cargo.lock /src/Cargo.lock > /evidence/lock-subset.stdout 2> /evidence/lock-subset.stderr
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
grep -q 'summary registry_triples=11 missing=0 mismatch=0 direct_pins_present=true' /evidence/lock-subset.stdout || {
  echo "registry summary is not 11/0/0" >&2
  printf '%s\n' 2 > /evidence/lock-summary.exit
  lock_exit=2
}
if [ ! -f /evidence/lock-summary.exit ]; then
  printf '%s\n' 0 > /evidence/lock-summary.exit
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
  printf '%s\n' not_run > /evidence/clippy-component.exit
  printf '%s\n' not_run > /evidence/clippy.exit
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
  known_answers \
  cli_roundtrip_reports_the_post_state_revision \
  cli_rejects_trailing_json_and_arguments \
  cancel_then_begin_turn_is_stopped \
  changed_actor_beats_stale_and_keeps_workspace \
  full_journal_does_not_drop_and_still_replays \
  happy_path_revisions_are_hand_counted \
  identical_replay_is_idempotent_and_conflict_refuses \
  overflow_does_not_wrap \
  reason_names_and_bounds \
  releasing_stops_admission_and_close_needs_s8_absence \
  stale_revision_and_generation_leave_the_snapshot \
  takeover_heartbeat_and_turn_timeout \
  time_boundaries_refuse_at_the_exact_instant \
  tool_sixty_is_the_last_admitted_start
do
  if ! grep -q "$name" /evidence/smoke-test.stdout; then
    echo "smoke missing $name" >&2
    missing_name=1
  fi
done
printf '%s\n' "$missing_name" > /evidence/smoke-names.exit
if [ "$smoke_exit" -ne 0 ] || [ "$smoke_count" -ne 15 ] || [ "$missing_name" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/test.exit
  printf '%s\n' not_run > /evidence/clippy-component.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  if [ "$smoke_exit" -ne 0 ]; then
    exit "$smoke_exit"
  fi
  exit 1
fi
if grep -q 'a272_handcomputed' /evidence/smoke-test.stdout; then
  echo "overlay name appeared in the pre-overlay smoke" >&2
  printf '%s\n' 2 > /evidence/overlay-absent.exit
  exit 2
fi

echo "=== overlay independent tests ==="
cp /overlay/a272_handcomputed.rs /work/tests/a272_handcomputed.rs
if [ -f /src/tools/session-transition/tests/a272_handcomputed.rs ]; then
  echo "overlay leaked into the archive" >&2
  printf '%s\n' 2 > /evidence/overlay-present.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/overlay-present.exit
set +e
cargo test --locked --offline --test a272_handcomputed -- --test-threads=1 > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
set -e
printf '%s\n' "$test_exit" > /evidence/test.exit
echo "PHASE test_exit=$test_exit"

echo "=== rustup component add clippy ==="
set +e
rustup component add clippy --toolchain 1.94.0 > /evidence/clippy-component.stdout 2> /evidence/clippy-component.stderr
component_exit=$?
set -e
printf '%s\n' "$component_exit" > /evidence/clippy-component.exit
echo "PHASE clippy_component_exit=$component_exit"
if [ "$component_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/clippy.exit
  worst=$component_exit
  if [ "$test_exit" -ne 0 ]; then worst=$test_exit; fi
  if [ "$lock_exit" -ne 0 ]; then worst=$lock_exit; fi
  exit "$worst"
fi

echo "=== cargo clippy ==="
set +e
cargo clippy --locked --offline --all-targets -- -D warnings > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"

find src -type f | sort | xargs sha256sum > /evidence/src-after.txt
if cmp -s /evidence/src-before.txt /evidence/src-after.txt && cmp -s Cargo.lock /blobs/Cargo.lock; then
  printf '%s\n' 0 > /evidence/product-unchanged.exit
else
  printf '%s\n' 2 > /evidence/product-unchanged.exit
  echo "product bytes changed inside the container" >&2
fi
cp Cargo.lock /evidence/Cargo.lock

worst=0
if [ "$lock_exit" -ne 0 ]; then worst=$lock_exit; fi
if [ "$test_exit" -ne 0 ]; then worst=$test_exit; fi
if [ "$clippy_exit" -ne 0 ]; then worst=$clippy_exit; fi
if [ "$(cat /evidence/product-unchanged.exit)" -ne 0 ]; then worst=2; fi
echo "worst_phase_exit=$worst"
exit "$worst"
