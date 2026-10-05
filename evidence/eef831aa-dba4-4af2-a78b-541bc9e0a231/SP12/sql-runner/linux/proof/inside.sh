#!/bin/bash
# Live proof for sql-runner. Fixed archive plus the sql-runner overlay only.
# Builds query-pg, the adapter, and the comparator. Does not test or clippy those
# two crates. Does not cargo update. Does not install the a222 tee wrapper.
# Origin pieces: a222 proof inside.sh libclang and query-pg --no-run sequence,
# a231 impl-2 for the sql-runner lock, a199 sha256.rs for executable digests.
set -euo pipefail
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1
cd /src

phase() {
  printf '%s\n' "$2" > "/evidence/phases/$1.exit"
  echo "PHASE $1=$2"
}

mkdir -p /evidence/phases /evidence/reports /evidence/attempts /evidence/admin \
  /cargo-home/bin /target/bins /target/adapter /target/workspace /tmp/runs
printf '%s\n' no > /evidence/cargo-update-invoked.txt

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
if [ -f rust-toolchain.toml ]; then
  echo "repo toolchain file still active" >&2
  phase toolchain 2
  exit 2
fi
phase toolchain 0

echo "=== tree ==="
diff -rq /archive /src > /evidence/tree-diff.txt || true
if grep -v -e 'Only in /archive: rust-toolchain.toml' \
  -e 'Only in /src: rust-toolchain.toml.repo' \
  -e 'Only in /src/benchmarks: sql-runner' \
  /evidence/tree-diff.txt | grep -q .; then
  echo "working tree is not the fixed archive plus sql-runner" >&2
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
if [ ! -f /src/benchmarks/sql-runner/Cargo.lock ]; then
  echo "crate lock missing" >&2
  phase lock 2
  exit 2
fi

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
grep 'serde 1.0.229 4148590afebada386688f18773da617792bf2ef03ffc1e4cbd2b1d45b023e0ba' /tmp/crate-pins.txt
grep 'serde_json 1.0.151 c841b55ecdae098c80dcae9cf767f6f8a0c2cdb3416bbef72181df4d0fe73f14' /tmp/crate-pins.txt
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
    echo "offline fetch missed for ${tag}; fetching locked, not updating" >&2
    set +e
    cargo fetch --manifest-path "$manifest" --locked > "/evidence/${tag}-fetch-online.stdout" 2> "/evidence/${tag}-fetch-online.stderr"
    code=$?
    set -e
  fi
  phase "${tag}-fetch" "$code"
  return "$code"
}

echo "=== cargo test sql-runner ==="
cd /src/benchmarks/sql-runner
set +e
cargo test --locked --offline -- --test-threads=1 > /evidence/sql-runner-test.stdout 2> /evidence/sql-runner-test.stderr
test_exit=$?
set -e
phase sql-runner-test "$test_exit"
if [ "$test_exit" -ne 0 ]; then
  exit "$test_exit"
fi

echo "=== hash tools ==="
rustc --edition 2021 -D unsafe_code /tools/sha256.rs -o /cargo-home/bin/sha256
phase sha256-tool $?
rustc --edition 2021 -D unsafe_code /tools/select-debs.rs -o /cargo-home/bin/select-debs
phase select-debs-tool $?
export PATH="/cargo-home/bin:${PATH}"

echo "=== libclang ==="
mkdir -p /evidence/libclang
if [ ! -s /evidence/libclang/libclang-dir.txt ] || [ ! -d /cargo-home/a231-clang-root/usr/lib ]; then
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
multiarch=/cargo-home/a231-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a231-clang-root/usr/lib/llvm-19/lib/clang/19/include
real=$(find /cargo-home/a231-clang-root/usr/lib -name 'libclang-19.so.19' -print -quit)
devso=$(find /cargo-home/a231-clang-root/usr/lib -name 'libclang.so' -print -quit)
if [ -n "$devso" ]; then
  libdir=$(dirname "$devso")
fi
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a231-clang-root/usr/share/pkgconfig"
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
export CARGO_PROFILE_DEV_DEBUG=0
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
unset CARGO_PROFILE_DEV_DEBUG
export CARGO_TARGET_DIR=/target
cargo build --manifest-path /src/benchmarks/sql-runner/Cargo.toml --locked --offline --bin sql-runner \
  > /evidence/sql-runner-build.stdout 2> /evidence/sql-runner-build.stderr
cp -a /target/debug/sql-runner /target/bins/sql-runner
wc -c /target/bins/query-pg /target/bins/query-result-adapter /target/bins/result-comparator /target/bins/sql-runner > /evidence/bin-sizes.txt
for bin in /target/bins/query-pg /target/bins/query-result-adapter /target/bins/result-comparator; do
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

echo "=== live query-pg ==="
mkdir -p /evidence/live
set +e
env OO_A198_LIVE=1 "$exe" --exact live_query_pg_against_private_postgres --test-threads=1 --nocapture \
  > /evidence/live/test.stdout 2> /evidence/live/test.stderr
live_exit=$?
set -e
phase live "$live_exit"
if [ "$live_exit" -ne 0 ]; then
  exit "$live_exit"
