#!/bin/bash
# Calendar-store review container. MODE is smoke, overlay, or persist.
# smoke runs the author tests before any review overlay.
# overlay runs the author restart read, then the independent test, then clippy.
# persist runs only the post-restart read.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1
export PATH=/usr/local/cargo/bin:/usr/local/bin:$PATH
MODE=${MODE:?mode}

echo "=== identity ==="
rustc -vV | tee /evidence/rustc-version.txt
echo "rustc_vv_exit=$?"
cargo -V | tee /evidence/cargo-version.txt
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
printf '%s\n' no > /evidence/cargo-update-invoked.txt

python_absent=0
if command -v python3 >/dev/null 2>&1 || command -v python >/dev/null 2>&1; then
  python_absent=1
fi
printf '%s\n' "$python_absent" > /evidence/python-present.flag

rm -rf /work/repo
mkdir -p /work/repo
cp -R /src/crates /work/repo/crates
cp -R /src/tools /work/repo/tools
mkdir -p /work/repo/examples/schema
cp /src/examples/schema/calendar.yaml /work/repo/examples/schema/calendar.yaml
cp /src/Cargo.lock /work/repo/Cargo.lock
cd /work/repo/tools/calendar-store

if [ -f tests/a297_calendar.rs ] || [ -f /src/tools/calendar-store/tests/a297_calendar.rs ]; then
  echo "overlay present in the archive" >&2
  printf '%s\n' 2 > /evidence/overlay-absent.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/overlay-absent.exit

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
cmp_one migrations/oo_calendar_control_v1.sql /blobs/migrations/oo_calendar_control_v1.sql
cmp_one src/error.rs /blobs/src/error.rs
cmp_one src/lib.rs /blobs/src/lib.rs
cmp_one src/publish.rs /blobs/src/publish.rs
cmp_one src/role.rs /blobs/src/role.rs
cmp_one src/sql.rs /blobs/src/sql.rs
cmp_one src/store.rs /blobs/src/store.rs
cmp_one src/tls.rs /blobs/src/tls.rs
cmp_one tests/contract.rs /blobs/tests/contract.rs
cmp_one tests/live.rs /blobs/tests/live.rs
cmp_one /work/repo/examples/schema/calendar.yaml /src/examples/schema/calendar.yaml
printf '%s\n' 0 > /evidence/blob-cmp.exit

find src migrations tests/contract.rs tests/live.rs Cargo.toml Cargo.lock CONTRACT.md -type f | sort > /tmp/product-files.txt
sort -u /tmp/product-files.txt | xargs sha256sum > /evidence/src-before.txt
sha256sum Cargo.lock > /evidence/lock-before.sha256
sha256sum /work/repo/Cargo.lock > /evidence/root-lock.sha256

run_lock() {
  echo "=== registry triples ==="
  set +e
  rustc --edition 2024 -D warnings /tools/lock_subset.rs -o /usr/local/bin/lock_subset
  lock_compile=$?
  set -e
  printf '%s\n' "$lock_compile" > /evidence/lock-compile.exit
  if [ "$lock_compile" -ne 0 ]; then
    echo "lock_subset did not compile" >&2
    exit "$lock_compile"
  fi
  set +e
  /usr/local/bin/lock_subset /work/repo/tools/calendar-store/Cargo.lock /work/repo/Cargo.lock > /evidence/lock-subset.stdout 2> /evidence/lock-subset.stderr
  lock_exit=$?
  set -e
  printf '%s\n' "$lock_exit" > /evidence/lock-subset.exit
  echo "PHASE lock_subset_exit=$lock_exit"
  awk '$1=="triple" { c++ } END { print c+0 }' /evidence/lock-subset.stdout > /evidence/lock-triple-count.txt
  if ! grep -q 'summary registry_triples=208 missing=0 mismatch=0 direct_pins_present=true helper_pins_present=true path_packages=3' /evidence/lock-subset.stdout; then
    echo "registry summary is not 208/0/0 with three path packages" >&2
    printf '%s\n' 2 > /evidence/lock-summary.exit
    lock_exit=2
  elif [ "$(cat /evidence/lock-triple-count.txt)" != "208" ]; then
    echo "triple line count is not 208" >&2
    printf '%s\n' 2 > /evidence/lock-summary.exit
    lock_exit=2
  else
    printf '%s\n' 0 > /evidence/lock-summary.exit
  fi
  if [ "$lock_exit" -ne 0 ]; then
    exit "$lock_exit"
  fi
}

run_fetch() {
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
    exit "$fetch_exit"
  fi
}

finish_unchanged() {
  sort -u /tmp/product-files.txt | xargs sha256sum > /evidence/src-after.txt
  sha256sum Cargo.lock > /evidence/lock-after.sha256
  if cmp -s /evidence/src-before.txt /evidence/src-after.txt && cmp -s /evidence/lock-before.sha256 /evidence/lock-after.sha256 && cmp -s Cargo.lock /blobs/Cargo.lock; then
    printf '%s\n' 0 > /evidence/product-unchanged.exit
  else
    printf '%s\n' 2 > /evidence/product-unchanged.exit
    echo "product bytes changed inside the container" >&2
    exit 2
  fi
  cp Cargo.lock /evidence/Cargo.lock
}

if [ "$MODE" = "smoke" ] || [ "$MODE" = "overlay" ]; then
  run_lock
  run_fetch
fi

