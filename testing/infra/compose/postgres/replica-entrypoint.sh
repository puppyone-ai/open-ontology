#!/bin/bash
# Build a streaming replica from the local primary. No published port.
set -euo pipefail

: "${PGDATA:=/var/lib/postgresql/data}"
mkdir -p "$PGDATA"
chown postgres:postgres "$PGDATA"
chmod 700 "$PGDATA"

# standby.signal is the completion marker. A partial basebackup has PG_VERSION
# and must be wiped and retried rather than started as a primary.
if [ ! -f "$PGDATA/standby.signal" ]; then
  pass=$(tr -d '\n' < /run/secrets/replication_password)
  if ! printf '%s' "$pass" | grep -Eq '^[0-9a-f]{48}$'; then
    echo "refusing non-hex replication secret" >&2
    exit 1
  fi
  export PGPASSWORD="$pass"
  ready=0
  for _ in $(seq 1 40); do
    if pg_isready -h pg-agent-primary -p 5432 -U oo_replicator >/dev/null 2>&1; then
      find "$PGDATA" -mindepth 1 -delete
      if gosu postgres pg_basebackup \
        -h pg-agent-primary \
        -p 5432 \
        -U oo_replicator \
        -D "$PGDATA" \
        -Fp -Xs -c fast; then
        ready=1
        break
      fi
      find "$PGDATA" -mindepth 1 -delete
    fi
    sleep 2
  done
  unset PGPASSWORD
  if [ "$ready" != 1 ]; then
    echo "replica basebackup failed" >&2
    exit 1
  fi
  umask 077
  printf 'pg-agent-primary:5432:*:oo_replicator:%s\n' "$pass" > "$PGDATA/pgpass"
  chown postgres:postgres "$PGDATA/pgpass"
  chmod 600 "$PGDATA/pgpass"
  printf "primary_conninfo = 'host=pg-agent-primary port=5432 user=oo_replicator passfile=%s/pgpass'\n" "$PGDATA" >> "$PGDATA/postgresql.auto.conf"
  chown postgres:postgres "$PGDATA/postgresql.auto.conf"
  touch "$PGDATA/standby.signal"
  chown postgres:postgres "$PGDATA/standby.signal"
  echo "replica_basebackup=ok"
fi

exec docker-entrypoint.sh postgres \
  -c hot_standby=on \
  -c hot_standby_feedback=on \
  -c log_statement=none \
  -c log_min_error_statement=error \
  -c log_connections=off
