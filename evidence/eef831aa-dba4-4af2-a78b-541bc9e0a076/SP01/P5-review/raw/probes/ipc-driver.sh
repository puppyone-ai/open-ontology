#!/bin/bash
# Malformed-worker IPC probes. The published worker is not modified.
set -u
exec > >(tee /out/ipc-driver.log) 2>&1
echo "oo_owner=a076 phase=ipc"
date -u +%Y-%m-%dT%H:%M:%SZ
cd /probe
gcc -O2 -o /probe/helper helper.c
echo "gcc_exit=$?"
mkdir -p /probe/bin
modes="premature silent-sleep partial stdout-junk oversize-frame hold-setsid-noreply hold-setsid-after hold-samegroup contradict oid-true version-server quick-deny extra-frame spoof-pid spoof-start spoof-nonce bad-magic stderr-flood inherit-report net-touch"
for mode in $modes; do
  ln -sfn /probe/helper "/probe/bin/$mode"
done
ln -sfn /probe/bin/contradict /probe/bin/worker-link
# foreign sleep is not in the worker process group
sleep 400 &
echo $! > /probe/foreign-sleep.pid
echo "foreign_sleep=$(cat /probe/foreign-sleep.pid)"
export PATH="/opt/clangroot/usr/bin:${PATH:-}"
export LD_LIBRARY_PATH="/opt/clangroot/usr/lib/aarch64-linux-gnu${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export LIBCLANG_PATH="/opt/clangroot/usr/lib/aarch64-linux-gnu"
cd /probe
cargo build --offline --manifest-path /probe/Cargo.toml
echo "probe_build_exit=$?"
PROBE=/target/debug/oo-a076-probe
run_case() {
  mode=$1
  limit=$2
  rm -f /probe/descendant.pid /probe/descendant.stat
  echo "=== case $mode limit=$limit ==="
  start=$(date +%s%3N)
  set +e
  timeout --signal=TERM --kill-after=2s "${limit}s" "$PROBE" ipc "/probe/bin/$mode"
  rc=$?
  set -e
  end=$(date +%s%3N)
  echo "case=$mode shell_rc=$rc elapsed_ms=$((end-start))"
  if [ -f /probe/descendant.pid ]; then
    dpid=$(cat /probe/descendant.pid)
    echo "descendant_pid=$dpid"
    ps -o pid,ppid,pgid,sid,stat,comm -p "$dpid" || echo "descendant_absent"
    echo "foreign_alive=$(ps -o stat= -p "$(cat /probe/foreign-sleep.pid)" || echo dead)"
    # own descendant only
    if ps -p "$dpid" >/dev/null 2>&1; then
      kill -KILL "$dpid" 2>/dev/null || true
      sleep 0.2
      ps -o pid,stat,comm -p "$dpid" || echo "descendant_killed"
    fi
  else
    echo "no_descendant"
    echo "foreign_alive=$(ps -o stat= -p "$(cat /probe/foreign-sleep.pid)" || echo dead)"
  fi
  echo "workdirs=$(find /tmp -maxdepth 1 -name 'oo-p05-*' -printf '%f\n' 2>/dev/null | tr '\n' ' ')"
  echo
}
# bounded controls first
run_case premature 8
run_case quick-deny 8
run_case spoof-pid 8
run_case spoof-start 8
run_case spoof-nonce 8
run_case bad-magic 8
run_case extra-frame 8
run_case contradict 8
run_case oid-true 8
run_case version-server 8
run_case stderr-flood 12
run_case partial 8
run_case silent-sleep 8
run_case stdout-junk 8
run_case oversize-frame 8
# hangs are the point: timeout must fire if the parent does not return
run_case hold-setsid-noreply 8
run_case hold-setsid-after 8
run_case hold-samegroup 8
echo '=== inherit ==='
rm -f /probe/inherit-fds.txt /probe/worker-caps.txt /probe/secret-marker.txt
set +e
timeout --signal=TERM --kill-after=2s 8s "$PROBE" inherit /probe/bin/inherit-report
echo "inherit_rc=$?"
set -e
echo '--- inherit fds ---'
cat /probe/inherit-fds.txt 2>/dev/null || true
echo '--- worker caps ---'
cat /probe/worker-caps.txt 2>/dev/null || true
echo '=== net touch ==='
rm -f /probe/outside-touch.txt
set +e
timeout --signal=TERM --kill-after=2s 12s "$PROBE" ipc /probe/bin/net-touch
echo "net_rc=$?"
set -e
echo '--- outside touch ---'
cat /probe/outside-touch.txt 2>/dev/null || true
echo '=== path boundary ==='
set +e
"$PROBE" paths
echo "paths_rc=$?"
set -e
echo '=== published worker dump ==='
"$PROBE" dump /out/worker-plain
echo "dump_rc=$?"
echo '=== leftovers before cleanup ==='
find /tmp -maxdepth 1 -name 'oo-p05-*' -printf '%p\n' || true
ps -eo pid,ppid,pgid,sid,stat,comm | head -80
# cleanup own foreign sleep and any own helper descendants
kill -KILL "$(cat /probe/foreign-sleep.pid)" 2>/dev/null || true
pkill -KILL -f '/probe/helper' 2>/dev/null || true
sleep 0.2
echo "foreign_after=$(ps -p "$(cat /probe/foreign-sleep.pid)" >/dev/null && echo alive || echo dead)"
echo "helper_left=$(ps -eo cmd | grep -c '/probe/helper' || true)"
echo "ipc_driver_done"
