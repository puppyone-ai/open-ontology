#!/bin/bash
# Author session-release tests and Clippy on the immutable archive.
# No product overlay. No cargo update and no generate-lockfile.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1
export OO_A295_FIXTURE=/run/oo-a307/fixture
export OO_ROOT_LOCK=/work/Cargo.lock

echo "=== identity ==="
rustc -vV
echo "rustc_vv_exit=$?"
cargo -V
uname -a
id
command -v python3 >/dev/null 2>&1 && echo python3_present=1 || echo python3_present=0
command -v bash || true

echo "=== signal probe ==="
if [ ! -x /bin/bash ] && [ ! -x /usr/bin/bash ]; then
  echo "bash is missing" >&2
  printf '%s\n' 2 > /evidence/signal-probe.exit
  exit 2
fi
/bin/bash -c 'set -m; sleep 30 & p=$!; /bin/bash -c "kill -s \"\$1\" -- \"\$2\"" kill TERM "$p"; wait "$p"; echo "direct_term_pid=$p"'
/bin/bash -c 'set -m; sleep 30 & p=$!; sleep 0.2; target=$((0 - p)); /bin/bash -c "kill -s \"\$1\" -- \"\$2\"" kill TERM "$target"; sleep 0.2; if [ -d "/proc/$p" ]; then echo "group_alive=$p"; /bin/bash -c "kill -s \"\$1\" -- \"\$2\"" kill KILL "$p" || true; exit 2; fi; echo "group_stopped=$p"'
group_probe=$?
echo "group_probe_exit=$group_probe"
if [ "$group_probe" -ne 0 ]; then
  printf '%s\n' 2 > /evidence/signal-probe.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/signal-probe.exit

echo "=== archive gate ==="
if [ ! -f /src/tools/session-transition/src/reduce.rs ]; then
  echo "fixed archive missing the reducer" >&2
  printf '%s\n' 2 > /evidence/archive.exit
  exit 2
fi
if [ ! -f /src/tools/session-store/src/store.rs ]; then
  echo "fixed archive missing session-store" >&2
  printf '%s\n' 2 > /evidence/archive.exit
  exit 2
fi
if [ ! -f /src/tools/session-release/src/release.rs ]; then
  echo "fixed archive missing session-release" >&2
  printf '%s\n' 2 > /evidence/archive.exit
  exit 2
fi
if [ ! -f /src/Cargo.lock ]; then
  echo "fixed archive missing the root lock" >&2
  printf '%s\n' 2 > /evidence/archive.exit
  exit 2
fi
if [ -e /overlay ]; then
  echo "product overlay must not be mounted" >&2
  printf '%s\n' 2 > /evidence/archive.exit
  exit 2
fi
if [ ! -f /run/oo-a307/fixture/endpoint.txt ] || [ ! -f /run/oo-a307/fixture/ca.crt ]; then
  echo "fixture endpoint is missing" >&2
  printf '%s\n' 2 > /evidence/archive.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/archive.exit

echo "=== rustc version gate ==="
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

rm -rf /work
mkdir -p /work
cp -a /src/. /work/
find /work/tools/session-release /work/tools/session-store /work/tools/session-transition /work/Cargo.lock -type f | sort | xargs sha256sum > /evidence/source-sha256.txt

echo "=== lock triples ==="
rustc --edition 2024 -C debuginfo=0 -o /target/lock_triples /review/lock_triples.rs > /evidence/lock-triples-build.stdout 2> /evidence/lock-triples-build.stderr
build_exit=$?
printf '%s\n' "$build_exit" > /evidence/lock-triples-build.exit
echo "PHASE lock_triples_build_exit=$build_exit"
if [ "$build_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/test.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  exit "$build_exit"
fi
set +e
/target/lock_triples \
  /work/tools/session-release/Cargo.lock \
  /work/tools/session-store/Cargo.lock \
  /work/tools/session-transition/Cargo.lock \
  /work/Cargo.lock \
  > /evidence/lock-triples.stdout 2> /evidence/lock-triples.stderr
lock_exit=$?
set -e
printf '%s\n' "$lock_exit" > /evidence/lock-triples.exit
echo "PHASE lock_triples_exit=$lock_exit"
if [ "$lock_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/test.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  exit "$lock_exit"
fi

echo "=== clippy component ==="
set +e
rustup component add clippy > /evidence/clippy-component.stdout 2> /evidence/clippy-component.stderr
component_exit=$?
set -e
printf '%s\n' "$component_exit" > /evidence/clippy-component.exit
echo "PHASE clippy_component_exit=$component_exit"
if [ "$component_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/test.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  exit "$component_exit"
fi

cd /work/tools/session-release
export CARGO_NET_OFFLINE=true

echo "=== cargo test locked offline ==="
set +e
cargo test --locked --offline --all-targets -- --test-threads=1 > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
set -e
printf '%s\n' "$test_exit" > /evidence/test.exit
echo "PHASE test_exit=$test_exit"
if [ "$test_exit" -ne 0 ] && grep -Eq 'failed to download|no matching package|unable to update|couldn.t fetch|offline mode' /evidence/test.stderr; then
  echo "=== cargo fetch locked once ==="
  set +e
  env -u CARGO_NET_OFFLINE cargo fetch --locked > /evidence/fetch.stdout 2> /evidence/fetch.stderr
  fetch_exit=$?
  set -e
  printf '%s\n' "$fetch_exit" > /evidence/fetch.exit
  echo "PHASE fetch_exit=$fetch_exit"
  if [ "$fetch_exit" -ne 0 ]; then
    printf '%s\n' not_run > /evidence/clippy.exit
    exit "$fetch_exit"
  fi
  cp /evidence/test.stdout /evidence/test-offline-attempt.stdout
  cp /evidence/test.stderr /evidence/test-offline-attempt.stderr
  cp /evidence/test.exit /evidence/test-offline-attempt.exit
  export CARGO_NET_OFFLINE=true
  set +e
  cargo test --locked --offline --all-targets -- --test-threads=1 > /evidence/test.stdout 2> /evidence/test.stderr
  test_exit=$?
  set -e
  printf '%s\n' "$test_exit" > /evidence/test.exit
  echo "PHASE test_retry_exit=$test_exit"
fi

echo "=== cargo clippy locked offline ==="
set +e
cargo clippy --locked --offline --all-targets -- -D warnings > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"

echo "=== unchanged archive ==="
changed=0
cd /src
find . -type f | sort | while IFS= read -r rel; do
  left=$(sha256sum "$rel" | awk '{print $1}')
  right=$(sha256sum "/work/$rel" | awk '{print $1}')
  if [ "$left" != "$right" ]; then
    echo "changed $rel"
    changed=1
  fi
done > /evidence/unchanged.txt
if grep -q '^changed ' /evidence/unchanged.txt; then
  printf '%s\n' 1 > /evidence/unchanged.exit
else
  printf '%s\n' 0 > /evidence/unchanged.exit
fi
echo "PHASE unchanged_exit=$(cat /evidence/unchanged.exit)"

grep -E '^test |^running |^test result:' /evidence/test.stdout > /evidence/test-names.txt || true

if [ "$test_exit" -ne 0 ]; then
  exit "$test_exit"
fi
if [ "$(cat /evidence/unchanged.exit)" != 0 ]; then
  exit 1
fi
exit "$clippy_exit"
