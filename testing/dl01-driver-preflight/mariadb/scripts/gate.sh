#!/bin/bash
# Real native MariaDB gate. Expected driver failures are checked. A failed phase keeps its files.
set -u
source "$(dirname -- "$0")/lib.sh"
NAME=oo-dl01w1-39-rust
EV="$OO_EVIDENCE"
DIGEST=sha256:0130d92c05fbf2d82adc2b86de742eaede65b2596c65d91786f8e03cd19e6a39
PROBE=/target/release/mariadb-probe
DECODE=/target/release/mariadb-arrow-decode
mkdir -p "$EV/gate"
phase() { printf '%s\n' "$1" | tee "$EV/gate-phase.txt"; }
finish() { printf '%s\n' "$1" > "$EV/gate.exit"; echo "gate_exit=$1"; exit "$1"; }

if ! docker exec "$NAME" test -x "$PROBE"; then
  echo "release probe is not in the builder" >&2
  finish 2
fi
if ! docker exec "$NAME" test -x "$DECODE"; then
  echo "release decoder is not in the builder" >&2
  finish 2
fi

run_probe() {
  local command=$1
  shift
  docker exec "$NAME" "$PROBE" "$command" \
    --host "$OO_CONTAINER" \
    --port 3306 \
    --user w39_reader \
    --password-file /run/secrets/reader.pw \
    --image-digest "$DIGEST" \
    "$@"
}

expect_fail() {
  local dir=$1
  local code=$2
  shift 2
  mkdir -p "$dir"
  set +e
  run_probe "$@" > "$dir/probe.stdout" 2> "$dir/probe.stderr"
  local status=$?
  set -e
  printf '%s\n' "$status" > "$dir/probe.exit"
  set +e
  if [ "$status" = "0" ]; then
    echo "expected failure $code exited 0" >&2
    return 1
  fi
  if [ -f "$dir/ready.json" ] || [ -f "$dir/ready.json.tmp" ]; then
    echo "half-ready receipt in $dir" >&2
    return 1
  fi
  python3 - "$dir/failed.json" "$code" <<'PY'
import json, sys
body = json.load(open(sys.argv[1], encoding="utf-8"))
ok = body.get("state") == "failed" and body.get("code") == sys.argv[2] and body.get("source_acceptance") is False
sys.exit(0 if ok else 1)
PY
}

phase inventory
bash "$(dirname -- "$0")/inventory.sh" > "$EV/gate/inventory.stdout" 2> "$EV/gate/inventory.stderr" || finish $?

phase oracle
python3 "$(dirname -- "$0")/oracle.py" "$EV/oracle" > "$EV/gate/oracle.stdout" 2> "$EV/gate/oracle.stderr" || finish $?

phase reset-log
oo_root -e "SET GLOBAL general_log = 0; SET GLOBAL log_output = 'TABLE'; TRUNCATE TABLE mysql.general_log; SET GLOBAL general_log = 1;" \
  > "$EV/gate/reset-log.stdout" 2> "$EV/gate/reset-log.stderr" || finish $?
state=$(oo_root -e "SELECT CONCAT(@@global.general_log+0,'|',@@global.log_output)")
printf '%s\n' "$state" > "$EV/gate/general-log-state.txt"
if [ "$state" != "1|TABLE" ]; then
  echo "general_log state is $state" >&2
  finish 1
fi

phase read
mkdir -p "$EV/read"
set +e
run_probe read --out /evidence/read --batch-rows 4 > "$EV/read/probe.stdout" 2> "$EV/read/probe.stderr"
status=$?
set -e
printf '%s\n' "$status" > "$EV/read/probe.exit"
if [ "$status" != "0" ]; then
  finish "$status"
fi
if [ ! -f "$EV/read/ready.json" ]; then
  echo "read produced no ready.json" >&2
  finish 1
fi

phase decode
set +e
docker exec "$NAME" bash -lc "ls /evidence/read/*.arrow >/dev/null && $DECODE --out /evidence/read/decoded.json /evidence/read/*.arrow" \
  > "$EV/read/decode.stdout" 2> "$EV/read/decode.stderr"
status=$?
set -e
printf '%s\n' "$status" > "$EV/read/decode.exit"
if [ "$status" != "0" ]; then
  finish "$status"
fi

phase compare
python3 "$(dirname -- "$0")/compare.py" \
  "$EV/read/decoded.json" \
  "$EV/oracle/oracle.json" \
  "$EV/oracle/oracle-checks.json" \
  "$OO_PREP/expected/types.tsv" \
  > "$EV/gate/compare.stdout" 2> "$EV/gate/compare.stderr" || finish $?

phase catalog
python3 "$(dirname -- "$0")/catalog_check.py" "$EV/read/catalog.json" "$EV/read/ready.json" \
  > "$EV/gate/catalog.stdout" 2> "$EV/gate/catalog.stderr" || finish $?

phase truncated-ipc
mkdir -p "$EV/truncated"
cp "$EV/read/k_unsigned.arrow" "$EV/truncated/chopped.arrow"
python3 - "$EV/truncated/chopped.arrow" <<'PY'
import pathlib, sys
path = pathlib.Path(sys.argv[1])
data = path.read_bytes()
if len(data) < 64:
    raise SystemExit("arrow stream is too small to chop")
