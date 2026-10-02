#!/bin/bash
# Fixed-tree focused xtask check plus an independent escape probe.
# Phase exits are recorded even when a later phase fails.
set -u
OUT=/out
exec > >(tee -a "$OUT/console.log") 2>&1
printf 'start %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"

record() {
  local name="$1"
  shift
  "$@" >"$OUT/${name}.log" 2>&1
  local code=$?
  printf '%s\n' "$code" >"$OUT/${name}.exit"
  printf '%s exit %s\n' "$name" "$code"
  return 0
}

rustc -vV >"$OUT/rustc-version.txt" 2>&1 || true
cargo -V >"$OUT/cargo-version.txt" 2>&1 || true
git --version >"$OUT/git-version.txt" 2>&1 || true
gcc --version >"$OUT/gcc-version.txt" 2>&1 || true
printf 'toolchain-file:\n' >"$OUT/toolchain-file.txt"
cat /src/rust-toolchain.toml >>"$OUT/toolchain-file.txt" 2>&1 || true
rustup show >"$OUT/rustup-show.txt" 2>&1 || true
rustup component list --toolchain 1.94.0 >"$OUT/components-before.txt" 2>&1 || true
grep -E 'Cap(Inh|Prm|Eff|Bnd|Amb)' /proc/self/status >"$OUT/caps.txt" 2>&1 || true
id >"$OUT/id.txt" 2>&1 || true
readlink -f /usr/bin/gcc /usr/bin/cc /bin/cc >"$OUT/compiler-canon.txt" 2>&1 || true
stat -c '%n %u %a' /usr/bin/gcc /usr/bin/cc /usr/bin/gcc-14 /bin/kill /usr/bin/kill >"$OUT/compiler-stat.txt" 2>&1 || true

record rustup-clippy rustup component add clippy --toolchain 1.94.0
rustup component list --toolchain 1.94.0 >"$OUT/components-after.txt" 2>&1 || true
record cargo-fetch cargo fetch --manifest-path /src/Cargo.toml --locked
record xtask-test cargo test --manifest-path /src/Cargo.toml --locked --offline -p xtask -- --test-threads=8
record xtask-clippy cargo clippy --manifest-path /src/Cargo.toml --locked --offline -p xtask --all-targets -- -D warnings

