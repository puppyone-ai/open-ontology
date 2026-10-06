#!/bin/bash
# Lock subset first, then fetch --locked, then the author 23-test smoke, then the overlay.
# The committed session-store lock is not generated or updated.
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
if ! rustc -vV | grep -q '^host: aarch64-unknown-linux-gnu$'; then
  echo "host is not linux arm64" >&2
  printf '%s\n' 2 > /evidence/version.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/version.exit
printf '%s\n' 'committed lock; cargo fetch --locked; no generate-lockfile; no cargo update' > /evidence/lock-recipe.txt

rm -rf /work/session-store /work/session-transition
mkdir -p /work
cp -R /src/tools/session-transition /work/session-transition
cp -R /src/tools/session-store /work/session-store
cd /work/session-store
find Cargo.toml Cargo.lock CONTRACT.md migrations src tests -type f | sort | xargs sha256sum > /evidence/source-sha256.txt
find src migrations -type f | sort > /tmp/product-files.txt
printf '%s\n' Cargo.toml Cargo.lock CONTRACT.md tests/live.rs >> /tmp/product-files.txt
sort -u /tmp/product-files.txt | xargs sha256sum > /evidence/src-before.txt

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
cmp_one Cargo.toml /blobs/Cargo.toml
cmp_one Cargo.lock /blobs/Cargo.lock
cmp_one CONTRACT.md /blobs/CONTRACT.md
cmp_one migrations/oo_session_control_v1.sql /blobs/migrations/oo_session_control_v1.sql
cmp_one src/error.rs /blobs/src/error.rs
cmp_one src/lib.rs /blobs/src/lib.rs
cmp_one src/main.rs /blobs/src/main.rs
cmp_one src/sha256.rs /blobs/src/sha256.rs
cmp_one src/sql.rs /blobs/src/sql.rs
cmp_one src/store.rs /blobs/src/store.rs
cmp_one src/tls.rs /blobs/src/tls.rs
cmp_one tests/live.rs /blobs/tests/live.rs
printf '%s\n' 0 > /evidence/blob-cmp.exit
if [ ! -f Cargo.lock ]; then
  echo "committed Cargo.lock is absent" >&2
  printf '%s\n' missing > /evidence/lock.exit
  exit 2
fi
printf '%s\n' present > /evidence/lock.exit
if [ -f tests/a281_session.rs ] || [ -f /src/tools/session-store/tests/a281_session.rs ]; then
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
/usr/local/bin/lock_subset /work/session-store/Cargo.lock /src/Cargo.lock > /evidence/lock-subset.stdout 2> /evidence/lock-subset.stderr
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
if ! grep -q 'summary registry_triples=128 missing=0 mismatch=0 direct_pins_present=true path_packages=2' /evidence/lock-subset.stdout; then
  echo "registry summary is not 128/0/0 with two path packages" >&2
  printf '%s\n' 2 > /evidence/lock-summary.exit
  lock_exit=2
else
  printf '%s\n' 0 > /evidence/lock-summary.exit
fi
if ! grep -q '^path session-store 0.1.0$' /evidence/lock-subset.stdout || ! grep -q '^path session-transition 0.1.0$' /evidence/lock-subset.stdout; then
  echo "path allowlist lines missing" >&2
  printf '%s\n' 2 > /evidence/lock-summary.exit
  lock_exit=2
fi
if [ "$lock_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/fetch.exit
  printf '%s\n' not_run > /evidence/smoke-test.exit
  printf '%s\n' not_run > /evidence/test.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  exit "$lock_exit"
fi

echo "=== cargo fetch locked ==="
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
awk '/^test result:/ { passed += $4; failed += $6 } END { print passed+0 }' /evidence/smoke-test.stdout > /evidence/smoke-count.txt
smoke_count=$(cat /evidence/smoke-count.txt)
echo "smoke_passed_count=$smoke_count"
missing_name=0
for name in \
  install_replay_drift_partial_and_privileges \
  wrong_ca_does_not_connect \
  two_connections_race_one_winner \
  rollback_before_commit_leaves_the_row \
  new_connection_replays_and_conflict_refuses \
  commit_unknown_reconciles_without_a_second_transition \
  owner_lease_takeover_keeps_the_workspace \
  changed_principal_new_session_and_project_separation \
  project_key_and_principal_are_not_authentication \
  column_json_mismatch_is_not_repaired \
  cli_acks_refusals_and_hides_the_password \
  product_does_not_read_the_fixture_directory
do
  if ! grep -q "$name" /evidence/smoke-test.stdout; then
    echo "smoke missing $name" >&2
    missing_name=1
  fi
done
printf '%s\n' "$missing_name" > /evidence/smoke-names.exit
if [ "$smoke_exit" -ne 0 ] || [ "$smoke_count" -ne 23 ] || [ "$missing_name" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/test.exit
  printf '%s\n' not_run > /evidence/clippy-component.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  if [ "$smoke_exit" -ne 0 ]; then
    exit "$smoke_exit"
  fi
  exit 1
fi
if grep -q 'a281_session' /evidence/smoke-test.stdout; then
  echo "overlay name appeared in the pre-overlay smoke" >&2
  printf '%s\n' 2 > /evidence/overlay-absent.exit
  exit 2
fi

echo "=== overlay independent tests ==="
cp /overlay/a281_session.rs /work/session-store/tests/a281_session.rs
if [ -f /src/tools/session-store/tests/a281_session.rs ]; then
  echo "overlay leaked into the archive" >&2
  printf '%s\n' 2 > /evidence/overlay-present.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/overlay-present.exit
set +e
cargo test --locked --offline --test a281_session -- --test-threads=1 --nocapture > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
set -e
printf '%s\n' "$test_exit" > /evidence/test.exit
echo "PHASE test_exit=$test_exit"
awk '/^test result:/ { passed += $4 } END { print passed+0 }' /evidence/test.stdout > /evidence/overlay-count.txt

echo "=== rustup component add clippy ==="
set +e
rustup component add clippy --toolchain 1.94.0 > /evidence/clippy-component.stdout 2> /evidence/clippy-component.stderr
component_exit=$?
set -e
printf '%s\n' "$component_exit" > /evidence/clippy-component.exit
echo "PHASE clippy_component_exit=$component_exit"
if [ "$component_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/clippy.exit
  exit "$component_exit"
fi

echo "=== cargo clippy ==="
set +e
cargo clippy --locked --offline --all-targets -- -D warnings > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"

sort -u /tmp/product-files.txt | xargs sha256sum > /evidence/src-after.txt
if cmp -s /evidence/src-before.txt /evidence/src-after.txt && cmp -s Cargo.lock /blobs/Cargo.lock; then
  printf '%s\n' 0 > /evidence/product-unchanged.exit
else
  printf '%s\n' 2 > /evidence/product-unchanged.exit
  echo "product bytes changed inside the container" >&2
fi
cp Cargo.lock /evidence/Cargo.lock

overlay_count=$(cat /evidence/overlay-count.txt)
echo "overlay_passed_count=$overlay_count"
worst=0
if [ "$test_exit" -ne 0 ]; then worst=$test_exit; fi
if [ "$clippy_exit" -ne 0 ]; then worst=$clippy_exit; fi
if [ "$overlay_count" -ne 8 ]; then
  echo "overlay passed count is $overlay_count" >&2
  worst=1
fi
if [ "$(cat /evidence/product-unchanged.exit)" -ne 0 ]; then worst=2; fi
echo "worst_phase_exit=$worst"
exit "$worst"