path.write_bytes(data[:-32])
PY
rm -f "$EV/truncated/decoded.json" "$EV/truncated/decoded.json.tmp"
set +e
docker exec "$NAME" "$DECODE" --out /evidence/truncated/decoded.json /evidence/truncated/chopped.arrow \
  > "$EV/truncated/decode.stdout" 2> "$EV/truncated/decode.stderr"
status=$?
set -e
printf '%s\n' "$status" > "$EV/truncated/decode.exit"
if [ "$status" = "0" ] || [ -f "$EV/truncated/decoded.json" ]; then
  echo "truncated IPC was accepted" >&2
  finish 1
fi

phase reject-float-key
expect_fail "$EV/reject-float-key" unsafe_float_key reject-table --table k_float_key --out /evidence/reject-float-key || finish $?
phase reject-float
expect_fail "$EV/reject-float" unsafe_float reject-table --table k_float --out /evidence/reject-float || finish $?
phase reject-bit
expect_fail "$EV/reject-bit" unsupported_bit reject-table --table k_bit --out /evidence/reject-bit || finish $?

phase zero-date
mkdir -p "$EV/zero-date"
set +e
run_probe zero-date --out /evidence/zero-date > "$EV/zero-date/probe.stdout" 2> "$EV/zero-date/probe.stderr"
status=$?
set -e
printf '%s\n' "$status" > "$EV/zero-date/probe.exit"
if [ "$status" != "0" ]; then
  finish "$status"
fi

phase deny-select
mkdir -p "$EV/deny-select"
set +e
run_probe deny-select --out /evidence/deny-select > "$EV/deny-select/probe.stdout" 2> "$EV/deny-select/probe.stderr"
status=$?
set -e
printf '%s\n' "$status" > "$EV/deny-select/probe.exit"
if [ "$status" != "0" ]; then
  finish "$status"
fi

phase cap-row
expect_fail "$EV/cap-row" row_cap caps row --max-rows 1 --out /evidence/cap-row || finish $?
phase cap-byte
expect_fail "$EV/cap-byte" byte_cap caps byte --max-bytes 32 --out /evidence/cap-byte || finish $?
phase cap-deadline
expect_fail "$EV/cap-deadline" deadline caps deadline --deadline-ms 500 --out /evidence/cap-deadline || finish $?
# The deadline aborts the client wait while SLEEP can still be visible server-side.
# Cancel's count is the live reader sessions, so wait until that session is gone.
reader_sessions=unknown
for _ in $(seq 1 40); do
  reader_sessions=$(oo_root -e "SELECT COUNT(*) FROM information_schema.PROCESSLIST WHERE USER = 'w39_reader'")
  if [ "$reader_sessions" = "0" ]; then
    break
  fi
  sleep 0.25
done
printf '%s\n' "$reader_sessions" > "$EV/gate/reader-sessions-before-cancel.txt"
if [ "$reader_sessions" != "0" ]; then
  echo "reader sessions still open before cancel: $reader_sessions" >&2
  finish 1
fi

phase cancel
mkdir -p "$EV/cancel"
rm -f "$EV/cancel/go" "$EV/cancel/ready.txt" "$EV/cancel/ready.json"
set +e
run_probe cancel --out /evidence/cancel --ready-file /evidence/cancel/ready.txt --go-file /evidence/cancel/go \
  > "$EV/cancel/probe.stdout" 2> "$EV/cancel/probe.stderr" &
cancel_pid=$!
set -e
seen=0
for _ in $(seq 1 80); do
  if [ -s "$EV/cancel/ready.txt" ]; then
    seen=1
    break
  fi
  sleep 0.25
done
if [ "$seen" != "1" ]; then
  wait "$cancel_pid" || true
  echo "cancel ready file was not written" >&2
  finish 1
fi
oo_dba -e "SELECT ID, USER, COMMAND, TIME, STATE, IFNULL(INFO, '') FROM information_schema.PROCESSLIST WHERE USER = 'w39_reader' ORDER BY ID" \
  > "$EV/cancel/dba-processlist.txt" 2> "$EV/cancel/dba-processlist.stderr" || true
{
  echo "dba_kill=false"
  echo "ready:"
  cat "$EV/cancel/ready.txt"
  echo "processlist:"
  cat "$EV/cancel/dba-processlist.txt"
} > "$EV/cancel/dba-observation.txt"
printf '%s\n' go > "$EV/cancel/go"
set +e
wait "$cancel_pid"
status=$?
set -e
printf '%s\n' "$status" > "$EV/cancel/probe.exit"
if [ "$status" != "0" ]; then
  finish "$status"
