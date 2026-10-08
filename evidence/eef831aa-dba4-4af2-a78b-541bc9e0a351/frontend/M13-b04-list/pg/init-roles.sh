#!/bin/bash
# Runs inside the owned PostgreSQL container as root. The password is not printed.
set -eu
pw=$(tr -d '\r\n' < /secrets/login.pass)
sb=$(tr -d '\r\n' < /secrets/selfbuild.pass)
if [ -z "$pw" ] || [ -z "$sb" ]; then
  echo fail
  exit 1
fi
umask 077
sql=/tmp/oo-fe01-m13-init-roles.sql
if su -s /bin/bash postgres -c "psql -U oo_fixture_admin -d postgres -Atqc \"SELECT count(*) FROM pg_roles WHERE rolname IN ('oo_identity_runtime','oo_identity_admin','oo_selfbuild_api_admin')\"" | grep -q '^3$'; then
  echo ok
  exit 0
fi
cat > "$sql" <<EOF
CREATE ROLE oo_identity_runtime NOLOGIN;
CREATE ROLE oo_identity_admin LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE INHERIT IN ROLE oo_identity_runtime PASSWORD '${pw}';
CREATE DATABASE identity OWNER oo_identity_admin;
CREATE ROLE oo_selfbuild_api_admin LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE PASSWORD '${sb}';
CREATE DATABASE selfbuild OWNER oo_selfbuild_api_admin;
CREATE DATABASE ledger OWNER oo_selfbuild_api_admin;
EOF
chown postgres:postgres "$sql"
chmod 600 "$sql"
if su -s /bin/bash postgres -c "psql -U oo_fixture_admin -d postgres -v ON_ERROR_STOP=1 -f /tmp/oo-fe01-m13-init-roles.sql" >/tmp/oo-fe01-m13-psql.out 2>/tmp/oo-fe01-m13-psql.err; then
  rm -f "$sql" /tmp/oo-fe01-m13-psql.out /tmp/oo-fe01-m13-psql.err
  echo ok
else
  rm -f "$sql" /tmp/oo-fe01-m13-psql.out /tmp/oo-fe01-m13-psql.err
  echo fail
  exit 1
fi