if [ "$MODE" = "smoke" ]; then
  echo "=== pre-overlay author tests ==="
  set +e
  cargo test --locked --offline -- --test-threads=1 --skip restart_snapshot_persists > /evidence/smoke-test.stdout 2> /evidence/smoke-test.stderr
  smoke_exit=$?
  set -e
  printf '%s\n' "$smoke_exit" > /evidence/smoke-test.exit
  echo "PHASE smoke_exit=$smoke_exit"
  awk '/^test result:/ { passed += $4 } END { print passed+0 }' /evidence/smoke-test.stdout > /evidence/smoke-count.txt
  missing_name=0
  for name in \
    migration_does_not_autofix_or_read_a_clock \
    blocked_midnights_keep_the_date_and_drop_the_instant \
    role_grammar_rejects_injection_and_fixed_roles \
    contract_name_and_migration_hash_are_fixed \
    sources_do_not_read_a_clock_or_publish_private_fields \
    fresh_pg_contract
  do
    if ! grep -q "$name" /evidence/smoke-test.stdout; then
      echo "smoke missing $name" >&2
      missing_name=1
    fi
  done
  printf '%s\n' "$missing_name" > /evidence/smoke-names.exit
  if grep -q 'a297_calendar' /evidence/smoke-test.stdout; then
    echo "overlay name appeared in the pre-overlay smoke" >&2
    printf '%s\n' 2 > /evidence/overlay-absent.exit
    exit 2
  fi
  finish_unchanged
  smoke_count=$(cat /evidence/smoke-count.txt)
  echo "smoke_passed_count=$smoke_count"
  if [ "$smoke_exit" -ne 0 ] || [ "$smoke_count" -ne 6 ] || [ "$missing_name" -ne 0 ]; then
    if [ "$smoke_exit" -ne 0 ]; then
      exit "$smoke_exit"
    fi
    exit 1
  fi
  exit 0
fi

if [ "$MODE" = "overlay" ]; then
  echo "=== author restart read, still before the overlay ==="
  set +e
  cargo test --locked --offline --test live restart_snapshot_persists -- --test-threads=1 --exact > /evidence/author-restart.stdout 2> /evidence/author-restart.stderr
  author_exit=$?
  set -e
  printf '%s\n' "$author_exit" > /evidence/author-restart.exit
  echo "PHASE author_restart_exit=$author_exit"
  if [ "$author_exit" -ne 0 ]; then
    printf '%s\n' not_run > /evidence/test.exit
    printf '%s\n' not_run > /evidence/clippy.exit
    finish_unchanged
    exit "$author_exit"
  fi
  if [ -f /src/tools/calendar-store/tests/a297_calendar.rs ]; then
    echo "overlay leaked into the archive" >&2
    printf '%s\n' 2 > /evidence/overlay-present.exit
    exit 2
  fi
  cp /overlay/a297_calendar.rs /work/repo/tools/calendar-store/tests/a297_calendar.rs
  printf '%s\n' 0 > /evidence/overlay-present.exit
  echo "=== independent overlay ==="
  set +e
  cargo test --locked --offline --test a297_calendar independent_store -- --test-threads=1 --exact --nocapture > /evidence/test.stdout 2> /evidence/test.stderr
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
    finish_unchanged
    exit "$component_exit"
  fi
  echo "=== cargo clippy ==="
  set +e
  cargo clippy --locked --offline --all-targets -- -D warnings > /evidence/clippy.stdout 2> /evidence/clippy.stderr
  clippy_exit=$?
  set -e
  printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
  echo "PHASE clippy_exit=$clippy_exit"
  finish_unchanged
  overlay_count=$(cat /evidence/overlay-count.txt)
  echo "overlay_passed_count=$overlay_count"
  if [ "$test_exit" -ne 0 ]; then
    exit "$test_exit"
  fi
  if [ "$clippy_exit" -ne 0 ]; then
    exit "$clippy_exit"
  fi
  if [ "$overlay_count" -ne 1 ]; then
    echo "overlay passed count is $overlay_count" >&2
    exit 1
  fi
  exit 0
fi

if [ "$MODE" = "persist" ]; then
  run_fetch
  cp /overlay/a297_calendar.rs /work/repo/tools/calendar-store/tests/a297_calendar.rs
  if [ -f /src/tools/calendar-store/tests/a297_calendar.rs ]; then
    echo "overlay leaked into the archive" >&2
    printf '%s\n' 2 > /evidence/overlay-present.exit
    exit 2
  fi
  printf '%s\n' 0 > /evidence/overlay-present.exit
  echo "=== post-restart read ==="
  set +e
  cargo test --locked --offline --test a297_calendar restart_keeps_the_explicit_row -- --test-threads=1 --exact --nocapture > /evidence/test.stdout 2> /evidence/test.stderr
  test_exit=$?
  set -e
  printf '%s\n' "$test_exit" > /evidence/test.exit
  echo "PHASE persist_exit=$test_exit"
  awk '/^test result:/ { passed += $4 } END { print passed+0 }' /evidence/test.stdout > /evidence/overlay-count.txt
  finish_unchanged
  if [ "$test_exit" -ne 0 ]; then
    exit "$test_exit"
  fi
  if [ "$(cat /evidence/overlay-count.txt)" != "1" ]; then
    echo "persist passed count is not 1" >&2
    exit 1
  fi
  exit 0
fi

echo "unknown mode $MODE" >&2
exit 2
