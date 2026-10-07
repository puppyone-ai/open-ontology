#!/bin/bash
# Host side of one private PostgreSQL. The builder signals ready files.
# Password bytes stay inside the database container.
set -u
SRC=/Users/redux/puppy-one/open-ontology
FIXTURE=${SRC}/testing/real-db/pg-e2e
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a317/SP11/real-pg-e2e/in-approved-rerun
READY=/tmp/oo-a317-eef831aa-dba4-4af2-a78b-541bc9e0a317/ready
NAME=oo-a317-pg
mkdir -p "$READY" "$EVIDENCE/linux/chain" "$EVIDENCE/live/actual" "$EVIDENCE/live/side" "$EVIDENCE/live/measurements"

run_load() {
  if bash "$EVIDENCE/linux/chain/load.sh" > "$EVIDENCE/linux/chain/load.log" 2>&1; then
    : > "$READY/loaded.ready"
    bash "$FIXTURE/cluster/membership-watch.sh" "$READY/runtime.ready" > "$EVIDENCE/linux/chain/membership-watch.log" 2>&1 &
    printf '%s\n' "$!" > "$EVIDENCE/linux/chain/membership.pid"
    echo "load_ok"
  else
    : > "$READY/load.failed"
    echo "load_failed"
  fi
}

run_priv() {
  docker cp "$FIXTURE/cluster/priv.sh" "$NAME":/tmp/a317-priv.sh
  set +e
  docker exec "$NAME" bash /tmp/a317-priv.sh > "$EVIDENCE/live/side/priv-container.stdout" 2> "$EVIDENCE/live/side/priv-container.stderr"
  printf '%s\n' "$?" > "$EVIDENCE/live/side/priv-container.exit"
  set -e
  docker cp "$NAME":/tmp/a317-priv-out/. "$EVIDENCE/live/side/" || true
  reader_state=$(grep -o '42501' "$EVIDENCE/live/side/reader.err" 2>/dev/null | head -n 1 || true)
  cancel_state=$(grep -o '57014' "$EVIDENCE/live/side/sleep.err" 2>/dev/null | head -n 1 || true)
  writer_exit=$(tr -d '[:space:]' < "$EVIDENCE/live/side/writer.exit" 2>/dev/null || true)
  reader_exit=$(tr -d '[:space:]' < "$EVIDENCE/live/side/reader.exit" 2>/dev/null || true)
  {
    echo "writer_exit=${writer_exit:-missing}"
    echo "reader_exit=${reader_exit:-missing}"
    echo "reader_sqlstate=${reader_state:-absent}"
    echo "cancel_sqlstate=${cancel_state:-absent}"
  } > "$EVIDENCE/live/side/summary.txt"
  if [ "$writer_exit" = "0" ] && [ "$reader_exit" != "0" ] && [ "$reader_state" = "42501" ] && [ "$cancel_state" = "57014" ]; then
    : > "$READY/priv.done"
    echo "priv_ok"
  else
    : > "$READY/side.failed"
    echo "priv_failed"
  fi
  set +e
}

run_copy() {
  docker cp "$FIXTURE/cluster/copy-out.sh" "$NAME":/tmp/a317-copy-out.sh
  fail=0
  for table in party account entry; do
    docker cp "$EVIDENCE/live/work/copy-${table}.sql" "$NAME":/tmp/a317-copy-${table}.sql
    if ! docker exec "$NAME" bash /tmp/a317-copy-out.sh "$table" > "$EVIDENCE/live/actual/${table}.copy" 2> "$EVIDENCE/live/actual/${table}.err"; then
      fail=1
    fi
  done
  if [ "$fail" -eq 0 ]; then
    : > "$READY/copy.ready"
    echo "copy_ok"
  else
    : > "$READY/copy.failed"
    echo "copy_failed"
  fi
}

run_restart() {
  if ! docker restart "$NAME" > "$EVIDENCE/pg/restart.id" 2> "$EVIDENCE/pg/restart.err"; then
    : > "$READY/restart.failed"
    echo "restart_failed"
    return
  fi
  ready=0
  for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30; do
    if docker exec -u postgres "$NAME" pg_isready -U oo_fixture_admin -d postgres >/dev/null 2>&1; then
      ready=1
      break
    fi
    sleep 2
  done
  if [ "$ready" -eq 1 ]; then
    date +%s > "$EVIDENCE/live/measurements/host-after-restart.epoch"
    : > "$READY/restarted.ready"
    echo "restart_ok"
  else
    docker logs "$NAME" > "$EVIDENCE/pg/restart-logs.txt" 2>&1 || true
    : > "$READY/restart.failed"
    echo "restart_failed"
  fi
}

while [ ! -f "$READY/stop" ]; do
  if [ -f "$READY/fixture.ready" ] && [ ! -f "$READY/load.started" ]; then
    : > "$READY/load.started"
    run_load
  fi
  if [ -f "$READY/inspect.done" ] && [ ! -f "$READY/stats.done" ]; then
    mkdir -p "$EVIDENCE/live/measurements"
    docker stats --no-stream --format 'cpu={{.CPUPerc}} mem={{.MemUsage}} mem_perc={{.MemPerc}} net={{.NetIO}} block={{.BlockIO}} pids={{.PIDs}}' "$NAME" > "$EVIDENCE/live/measurements/pg-stats.txt" 2> "$EVIDENCE/live/measurements/pg-stats.err" || true
    date +%s > "$EVIDENCE/live/measurements/host-after-inspect.epoch"
    : > "$READY/stats.done"
  fi
  if [ -f "$READY/inspect.done" ] && [ ! -f "$READY/priv.started" ]; then
    : > "$READY/priv.started"
    run_priv
  fi
  if [ -f "$READY/copy.wanted" ] && [ ! -f "$READY/copy.started" ]; then
    : > "$READY/copy.started"
    run_copy
  fi
  if [ -f "$READY/side.wanted" ] && [ ! -f "$READY/restart.started" ]; then
    : > "$READY/restart.started"
    run_restart
  fi
  sleep 1
done
echo "watch_stop"