fi
grep -q '^done=ok$' /evidence/live/oracle.txt
grep -q '^hidden_exit=6$' /evidence/live/oracle.txt
phase oracle 0

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

reap_children() {
  local label="$1"
  local pid
  children > "/evidence/attempts/${label}-children.txt" || true
  if [ ! -s "/evidence/attempts/${label}-children.txt" ]; then
    return 0
  fi
  echo "reaping leftover children after ${label}" >&2
  while read -r pid _; do
    [ -n "$pid" ] || continue
    kill "$pid" 2>/dev/null || true
  done < "/evidence/attempts/${label}-children.txt"
  sleep 1
  children > "/evidence/attempts/${label}-children-after.txt" || true
  if [ -s "/evidence/attempts/${label}-children-after.txt" ]; then
    while read -r pid _; do
      [ -n "$pid" ] || continue
      kill -9 "$pid" 2>/dev/null || true
    done < "/evidence/attempts/${label}-children-after.txt"
    sleep 1
    children > "/evidence/attempts/${label}-children-final.txt" || true
    if [ -s "/evidence/attempts/${label}-children-final.txt" ]; then
      return 1
    fi
  fi
  return 0
}

if ! reap_children live; then
  phase children 1
  exit 1
fi

echo "=== suite documents ==="
rustc --edition 2021 -D unsafe_code /tools/gold.rs -o /target/bins/gold
rustc --edition 2021 -D unsafe_code /tools/prepare.rs -o /target/bins/prepare
rustc --edition 2021 -D unsafe_code /tools/check.rs -o /target/bins/check
rustc --edition 2021 -D unsafe_code /tools/occupy.rs -o /target/bins/occupy
phase tools 0
/target/bins/gold /evidence/fixtures/desk-gold.json /evidence/gold
phase gold 0
trunc=$(awk -F= '$1=="truncate_bytes"{print $2; exit}' /evidence/live/oracle.txt)
printf '%s\n' "$trunc" > /evidence/truncate-bytes.txt
case "$trunc" in
  ''|0|4096|*[!0-9]*)
    echo "truncate_bytes is not a usable positive limit" >&2
    phase prepare 2
    exit 2
    ;;
esac
query_sha=$(/cargo-home/bin/sha256 file /target/bins/query-pg | awk '{print $1}')
adapter_sha=$(/cargo-home/bin/sha256 file /target/bins/query-result-adapter | awk '{print $1}')
comparator_sha=$(/cargo-home/bin/sha256 file /target/bins/result-comparator | awk '{print $1}')
{
  echo "query_pg=$query_sha"
  echo "adapter=$adapter_sha"
  echo "comparator=$comparator_sha"
} > /evidence/bin-sha256.txt
/target/bins/prepare \
  /evidence/suites \
  /target/bins/query-pg "$query_sha" \
  /target/bins/query-result-adapter "$adapter_sha" \
  /target/bins/result-comparator "$comparator_sha" \
  /evidence/fixtures/desk-header.json \
  /evidence/suites/header-trunc.json \
  /evidence/gold/gold-exact.json \
  /evidence/gold/gold-changed.json \
  /evidence/gold/gold-prefix.json \
  /evidence/gold/gold-set.json \
  "$trunc"
phase prepare 0

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

echo "=== hold table ==="
date -u +%Y-%m-%dT%H:%M:%SZ > /evidence/hold-needed
if ! wait_flag /evidence/hold-ready 90; then
  phase hold 1
  exit 1
fi
phase hold 0

run_suite() {
  local name="$1"
  local want="$2"
  local run="$3"
  local out="/evidence/reports"
  local code
  local had_e=0
  case $- in
    *e*) had_e=1 ;;
  esac
  restore_e() {
    if [ "$had_e" -eq 1 ]; then
      set -e
    else
      set +e
    fi
  }
  if [ -e "$run" ]; then
    echo "run dir already exists for $name" >&2
    return 2
  fi
  set +e
  /target/bins/sql-runner --suite "/evidence/suites/suite-${name}.json" --run-dir "$run" \
    > "${out}/${name}.stdout" 2> "${out}/${name}.stderr"
  code=$?
  restore_e
  printf '%s\n' "$code" > "${out}/${name}.exit"
  if [ -f "${run}/report.json" ]; then
    cp "${run}/report.json" "${out}/${name}.json"
  fi
  if ! reap_children "$name"; then
    echo "child leaked after $name" >&2
    return 1
  fi
  if [ "$code" -ne "$want" ]; then
    echo "runner exit $code want $want for $name" >&2
    return 1
  fi
  if [ ! -f "${out}/${name}.json" ]; then
    echo "report missing for $name" >&2
    return 1
  fi
  if ! cmp -s "${out}/${name}.stdout" "${out}/${name}.json"; then
    echo "stdout differs from report.json for $name" >&2
    return 1
  fi
  set +e
  /target/bins/check "${out}/${name}.json" "${out}/${name}.stderr" /run/oo-a198/fixture/login.pass "/evidence/expect/${name}.txt" \
    > "${out}/${name}.check.stdout" 2> "${out}/${name}.check.stderr"
  code=$?
  restore_e
  printf '%s\n' "$code" > "${out}/${name}.check.exit"
  if [ "$code" -ne 0 ]; then
    echo "report check failed for $name" >&2
    return 1
  fi
  return 0
}

