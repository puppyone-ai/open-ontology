#!/bin/bash
# Live proof of sealed sql-runner 8f533140. The source mount is the git archive
# plus a toolchain rename. No overlay. No cargo update. No product edits.
# Origin pieces: a231 proof inside.sh, a222 libclang and query-pg --no-run.
# Delta: clippy on the sealed tree, fresh a244 suites, child scan before any
# kill, backend snapshot after the runner returns and before admin release.
set -euo pipefail
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1
export CARGO_PROFILE_DEV_DEBUG=0
cd /src

phase() {
  printf '%s\n' "$2" > "/evidence/phases/$1.exit"
  echo "PHASE $1=$2"
}

note_fail() {
  proof_exit=1
}

mkdir -p /evidence/phases /evidence/reports /evidence/attempts /evidence/admin \
  /evidence/built /evidence/live /cargo-home/bin /target/bins /tmp/runs /tmp/suite-out
printf '%s\n' no > /evidence/cargo-update-invoked.txt
proof_exit=0
NOEND_DIR=/tmp/runs/noend

echo "=== identity ==="
rustc -vV
cargo -V
rustup show
uname -a
printf '%s\n' "$(rustc -vV | awk '/^release:/{print $2}')" > /evidence/rustc-release.txt
if [ "$(cat /evidence/rustc-release.txt)" != "1.94.0" ]; then
  echo "rustc is not 1.94.0" >&2
  phase rustc 2
  exit 2
fi
phase rustc 0
if [ -f rust-toolchain.toml ] || [ ! -f rust-toolchain.toml.repo ]; then
  echo "toolchain rename is not in place" >&2
  phase toolchain 2
  exit 2
fi
phase toolchain 0

echo "=== tree ==="
diff -rq /archive /src > /evidence/tree-diff.txt || true
if ! grep -qx 'Only in /archive: rust-toolchain.toml' /evidence/tree-diff.txt \
  || ! grep -qx 'Only in /src: rust-toolchain.toml.repo' /evidence/tree-diff.txt
then
  echo "toolchain diff is missing" >&2
  phase tree 2
  exit 2
fi
if grep -v -e 'Only in /archive: rust-toolchain.toml' \
  -e 'Only in /src: rust-toolchain.toml.repo' \
  /evidence/tree-diff.txt | grep -q .; then
  echo "working tree is not the sealed archive plus the toolchain rename" >&2
  phase tree 2
  exit 2
fi
phase tree 0

echo "=== source hash ==="
(
  cd /src/benchmarks/sql-runner
  find . -type f ! -name '.DS_Store' | sort | xargs sha256sum
) > /evidence/source-sha256.txt
if ! ( cd /src/benchmarks/sql-runner && sha256sum -c /evidence/source-sha256.txt > /evidence/source-sha256.check 2>&1 ); then
  echo "source hash could not be checked" >&2
  phase source-check 2
  exit 2
fi
phase source-check 0