fi
python3 - "$EV/cancel/cancel.json" "$EV/cancel/ready.txt" <<'PY'
import json, sys
body = json.load(open(sys.argv[1], encoding="utf-8"))
ready = open(sys.argv[2], encoding="utf-8").read()
facts = dict(line.split("=", 1) for line in ready.splitlines() if "=" in line)
ok = (
    body.get("state") == "cancel_proven"
    and body.get("dba_kill_used_by_driver") is False
    and body.get("source_acceptance") is False
    and body.get("marker_cleared") is True
    and int(body.get("connections_before")) == 2
    and str(body.get("business_connection_id")) == facts.get("business")
    and str(body.get("control_connection_id")) == facts.get("control")
)
sys.exit(0 if ok else 1)
PY
if [ $? -ne 0 ]; then
  echo "cancel receipt did not prove a driver cancel" >&2
  finish 1
fi

phase broken
mkdir -p "$EV/broken"
rm -f "$EV/broken/id.txt" "$EV/broken/ready.json"
set +e
run_probe broken --out /evidence/broken --ready-file /evidence/broken/id.txt \
  > "$EV/broken/probe.stdout" 2> "$EV/broken/probe.stderr" &
broken_pid=$!
set -e
seen=0
for _ in $(seq 1 80); do
  if [ -s "$EV/broken/id.txt" ]; then
    seen=1
    break
  fi
  sleep 0.25
done
if [ "$seen" != "1" ]; then
  wait "$broken_pid" || true
  echo "broken connection id was not written" >&2
  finish 1
fi
# The probe sets TCP_KEEPIDLE to 1s. This image keeps the default 75s interval and 9 probes,
# so a disconnect that does not RST the socket takes about 676s to surface.
docker network disconnect "$OO_NETWORK" "$NAME" > "$EV/broken/disconnect.stdout" 2> "$EV/broken/disconnect.stderr"
disconnect_status=$?
printf '%s\n' "$disconnect_status" > "$EV/broken/disconnect.exit"
set +e
wait "$broken_pid"
status=$?
set -e
printf '%s\n' "$status" > "$EV/broken/probe.exit"
docker network connect "$OO_NETWORK" "$NAME" > "$EV/broken/reconnect.stdout" 2> "$EV/broken/reconnect.stderr"
printf '%s\n' "$?" > "$EV/broken/reconnect.exit"
if [ "$disconnect_status" != "0" ] || [ "$status" = "0" ] || [ -f "$EV/broken/ready.json" ]; then
  echo "broken connection was not a necessary failure" >&2
  finish 1
fi
python3 - "$EV/broken/failed.json" <<'PY'
import json, sys
body = json.load(open(sys.argv[1], encoding="utf-8"))
ok = body.get("state") == "failed" and body.get("code") == "broken_connection" and body.get("source_acceptance") is False
sys.exit(0 if ok else 1)
PY
if [ $? -ne 0 ]; then
  finish 1
fi

phase audit
mkdir -p "$EV/audit"
oo_dba -e "SELECT command_type, REPLACE(REPLACE(argument, CHAR(10), ' '), CHAR(13), ' ') FROM mysql.general_log WHERE user_host LIKE 'w39_reader%' ORDER BY event_time, thread_id" \
  > "$EV/audit/reader-general-log.tsv" 2> "$EV/audit/reader-general-log.stderr" || finish $?
python3 "$(dirname -- "$0")/secret_scan.py" "$OO_SECRET_DIR" "$EV/audit/reader-general-log.tsv" > "$EV/audit/secret-scan.txt" || finish $?
set +e
docker exec "$NAME" "$PROBE" audit --log /evidence/audit/reader-general-log.tsv > "$EV/audit/audit.stdout" 2> "$EV/audit/audit.stderr"
status=$?
set -e
printf '%s\n' "$status" > "$EV/audit/audit.exit"
if [ "$status" != "0" ]; then
  finish "$status"
fi
oo_dba -e "SELECT command_type, REPLACE(REPLACE(argument, CHAR(10), ' '), CHAR(13), ' ') FROM mysql.general_log WHERE user_host LIKE 'w39_dba%' ORDER BY event_time, thread_id" \
  > "$EV/audit/dba-general-log.tsv"
python3 - "$EV/audit/dba-general-log.tsv" "$EV/audit/audit.stdout" <<'PY'
import sys
text = open(sys.argv[1], encoding="utf-8", errors="replace").read().splitlines()
kills = [line for line in text if line.split("\t", 1)[0] == "Query" and line.split("\t", 1)[-1].lstrip().upper().startswith("KILL")]
open(sys.argv[1] + ".kill-count", "w", encoding="utf-8").write(f"dba_kill_statements={len(kills)}\n")
counts = {}
for part in open(sys.argv[2], encoding="utf-8").read().replace("\n", " ").split():
    if "=" in part:
        key, value = part.split("=", 1)
        counts[key] = value
sys.exit(0 if kills == [] and counts.get("write") == "0" and counts.get("cancel", "0") != "0" and counts.get("connect", "0") != "0" else 1)
PY
if [ $? -ne 0 ]; then
  echo "audit did not show a zero-write reader log and a DBA log without KILL" >&2
  finish 1
fi

phase binaries
docker exec "$NAME" sha256sum "$PROBE" "$DECODE" > "$EV/raw/binary-sha256.txt"
printf '%s\n' 0 > "$EV/gate.exit"
echo "gate_exit=0"