proof_exit=0
echo "=== suite exact ==="
if ! run_suite exact 0 /tmp/runs/exact; then
  proof_exit=1
fi
phase exact "$(cat /evidence/reports/exact.exit 2>/dev/null || echo missing)"

echo "=== suite fail ==="
if ! run_suite fail 1 /tmp/runs/fail; then
  proof_exit=1
fi
phase fail "$(cat /evidence/reports/fail.exit 2>/dev/null || echo missing)"

echo "=== suite timeout ==="
date -u +%Y-%m-%dT%H:%M:%SZ > /evidence/timeout-needed
if ! wait_flag /evidence/timeout-ready 120; then
  echo "lock was not acquired" >&2
  proof_exit=1
  phase timeout missing
else
  if ! run_suite timeout 1 /tmp/runs/timeout; then
    proof_exit=1
  fi
  phase timeout "$(cat /evidence/reports/timeout.exit 2>/dev/null || echo missing)"
fi
date -u +%Y-%m-%dT%H:%M:%SZ > /evidence/timeout-release
if ! wait_flag /evidence/timeout-released 60; then
  echo "lock release was not recorded" >&2
  proof_exit=1
fi

echo "=== tight filesystem attempts ==="
tight_attempt() {
  local name="$1"
  local run="/tight/${name}-run"
  local code
  rm -rf "$run"
  set +e
  /target/bins/sql-runner --suite /evidence/suites/suite-noend.json --run-dir "$run" \
    > "/evidence/attempts/${name}.stdout" 2> "/evidence/attempts/${name}.stderr"
  code=$?
  set -e
  printf '%s\n' "$code" > "/evidence/attempts/${name}.exit"
  if [ -f "${run}/report.json" ]; then
    cp "${run}/report.json" "/evidence/attempts/${name}.json"
  fi
  if ! reap_children "$name"; then
    echo "child leaked after $name" >&2
    proof_exit=1
  fi
}
mkdir -p /tight/space
dd if=/dev/zero of=/tight/space/fill bs=1024 count=48 >/dev/null 2>&1 || true
tight_attempt tight-room
dd if=/dev/zero of=/tight/space/rest bs=1024 count=1024 >/dev/null 2>&1 || true
tight_attempt tight-full
printf '%s\n' 'tight attempts are recorded and are not the asserted missing-completion trial' > /evidence/attempts/tight-note.txt

echo "=== suite noend ==="
noend_once() {
  local tag="$1"
  local run="/tmp/runs/${tag}"
  local trial="${run}/case-noend/trial-0"
  local occ=1
  local code=1
  local pid
  /target/bins/occupy "$trial" > "/evidence/attempts/${tag}-occupy.stdout" 2> "/evidence/attempts/${tag}-occupy.stderr" &
  pid=$!
  set +e
  run_suite noend 1 "$run"
  code=$?
  wait "$pid"
  occ=$?
  set -e
  printf '%s\n' "$occ" > "/evidence/attempts/${tag}-occupy.exit"
  if [ -f "${trial}/audit.jsonl" ]; then
    local size
    size=$(wc -c < "${trial}/audit.jsonl")
    if [ "$size" -le 16 ]; then
      cp "${trial}/audit.jsonl" "/evidence/attempts/${tag}-audit.jsonl"
    fi
  fi
  if [ "$code" -ne 0 ] || [ "$occ" -ne 0 ]; then
    return 1
  fi
  cp /evidence/reports/noend.json "/evidence/reports/${tag}.json" 2>/dev/null || true
  return 0
}
if ! noend_once noend; then
  echo "first missing-completion attempt did not meet the expect; trying once more" >&2
  rm -f /evidence/reports/noend.json /evidence/reports/noend.stdout /evidence/reports/noend.stderr \
    /evidence/reports/noend.exit /evidence/reports/noend.check.exit /evidence/reports/noend.check.stdout \
    /evidence/reports/noend.check.stderr
  if ! noend_once noend-retry; then
    proof_exit=1
    phase noend 1
  else
    phase noend 0
  fi
else
  phase noend 0
fi

echo "=== backend count ==="
date -u +%Y-%m-%dT%H:%M:%SZ > /evidence/counts-needed
if ! wait_flag /evidence/counts-ready 60; then
  echo "backend count was not recorded" >&2
  proof_exit=1
fi

echo "=== product unchanged ==="
diff -rq /archive /src > /evidence/tree-diff-after.txt || true
if grep -v -e 'Only in /archive: rust-toolchain.toml' \
  -e 'Only in /src: rust-toolchain.toml.repo' \
  -e 'Only in /src/benchmarks: sql-runner' \
  /evidence/tree-diff-after.txt | grep -q .; then
  echo "product tree changed" >&2
  phase product 2
  proof_exit=2
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
  proof_exit=2
else
  phase source-after 0
fi
phase proof "$proof_exit"
exit "$proof_exit"
