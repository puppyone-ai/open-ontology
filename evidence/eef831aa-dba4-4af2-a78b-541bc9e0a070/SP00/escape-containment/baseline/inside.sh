#!/bin/bash
# Baseline focused xtask smoke plus a containment-capability probe.
# The probe does not change the source snapshot. Its failure is recorded
# separately and does not replace the test or clippy exit.
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

record rustup-clippy rustup component add clippy --toolchain 1.94.0
rustup component list --toolchain 1.94.0 >"$OUT/components-after.txt" 2>&1 || true

record cargo-fetch cargo fetch --manifest-path /src/Cargo.toml --locked

record xtask-test cargo test --manifest-path /src/Cargo.toml --locked --offline -p xtask -- --test-threads=8
record xtask-clippy cargo clippy --manifest-path /src/Cargo.toml --locked --offline -p xtask --all-targets -- -D warnings

# Capability probe. Isolated from the smoke exits.
probe=/target/a070-cap-probe
mkdir -p "$probe"
{
  printf '=== tools ===\n'
  command -v unshare || true
  command -v python3 || true
  command -v gcc || true
  command -v cc || true
  ls -l /usr/bin/unshare /bin/unshare /usr/bin/gcc /usr/bin/cc /bin/kill /usr/bin/kill 2>&1 || true
  printf '=== cgroup ===\n'
  cat /proc/self/cgroup 2>&1 || true
  ls -ld /sys/fs/cgroup 2>&1 || true
  echo controllers:
  cat /sys/fs/cgroup/cgroup.controllers 2>&1 || true
  echo subtree:
  cat /sys/fs/cgroup/cgroup.subtree_control 2>&1 || true
  cg=/sys/fs/cgroup/oo-a070-probe
  rm -rf "$cg"
  if mkdir "$cg" 2>"$probe/cgroup-mkdir.err"; then
    echo mkdir=0
    /bin/sh -c "printf '%s\n' \$\$ > '$cg/cgroup.procs'; echo joined=\$\$; exec /bin/sleep 30" >"$probe/cgroup-child.out" 2>"$probe/cgroup-child.err" &
    cpid=$!
    sleep 0.3
    echo child_pid=$cpid
    ps -o pid,ppid,stat,cmd -p "$cpid" >"$probe/cgroup-before.txt" 2>&1 || true
    if printf '1\n' >"$cg/cgroup.kill" 2>"$probe/cgroup-kill.err"; then
      echo cgroup_kill_write=0
    else
      echo cgroup_kill_write=1
    fi
    sleep 0.3
    if ps -o pid,stat,cmd -p "$cpid" >"$probe/cgroup-after.txt" 2>&1; then
      echo child_still_listed=1
    else
      echo child_still_listed=0
    fi
    echo events:
    cat "$cg/cgroup.events" 2>&1 || true
    wait "$cpid" 2>/dev/null || true
    rmdir "$cg" 2>/dev/null || echo rmdir_failed
  else
    echo mkdir=1
    cat "$probe/cgroup-mkdir.err" || true
  fi
  printf '=== unshare ===\n'
  if [ -x /usr/bin/unshare ]; then
    /usr/bin/unshare --user --map-root-user --pid --fork --mount --mount-proc --kill-child -- /bin/true
    echo unshare_true=$?
  else
    echo unshare_missing
  fi
  printf '=== subreaper probe ===\n'
  cat >"$probe/reaper.c" <<'EOF'
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/prctl.h>
#include <sys/wait.h>
#include <unistd.h>
int main(void) {
    if (prctl(PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) != 0) {
        perror("prctl");
        return 81;
    }
    pid_t child = fork();
    if (child == 0) {
        execl("/bin/sh", "sh", "-c",
              "if [ -x /usr/bin/setsid ]; then /usr/bin/setsid /bin/sleep 120 & else /bin/sleep 120 & fi; echo $! > /target/a070-cap-probe/escape.pid; exit 0",
              (char *)NULL);
        _exit(127);
    }
    int status = 0;
    if (waitpid(child, &status, 0) < 0) {
        perror("wait");
        return 82;
    }
    usleep(200000);
    int killed = 0;
    for (int pass = 0; pass < 20; pass++) {
        int saw = 0;
        for (;;) {
            int st = 0;
            pid_t w = waitpid(-1, &st, WNOHANG);
            if (w == 0) { saw = 1; break; }
            if (w < 0 && errno == ECHILD) break;
            if (w < 0) break;
        }
        char path[64];
        snprintf(path, sizeof path, "/proc/self/task/%d/children", getpid());
        FILE *f = fopen(path, "r");
        if (f) {
            int pid = 0;
            while (fscanf(f, "%d", &pid) == 1) {
                if (pid > 1) {
                    kill(pid, SIGKILL);
                    killed++;
                }
            }
            fclose(f);
        }
        if (!saw) {
            int st = 0;
            pid_t w = waitpid(-1, &st, WNOHANG);
            if (w < 0 && errno == ECHILD) {
                usleep(50000);
                w = waitpid(-1, &st, WNOHANG);
                if (w < 0 && errno == ECHILD) {
                    printf("reaper_verified killed_signals=%d child_exit=%d\n", killed, WIFEXITED(status) ? WEXITSTATUS(status) : -1);
                    return 0;
                }
            }
        }
        usleep(20000);
    }
    printf("reaper_incomplete\n");
    return 83;
}
EOF
  cc -O2 -Wall -Werror -o "$probe/reaper" "$probe/reaper.c"
  echo reaper_compile=$?
  if [ -x "$probe/reaper" ]; then
    "$probe/reaper"
    echo reaper_exit=$?
    echo escape_pid:
    cat "$probe/escape.pid" 2>/dev/null || true
    ep=$(cat "$probe/escape.pid" 2>/dev/null || true)
    if [ -n "${ep:-}" ]; then
      if [ -d "/proc/$ep" ]; then
        echo escape_alive=1
        tr '\0' ' ' <"/proc/$ep/cmdline" || true
        echo
        awk '{print "ppid="$4}' "/proc/$ep/stat" 2>/dev/null || true
      else
        echo escape_alive=0
      fi
    fi
  fi
  printf '=== leftover sleep ===\n'
  ps -eo pid,ppid,stat,cmd | grep -E '[s]leep 120|[s]leep 30' || echo no_probe_sleep
} >"$OUT/capability.log" 2>&1
printf '0\n' >"$OUT/capability.exit"

test_code=$(cat "$OUT/xtask-test.exit")
clippy_code=$(cat "$OUT/xtask-clippy.exit")
fetch_code=$(cat "$OUT/cargo-fetch.exit")
printf 'fetch=%s test=%s clippy=%s\n' "$fetch_code" "$test_code" "$clippy_code" >"$OUT/summary.txt"
if [ "$fetch_code" -eq 0 ] && [ "$test_code" -eq 0 ] && [ "$clippy_code" -eq 0 ]; then
  exit 0
fi
exit 1