echo "=== lock pins ==="
cp /src/benchmarks/sql-runner/Cargo.lock /evidence/Cargo.lock
awk '
  function flush() {
    if (checksum != "") print name " " version " " checksum
    name = ""
    version = ""
    checksum = ""
  }
  /^\[\[package\]\]/ { flush(); next }
  $1 == "name" { gsub(/"/, "", $3); name = $3 }
  $1 == "version" { gsub(/"/, "", $3); version = $3 }
  $1 == "checksum" { gsub(/"/, "", $3); checksum = $3 }
  END { flush() }
' /src/Cargo.lock | sort > /tmp/root-pins.txt
awk '
  function flush() {
    if (checksum != "") print name " " version " " checksum
    name = ""
    version = ""
    checksum = ""
  }
  /^\[\[package\]\]/ { flush(); next }
  $1 == "name" { gsub(/"/, "", $3); name = $3 }
  $1 == "version" { gsub(/"/, "", $3); version = $3 }
  $1 == "checksum" { gsub(/"/, "", $3); checksum = $3 }
  END { flush() }
' /src/benchmarks/sql-runner/Cargo.lock | sort > /tmp/crate-pins.txt
comm -13 /tmp/root-pins.txt /tmp/crate-pins.txt > /evidence/lock-drift.txt
if [ -s /evidence/lock-drift.txt ]; then
  echo "sql-runner lock is not inside the fixed registry closure" >&2
  phase lock-drift 2
  exit 2
fi
phase lock-drift 0
grep -q 'serde 1.0.229 4148590afebada386688f18773da617792bf2ef03ffc1e4cbd2b1d45b023e0ba' /tmp/crate-pins.txt
grep -q 'serde_json 1.0.151 c841b55ecdae098c80dcae9cf767f6f8a0c2cdb3416bbef72181df4d0fe73f14' /tmp/crate-pins.txt
phase lock 0

fetch_locked() {
  local manifest="$1"
  local tag="$2"
  local code
  set +e
  cargo fetch --manifest-path "$manifest" --locked --offline > "/evidence/${tag}-fetch.stdout" 2> "/evidence/${tag}-fetch.stderr"
  code=$?
  set -e
  if [ "$code" -ne 0 ]; then
    echo "offline fetch missed for ${tag}; fetching locked" >&2
    set +e
    cargo fetch --manifest-path "$manifest" --locked > "/evidence/${tag}-fetch-online.stdout" 2> "/evidence/${tag}-fetch-online.stderr"
    code=$?
    set -e
  fi
  phase "${tag}-fetch" "$code"
  return "$code"
}

echo "=== fetch sql-runner ==="
if ! fetch_locked /src/benchmarks/sql-runner/Cargo.toml sql-runner; then
  exit 1
fi

echo "=== clippy ==="
set +e
cargo clippy --version > /evidence/clippy-version.txt 2> /evidence/clippy-version.stderr
clippy_ver=$?
set -e
if [ "$clippy_ver" -ne 0 ]; then
  set +e
  rustup component add clippy > /evidence/clippy-component.stdout 2> /evidence/clippy-component.stderr
  component_exit=$?
  set -e
  printf '%s\n' "$component_exit" > /evidence/clippy-component.exit
  if [ "$component_exit" -ne 0 ]; then
    phase clippy "$component_exit"
    note_fail
  fi
else
  printf '%s\n' 0 > /evidence/clippy-component.exit
fi
if [ ! -f /evidence/phases/clippy.exit ]; then
  set +e
  cargo clippy --manifest-path /src/benchmarks/sql-runner/Cargo.toml --locked --offline --all-targets -- -D warnings \
    > /evidence/clippy.stdout 2> /evidence/clippy.stderr
  clippy_exit=$?
  set -e
  phase clippy "$clippy_exit"
  if [ "$clippy_exit" -ne 0 ]; then
    note_fail
  fi
fi

echo "=== cargo test sql-runner ==="
set +e
cargo test --manifest-path /src/benchmarks/sql-runner/Cargo.toml --locked --offline -- --test-threads=1 \
  > /evidence/sql-runner-test.stdout 2> /evidence/sql-runner-test.stderr
test_exit=$?
set -e
phase sql-runner-test "$test_exit"
if [ "$test_exit" -ne 0 ]; then
  note_fail
fi

echo "=== hash and review tools ==="
rustc --edition 2021 -D unsafe_code /tools/sha256.rs -o /cargo-home/bin/sha256
phase sha256-tool $?
rustc --edition 2021 -D unsafe_code /tools/select-debs.rs -o /cargo-home/bin/select-debs
phase select-debs-tool $?
rustc --edition 2021 -D unsafe_code /tools/fill.rs -o /target/bins/fill
rustc --edition 2021 -D unsafe_code /tools/check.rs -o /target/bins/check
rustc --edition 2021 -D unsafe_code /tools/facts.rs -o /target/bins/facts
rustc --edition 2021 -D unsafe_code /tools/occupy.rs -o /target/bins/occupy
rustc --edition 2021 -D unsafe_code /tools/keep.rs -o /target/bins/keep
phase tools 0
export PATH="/cargo-home/bin:${PATH}"

echo "=== libclang ==="
mkdir -p /evidence/libclang
if [ ! -s /evidence/libclang/libclang-dir.txt ] || [ ! -d /cargo-home/a244-clang-root/usr/lib ]; then
  set +e
  bash /extract-libclang.sh > /evidence/libclang/extract.log 2>&1
  extract_exit=$?
  set -e
  phase libclang "$extract_exit"
  if [ "$extract_exit" -ne 0 ]; then
    exit "$extract_exit"
  fi
else
  echo "libclang_root=reused" > /evidence/libclang/extract.log
  phase libclang 0
fi
libdir=$(cat /evidence/libclang/libclang-dir.txt)
multiarch=/cargo-home/a244-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a244-clang-root/usr/lib/llvm-19/lib/clang/19/include
real=$(find /cargo-home/a244-clang-root/usr/lib -name 'libclang-19.so.19' -print -quit)
devso=$(find /cargo-home/a244-clang-root/usr/lib -name 'libclang.so' -print -quit)
if [ -n "$devso" ]; then
  libdir=$(dirname "$devso")
fi
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a244-clang-root/usr/share/pkgconfig"
{
  echo "LIBCLANG_PATH=$LIBCLANG_PATH"
  echo "LD_LIBRARY_PATH=$LD_LIBRARY_PATH"
  test -f "$llvm_inc/stdbool.h" && echo "stdbool.h=present" || echo "stdbool.h=absent"
} > /evidence/libclang/libclang.txt
if [ ! -f "$llvm_inc/stdbool.h" ] || [ -z "$real" ]; then
  phase libclang-load 1
  exit 1
fi
/lib/ld-linux-aarch64.so.1 --list "$real" > /evidence/libclang/libclang-ldd.txt 2>&1 || true
if grep -Eq 'not found|error while loading' /evidence/libclang/libclang-ldd.txt; then
  phase libclang-load 1
  exit 1
fi
phase libclang-load 0

echo "=== compile comparator, adapter, query-pg ==="
export CARGO_TARGET_DIR=/target/workspace
if ! fetch_locked /src/Cargo.toml workspace; then
  exit 1
fi
if ! fetch_locked /src/benchmarks/result-comparator/Cargo.toml comparator; then
  exit 1
fi
set +e
cargo build --manifest-path /src/benchmarks/result-comparator/Cargo.toml --locked --offline --bin result-comparator \
  > /evidence/comparator-build.stdout 2> /evidence/comparator-build.stderr
comparator_build=$?
set -e
phase comparator-build "$comparator_build"
if [ "$comparator_build" -ne 0 ]; then
  exit "$comparator_build"
fi
export CARGO_TARGET_DIR=/target/adapter
if ! fetch_locked /src/benchmarks/query-result-adapter/Cargo.toml adapter; then
  exit 1
fi
set +e
cargo build --manifest-path /src/benchmarks/query-result-adapter/Cargo.toml --locked --offline --bin query-result-adapter \
  > /evidence/adapter-build.stdout 2> /evidence/adapter-build.stderr
adapter_build=$?
set -e
phase adapter-build "$adapter_build"
if [ "$adapter_build" -ne 0 ]; then
  exit "$adapter_build"
fi
export CARGO_TARGET_DIR=/target/workspace
set +e
cargo test --manifest-path /src/Cargo.toml --locked --offline \
  -p oo-gateway --test query_pg_cli --no-run \
  > /evidence/norun.stdout 2> /evidence/norun.stderr
norun_exit=$?
set -e
phase norun "$norun_exit"
if [ "$norun_exit" -ne 0 ]; then
  exit "$norun_exit"
fi
exe=$(awk -F'[()]' '/Executable tests\/query_pg_cli.rs \(/ {print $2; exit}' /evidence/norun.stdout /evidence/norun.stderr)
exe=$(printf '%s' "$exe" | tr -d '[:space:]')
printf '%s\n' "$exe" > /evidence/query-pg-test-exe.txt
if [ ! -x "$exe" ] || [ ! -x /target/workspace/debug/query-pg ]; then
  echo "query-pg test binary missing" >&2
  phase bins 2
  exit 2
fi
cp -a /target/workspace/debug/query-pg /target/bins/query-pg
cp -a /target/adapter/debug/query-result-adapter /target/bins/query-result-adapter
cp -a /target/workspace/debug/result-comparator /target/bins/result-comparator
export CARGO_TARGET_DIR=/target
set +e
cargo build --manifest-path /src/benchmarks/sql-runner/Cargo.toml --locked --offline --bin sql-runner \
  > /evidence/sql-runner-build.stdout 2> /evidence/sql-runner-build.stderr
runner_build=$?
set -e
if [ "$runner_build" -ne 0 ]; then
  phase bins "$runner_build"
  exit "$runner_build"
fi
cp -a /target/debug/sql-runner /target/bins/sql-runner
wc -c /target/bins/query-pg /target/bins/query-result-adapter /target/bins/result-comparator /target/bins/sql-runner > /evidence/bin-sizes.txt
for bin in /target/bins/query-pg /target/bins/query-result-adapter /target/bins/result-comparator /target/bins/sql-runner; do
  size=$(wc -c < "$bin")
  if [ "$size" -gt 268435456 ]; then
    echo "executable exceeds the runner cap: $bin" >&2
    phase bins 2
    exit 2
  fi
done
phase bins 0
env -u LD_LIBRARY_PATH -u LIBCLANG_PATH /lib/ld-linux-aarch64.so.1 --list /target/bins/query-pg > /evidence/query-pg-ldd.txt 2>&1 || true
if grep -Eq 'not found|error while loading' /evidence/query-pg-ldd.txt; then
  echo "query-pg needs a library the cleared child environment cannot see" >&2
  phase ldd 1
  exit 1
fi
phase ldd 0

query_sha=$(/cargo-home/bin/sha256 file /target/bins/query-pg | awk '{print $1}')
adapter_sha=$(/cargo-home/bin/sha256 file /target/bins/query-result-adapter | awk '{print $1}')
comparator_sha=$(/cargo-home/bin/sha256 file /target/bins/result-comparator | awk '{print $1}')
runner_sha=$(/cargo-home/bin/sha256 file /target/bins/sql-runner | awk '{print $1}')
{
  echo "query_pg=$query_sha"
  echo "adapter=$adapter_sha"
  echo "comparator=$comparator_sha"
  echo "sql_runner=$runner_sha"
} > /evidence/bin-sha256.txt

children() {
  local d comm
  for d in /proc/[0-9]*; do
    [ -r "$d/cmdline" ] || continue
    comm=$(tr '\0' ' ' < "$d/cmdline" || true)
    case "$comm" in
      */target/bins/query-pg*|*/target/bins/query-result-adapter*|*/target/bins/result-comparator*|*/target/bins/sql-runner*|*/target/workspace/debug/query-pg*|*/target/adapter/debug/query-result-adapter*|*/target/workspace/debug/result-comparator*)
        printf '%s %s\n' "${d#/proc/}" "$comm"
        ;;
    esac
  done
}

scan_children() {
  local label="$1"
  local pid
  children > "/evidence/attempts/${label}-children.txt" || true
  if [ ! -s "/evidence/attempts/${label}-children.txt" ]; then
    return 0
  fi
  printf '%s\n' "$label" >> /evidence/attempts/child-leaks.txt
  echo "child still present after ${label}; recording before cleanup kill" >&2
  while read -r pid _; do
    [ -n "$pid" ] || continue
    kill "$pid" 2>/dev/null || true
  done < "/evidence/attempts/${label}-children.txt"
  sleep 1
  children > "/evidence/attempts/${label}-children-after.txt" || true
  return 1
}

withhold_live() {
  local src base
  mkdir -p /tmp/live-raw
  find /evidence/live -type f > /tmp/live-list.txt
  while IFS= read -r src; do
    [ -n "$src" ] || continue
    base=$(basename "$src")
    mv "$src" "/tmp/live-raw/$base"
  done < /tmp/live-list.txt
  for src in /tmp/live-raw/*; do
    [ -f "$src" ] || continue
    base=$(basename "$src")
    /target/bins/keep "$src" "/evidence/live/$base" /run/oo-a198/fixture/login.pass
  done
  rm -rf /tmp/live-raw /tmp/live-list.txt
}

echo "=== live query-pg ==="
set +e
env OO_A198_LIVE=1 "$exe" --exact live_query_pg_against_private_postgres --test-threads=1 --nocapture \
  > /evidence/live/test.stdout 2> /evidence/live/test.stderr
live_exit=$?
set -e
phase live "$live_exit"
if [ -f /evidence/live/oracle.txt ]; then
  awk -F= '$1 ~ /^(estimated_rows|desk_pending|desk_explicit_rows|star_columns|star_rows|count_star|peek_rows|truncate_bytes|truncate_rows|stdin_audit_lines|hidden_exit|hidden_audit|write_exit|write_audit|sleep_exit|sleep_audit|existing_audit_sessions|unwritable_parent_sessions|garbage_ca_sessions|invalid_ca_exit|invalid_ca_audit|done)$/ {print}' \
    /evidence/live/oracle.txt > /evidence/attempts/oracle-facts.txt
fi
withhold_live
if [ "$live_exit" -ne 0 ]; then
  exit "$live_exit"
fi
grep -q '^done=ok$' /evidence/live/oracle.txt
grep -q '^hidden_exit=6$' /evidence/live/oracle.txt
phase oracle 0
if ! scan_children live; then
  note_fail
fi

echo "=== suite documents ==="
trunc=$(awk -F= '$1=="truncate_bytes"{print $2; exit}' /evidence/live/oracle.txt)
printf '%s\n' "$trunc" > /evidence/truncate-bytes.txt
set +e
/target/bins/fill /hand /evidence/built \
  /target/bins/query-pg "$query_sha" \
  /target/bins/query-result-adapter "$adapter_sha" \
  /target/bins/result-comparator "$comparator_sha" \
  "$trunc" /run/oo-a198/fixture/login.pass \
  > /evidence/attempts/fill.stdout 2> /evidence/attempts/fill.stderr
fill_exit=$?
set -e
phase prepare "$fill_exit"
if [ "$fill_exit" -ne 0 ]; then
  exit "$fill_exit"
fi

wait_flag() {
  local flag="$1"
  local limit="$2"
  local i
  i=0
  while [ "$i" -lt "$limit" ]; do
    if [ -f "$flag" ]; then
      return 0
    fi
    i=$((i + 1))
    sleep 1
  done
  echo "timed out waiting for $flag" >&2
  return 1
}

copy_report() {
  local label="$1"
  local run="$2"
  local check_err="/tmp/suite-out/${label}.check.stderr"
  if [ -f "$check_err" ] && grep -q 'secret bytes present' "$check_err"; then
    /cargo-home/bin/sha256 file "/tmp/suite-out/${label}.stdout" > "/evidence/reports/${label}.withheld-sha256" 2>/dev/null || true
    printf '%s\n' 'withheld secret-bearing runner output' > "/evidence/reports/${label}.withheld.txt"
    return 1
  fi
  cp "/tmp/suite-out/${label}.stdout" "/evidence/reports/${label}.stdout"
  cp "/tmp/suite-out/${label}.stderr" "/evidence/reports/${label}.stderr"
  if [ -f "${run}/report.json" ]; then
    cp "${run}/report.json" "/evidence/reports/${label}.json"
  fi
  return 0
}

run_suite() {
  local name="$1"
  local label="$2"
  local want="$3"
  local run="$4"
  local code
  if [ -e "$run" ]; then
    echo "run dir already exists for $label" >&2
    return 2
  fi
  set +e
  /target/bins/sql-runner --suite "/evidence/built/suite-${name}.json" --run-dir "$run" \
    > "/tmp/suite-out/${label}.stdout" 2> "/tmp/suite-out/${label}.stderr"
  code=$?
  set -e
  printf '%s\n' "$code" > "/evidence/reports/${label}.exit"
  if ! scan_children "$label"; then
    note_fail
  fi
  set +e
  /target/bins/check "/tmp/suite-out/${label}.stdout" "/tmp/suite-out/${label}.stderr" \
    /run/oo-a198/fixture/login.pass "/expect/${name}.txt" \
    "$query_sha" "$adapter_sha" "$comparator_sha" \
    > "/tmp/suite-out/${label}.check.stdout" 2> "/tmp/suite-out/${label}.check.stderr"
  local check_code=$?
  set -e
  if ! copy_report "$label" "$run"; then
    printf '%s\n' "$check_code" > "/evidence/reports/${label}.check.exit"
    return 1
  fi
  cp "/tmp/suite-out/${label}.check.stdout" "/evidence/reports/${label}.check.stdout"
  cp "/tmp/suite-out/${label}.check.stderr" "/evidence/reports/${label}.check.stderr"
  printf '%s\n' "$check_code" > "/evidence/reports/${label}.check.exit"
  if [ "$code" -ne "$want" ]; then
    echo "runner exit $code want $want for $label" >&2
    return 1
  fi
  if [ ! -f "/evidence/reports/${label}.json" ]; then
    echo "report missing for $label" >&2
    return 1
  fi
  if ! cmp -s "/evidence/reports/${label}.stdout" "/evidence/reports/${label}.json"; then
    echo "stdout differs from report.json for $label" >&2
    return 1
  fi
  if [ "$check_code" -ne 0 ]; then
    echo "report check failed for $label" >&2
    return 1
  fi
  return 0
}

run_bound() {
  local name="$1"
  local label="$2"
  local want_line="$3"
  local run="/tmp/runs/${label}"
  local code
  set +e
  /target/bins/sql-runner --suite "/evidence/built/suite-${name}.json" --run-dir "$run" \
    > "/tmp/suite-out/${label}.stdout" 2> "/tmp/suite-out/${label}.stderr"
  code=$?
  set -e
  printf '%s\n' "$code" > "/evidence/attempts/${label}.exit"
  cp "/tmp/suite-out/${label}.stdout" "/evidence/attempts/${label}.stdout"
  cp "/tmp/suite-out/${label}.stderr" "/evidence/attempts/${label}.stderr"
  if ! scan_children "$label"; then
    note_fail
  fi
  printf '%s\n' "$want_line" > "/tmp/suite-out/${label}.want"
  local ok=0
  if [ "$code" -ne 2 ]; then
    ok=1
  fi
  if [ -s "/tmp/suite-out/${label}.stdout" ]; then
    ok=1
  fi
  if ! cmp -s "/tmp/suite-out/${label}.stderr" "/tmp/suite-out/${label}.want"; then
    ok=1
  fi
  if [ -e "$run" ]; then
    echo "config failure created a run dir for $label" >&2
    find "$run" -maxdepth 2 -type d > "/evidence/attempts/${label}-rundir.txt" || true
    ok=1
  else
    printf '%s\n' absent > "/evidence/attempts/${label}-rundir.txt"
  fi
  if [ "$ok" -ne 0 ]; then
    note_fail
    phase "$label" 1
    return 1
  fi
  phase "$label" 0
  return 0
}

echo "=== bounds ==="
run_bound bad-hash bad-hash 'sql-runner: hash /executables/query_pg/sha256' || true
run_bound gold-poison gold-poison 'sql-runner: gold /cases/0/request_file' || true

echo "=== hold table ==="
date -u +%Y-%m-%dT%H:%M:%SZ > /evidence/hold-needed
if ! wait_flag /evidence/hold-ready 90; then
  phase hold 1
  note_fail
else
  phase hold 0
fi

echo "=== suite pass ==="
if ! run_suite pass pass 0 /tmp/runs/pass; then
  note_fail
fi
phase pass "$(cat /evidence/reports/pass.exit 2>/dev/null || echo missing)"

echo "=== suite fail ==="
if ! run_suite fail fail 1 /tmp/runs/fail; then
  note_fail
fi
phase fail "$(cat /evidence/reports/fail.exit 2>/dev/null || echo missing)"

echo "=== suite peek ==="
if ! run_suite peek peek 1 /tmp/runs/peek; then
  note_fail
fi
phase peek "$(cat /evidence/reports/peek.exit 2>/dev/null || echo missing)"

echo "=== suite timeout ==="
date -u +%Y-%m-%dT%H:%M:%SZ > /evidence/timeout-needed
if ! wait_flag /evidence/timeout-ready 120; then
  echo "lock was not acquired" >&2
  note_fail
  phase timeout missing
else
  if ! run_suite timeout timeout 1 /tmp/runs/timeout; then
    note_fail
  fi
  phase timeout "$(cat /evidence/reports/timeout.exit 2>/dev/null || echo missing)"
fi
date -u +%Y-%m-%dT%H:%M:%SZ > /evidence/timeout-done
if ! wait_flag /evidence/after-runner-ready 60; then
  echo "backend snapshot after the runner was not recorded" >&2
  note_fail
fi
date -u +%Y-%m-%dT%H:%M:%SZ > /evidence/timeout-release
if ! wait_flag /evidence/timeout-released 60; then
  echo "lock release was not recorded" >&2
  note_fail
fi

echo "=== suite noend ==="
noend_once() {
  local label="$1"
  local run="$2"
  local trial="${run}/case-noend/trial-0"
  local occ=1
  local code=1
  local pid
  /target/bins/occupy "$trial" > "/evidence/attempts/${label}-occupy.stdout" 2> "/evidence/attempts/${label}-occupy.stderr" &
  pid=$!
  set +e
  run_suite noend "$label" 1 "$run"
  code=$?
  wait "$pid"
  occ=$?
  set -e
  printf '%s\n' "$occ" > "/evidence/attempts/${label}-occupy.exit"
  if [ -f "${trial}/audit.jsonl" ]; then
    local size
    size=$(wc -c < "${trial}/audit.jsonl")
    if [ "$size" -le 16 ]; then
      cp "${trial}/audit.jsonl" "/evidence/attempts/${label}-audit.jsonl"
    else
      printf 'withheld\nbytes=%s\n' "$size" > "/evidence/attempts/${label}-audit.jsonl"
    fi
  fi
  if [ "$code" -ne 0 ] || [ "$occ" -ne 0 ]; then
    return 1
  fi
  return 0
}
if noend_once noend /tmp/runs/noend; then
  NOEND_DIR=/tmp/runs/noend
  phase noend 0
elif noend_once noend-retry /tmp/runs/noend-retry; then
  echo "first missing-completion attempt missed; the retry met the expect" >&2
  NOEND_DIR=/tmp/runs/noend-retry
  phase noend 0
else
  echo "missing-completion did not meet the expect" >&2
  NOEND_DIR=/tmp/runs/noend-retry
  phase noend 1
  note_fail
fi

echo "=== facts ==="
set +e
/target/bins/facts /run/oo-a198/fixture/login.pass \
  /tmp/runs/pass /tmp/runs/fail /tmp/runs/peek /tmp/runs/timeout "$NOEND_DIR" \
  > /evidence/attempts/facts.txt 2> /evidence/attempts/facts.stderr
facts_exit=$?
set -e
phase facts "$facts_exit"
if [ "$facts_exit" -ne 0 ]; then
  note_fail
fi

echo "=== product unchanged ==="
diff -rq /archive /src > /evidence/tree-diff-after.txt || true
if ! grep -qx 'Only in /archive: rust-toolchain.toml' /evidence/tree-diff-after.txt \
  || ! grep -qx 'Only in /src: rust-toolchain.toml.repo' /evidence/tree-diff-after.txt \
  || grep -v -e 'Only in /archive: rust-toolchain.toml' \
    -e 'Only in /src: rust-toolchain.toml.repo' \
    /evidence/tree-diff-after.txt | grep -q .
then
  echo "product tree changed" >&2
  phase product 2
  note_fail
else
  phase product 0
fi
(
  cd /src/benchmarks/sql-runner
  find . -type f ! -name '.DS_Store' | sort | xargs sha256sum
) > /evidence/source-sha256.after
if ! cmp -s /evidence/source-sha256.txt /evidence/source-sha256.after; then
  echo "sql-runner sources changed during the proof" >&2
  phase source-after 2
  note_fail
else
  phase source-after 0
fi
phase proof "$proof_exit"
exit "$proof_exit"
