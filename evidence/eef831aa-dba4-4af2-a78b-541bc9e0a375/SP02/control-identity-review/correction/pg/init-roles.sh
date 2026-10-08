#!/bin/bash
# Runs inside the owned PostgreSQL container as root. The password is read from the
# mounted file and is not printed. This script does not TRUNCATE and does not grant data roles.
set -eu
pw=$(tr -d '\r\n' < /secrets/login.pass)
if [ -z "$pw" ]; then
  echo fail
  exit 1
fi
umask 077
sql=/tmp/oo-a375c-init-roles.sql
if su -s /bin/bash postgres -c "psql -U oo_fixture_admin -d postgres -Atqc \"SELECT count(*) FROM pg_roles WHERE rolname IN ('oo_identity_runtime','oo_identity_admin')\"" | grep -q '^2$'; then
  echo ok
  exit 0
fi
cat > "$sql" <<EOF
CREATE ROLE oo_identity_runtime NOLOGIN;
CREATE ROLE oo_identity_admin LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE INHERIT IN ROLE oo_identity_runtime PASSWORD '${pw}';
CREATE DATABASE identity OWNER oo_identity_admin;
EOF
chown postgres:postgres "$sql"
chmod 600 "$sql"
if su -s /bin/bash postgres -c "psql -U oo_fixture_admin -d postgres -v ON_ERROR_STOP=1 -f /tmp/oo-a375c-init-roles.sql" >/tmp/oo-a375c-psql.out 2>/tmp/oo-a375c-psql.err; then
  rm -f "$sql" /tmp/oo-a375c-psql.out /tmp/oo-a375c-psql.err
  echo ok
else
  rm -f "$sql" /tmp/oo-a375c-psql.out /tmp/oo-a375c-psql.err
  echo fail
  exit 1
fi
