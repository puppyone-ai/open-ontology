#!/bin/bash
# Run psql as the fixture admin. The password is never an argument.
# Local trust is tried first. The fallback reads /secrets/admin.pass into a
# mode-600 pgpass file and unsets the shell variable before psql starts.
# Origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a231/SP12/sql-runner/pg/psql-admin.sh
# Delta: temporary names are a244.
set -euo pipefail
unset PGPASSWORD POSTGRES_PASSWORD || true
export PGHOST=/var/run/postgresql
export PGUSER=oo_fixture_admin
export PGDATABASE="${PGDATABASE:-oo_a198}"
if psql -w -v ON_ERROR_STOP=1 -c 'SELECT 1' >/dev/null 2>/tmp/oo-a244-psql-try.err; then
  printf '%s\n' local > /tmp/oo-a244-auth-mode
  exec psql -w -v ON_ERROR_STOP=1 "$@"
fi
umask 077
passfile=$(mktemp)
trap 'rm -f "$passfile"' EXIT
pass=$(awk 'NR==1 { sub(/\r$/, ""); print; exit }' /secrets/admin.pass)
printf '*:*:*:%s:%s\n' "$PGUSER" "$pass" > "$passfile"
unset pass
chmod 600 "$passfile"
export PGPASSFILE="$passfile"
printf '%s\n' pgpass > /tmp/oo-a244-auth-mode
psql -w -v ON_ERROR_STOP=1 "$@"
status=$?
exit "$status"