probe=/target/a070-escape-probe
mkdir -p "$probe"
(
  printf '=== supervisor ===\n'
  find /target -type f -name 'oo-xtask-contain-*' ! -name '*.c' -print >"$probe/bins.txt" 2>/dev/null || true
  sup=$(head -n 1 "$probe/bins.txt" || true)
  printf 'supervisor=%s\n' "${sup:-missing}"
  if [ -z "${sup:-}" ] || [ ! -x "$sup" ]; then
    echo 'probe_result=supervisor-missing'
    exit 2
  fi
  cat >"$probe/escape.c" <<'EOF'
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
static void write_pid(const char *path) {
    FILE *file = fopen(path, "w");
    if (!file) _exit(4);
    fprintf(file, "%d\n", (int)getpid());
    fflush(file);
    fclose(file);
}
int main(int argc, char **argv) {
    const char *mode;
    const char *pidfile;
    pid_t first;
    pid_t second;
    int i;
    if (argc < 3) return 2;
    mode = argv[1];
    pidfile = argv[2];
    first = fork();
    if (first < 0) return 3;
    if (first > 0) {
        if (strcmp(mode, "immediate") == 0) _exit(0);
        for (i = 0; i < 500; i++) {
            if (access(pidfile, R_OK) == 0) _exit(0);
            usleep(1000);
        }
        _exit(0);
    }
    if (setsid() < 0) _exit(5);
    second = fork();
    if (second < 0) _exit(6);
    if (second > 0) _exit(0);
    if (strcmp(mode, "hold") == 0) {
        int fd;
        write_pid(pidfile);
        fd = open(argv[3], O_CREAT | O_WRONLY | O_APPEND, 0644);
        for (;;) {
            if (fd >= 0 && write(fd, "x", 1) < 0) _exit(0);
            if (write(1, "y", 1) < 0) _exit(0);
        }
    }
    write_pid(pidfile);
    execl("/bin/sleep", "sleep", "121", (char *)0);
    _exit(127);
}
EOF
  gcc -std=c11 -O2 -Wall -Werror -o "$probe/escape" "$probe/escape.c"
  echo helper_compile=$?
  if [ ! -x "$probe/escape" ]; then
    echo 'probe_result=helper-compile-failed'
    exit 2
  fi
  field() {
    awk -F= -v key="$1" '$1==key {print substr($0, index($0, "=")+1)}' "$2"
  }
  fail=0
  note_fail() {
    echo "FAIL $*"
    fail=1
  }
  foreign=$(/bin/sleep 180 >/dev/null 2>&1 & echo $!)
  echo "foreign_pid=$foreign"
  sleep 0.1
  if [ ! -d "/proc/$foreign" ]; then
    note_fail "foreign sleep did not start"
  fi
  run_one() {
    local label="$1"
    shift
    local dir="$probe/$label"
    mkdir -p "$dir"
    local before after
    before=$(date +%s%N)
    "$sup" "$dir/status" "$dir/stdout" "$dir/stderr" 16777216 -- "$@" >"$dir/sup.out" 2>"$dir/sup.err"
    local code=$?
    after=$(date +%s%N)
    echo "case=$label supervisor_exit=$code elapsed_ns=$((after-before))"
    echo "status:"
    cat "$dir/status" 2>/dev/null || echo missing
  }
  for i in $(seq 1 15); do
    dir="$probe/immediate-$i"
    rm -rf "$dir"
    mkdir -p "$dir"
    run_one "immediate-$i" "$probe/escape" immediate "$dir/pid"
    alive=$(ps -eo args | awk '/[s]leep 121/ {n++} END{print n+0}')
    echo "immediate_$i sleep121=$alive"
    if [ "$alive" != 0 ]; then
      note_fail "immediate $i left sleep 121"
      ps -eo pid,ppid,stat,args | awk '/[s]leep 121/' || true
    fi
    cont=$(field containment "$dir/status" || true)
    echo "immediate_$i containment=${cont:-missing}"
    if [ "$cont" != verified ]; then
      note_fail "immediate $i containment=$cont"
    fi
  done
  for i in $(seq 1 15); do
    dir="$probe/recorded-$i"
    rm -rf "$dir"
    mkdir -p "$dir"
    run_one "recorded-$i" "$probe/escape" recorded "$dir/pid"
    pid=$(tr -cd '0-9' <"$dir/pid" || true)
    state=gone
    if [ -n "$pid" ] && [ -d "/proc/$pid" ]; then
      state=$(awk '{print $3}' "/proc/$pid/stat" 2>/dev/null || echo listed)
    fi
    sleep 0.3
    state_after=gone
    if [ -n "$pid" ] && [ -d "/proc/$pid" ]; then
      state_after=$(awk '{print $3}' "/proc/$pid/stat" 2>/dev/null || echo listed)
    fi
    echo "recorded_$i pid=${pid:-missing} at_return=$state after=$state_after direct=$(field direct_pid "$dir/status" || true)"
    if [ -z "$pid" ]; then
      note_fail "recorded $i wrote no pid"
    fi
    if [ "$state" != gone ] || [ "$state_after" != gone ]; then
      note_fail "recorded $i pid $pid state $state/$state_after"
    fi
    if [ "$(field containment "$dir/status")" != verified ]; then
      note_fail "recorded $i containment not verified"
    fi
  done
  dir="$probe/hold"
  rm -rf "$dir"
  mkdir -p "$dir"
  run_one hold "$probe/escape" hold "$dir/pid" "$dir/side"
  pid=$(tr -cd '0-9' <"$dir/pid" || true)
  size1=$(wc -c <"$dir/side" 2>/dev/null || echo 0)
  sleep 0.3
  size2=$(wc -c <"$dir/side" 2>/dev/null || echo 0)
  out_size=$(wc -c <"$dir/stdout" 2>/dev/null || echo 0)
  echo "hold pid=${pid:-missing} side=$size1/$size2 stdout=$out_size"
  if [ -n "$pid" ] && [ -d "/proc/$pid" ]; then
    note_fail "hold writer $pid still present"
  fi
  if [ "$size1" != "$size2" ]; then
    note_fail "hold side grew $size1 -> $size2"
  fi
  if [ "$out_size" -gt 16777216 ]; then
    note_fail "hold stdout $out_size exceeds 16777216"
  fi
  dir="$probe/exact"
  rm -rf "$dir"
  mkdir -p "$dir"
  run_one exact /bin/dd if=/dev/zero bs=1048576 count=16
  exact_size=$(wc -c <"$dir/stdout" || echo 0)
  echo "exact size=$exact_size limited=$(field output_limited "$dir/status") exit=$(field exit_code "$dir/status") containment=$(field containment "$dir/status")"
  if [ "$exact_size" != 16777216 ] || [ "$(field output_limited "$dir/status")" != 0 ] || [ "$(field exit_code "$dir/status")" != 0 ] || [ "$(field containment "$dir/status")" != verified ]; then
    note_fail "exact 16MiB did not seal a bounded success"
  fi
  dir="$probe/plus"
  rm -rf "$dir"
  mkdir -p "$dir"
  run_one plus /bin/sh -c '/bin/dd if=/dev/zero bs=1048576 count=16; /bin/dd if=/dev/zero bs=1 count=1'
  plus_size=$(wc -c <"$dir/stdout" || echo 0)
  echo "plus size=$plus_size limited=$(field output_limited "$dir/status") exit=$(field exit_code "$dir/status")"
  if [ "$plus_size" != 16777216 ] || [ "$(field output_limited "$dir/status")" != 1 ]; then
    note_fail "limit+1 was not capped"
  fi
  dir="$probe/overshoot"
  rm -rf "$dir"
  mkdir -p "$dir"
  run_one overshoot /bin/dd if=/dev/zero bs=1048576 count=32
  over_size=$(wc -c <"$dir/stdout" || echo 0)
  sleep 0.2
  over_after=$(wc -c <"$dir/stdout" || echo 0)
  echo "overshoot size=$over_size/$over_after limited=$(field output_limited "$dir/status") containment=$(field containment "$dir/status")"
  if [ "$over_size" != 16777216 ] || [ "$over_after" != 16777216 ] || [ "$(field output_limited "$dir/status")" != 1 ] || [ "$(field containment "$dir/status")" != verified ]; then
    note_fail "overshoot storage was not bounded"
  fi
  if [ -d "/proc/$foreign" ]; then
    echo foreign_after=alive
  else
    echo foreign_after=gone
    note_fail "foreign sleep was killed"
  fi
  kill -TERM "$foreign" 2>/dev/null || true
  wait "$foreign" 2>/dev/null || true
  left=$(ps -eo args | awk '/[s]leep 121|[s]leep 180/ {n++} END{print n+0}')
  echo "leftover_sleeps=$left"
  if [ "$left" != 0 ]; then
    note_fail "leftover sleep $left"
    ps -eo pid,ppid,stat,args | awk '/[s]leep 121|[s]leep 180/' || true
  fi
  if [ "$fail" -eq 0 ]; then
    echo probe_result=pass
    exit 0
  fi
  echo probe_result=fail
  exit 1
) >"$OUT/probe.log" 2>&1
printf '%s\n' "$?" >"$OUT/probe.exit"
printf 'probe exit %s\n' "$(cat "$OUT/probe.exit")"

awk '/^test result:/{print}' "$OUT/xtask-test.log" >"$OUT/test-results.txt" || true
awk '/FAILED/{print}' "$OUT/xtask-test.log" >"$OUT/test-failures.txt" || true

fetch_code=$(cat "$OUT/cargo-fetch.exit")
test_code=$(cat "$OUT/xtask-test.exit")
clippy_code=$(cat "$OUT/xtask-clippy.exit")
probe_code=$(cat "$OUT/probe.exit")
printf 'fetch=%s test=%s clippy=%s probe=%s\n' "$fetch_code" "$test_code" "$clippy_code" "$probe_code" >"$OUT/summary.txt"
if [ "$fetch_code" -eq 0 ] && [ "$clippy_code" -eq 0 ] && [ "$probe_code" -eq 0 ]; then
  unexpected=$(grep -E '^test [^ ]+ \.\.\. FAILED' "$OUT/xtask-test.log" | grep -v 'reviewed_shape_rejects_unknown_keys_and_weakened_values' | grep -v 'checked_in_deny_config_matches_the_reviewed_policy' || true)
  printf '%s\n' "$unexpected" >"$OUT/unexpected-failures.txt"
  if [ -z "$unexpected" ]; then
    exit 0
  fi
fi
exit 1
