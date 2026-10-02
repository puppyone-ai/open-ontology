#!/bin/bash
# Apply the synthetic catalog as the local superuser. Passwords stay in files.
set -eu
umask 077
export PGAPPNAME=oo-pg-fixture
export OO_READER_PASSWORD="$(cat /secrets/reader.pass)"
export OO_INHERIT_PASSWORD="$(cat /secrets/inherit.pass)"
export OO_SETONLY_PASSWORD="$(cat /secrets/setonly.pass)"
export OO_DEFINER_PASSWORD="$(cat /secrets/definer.pass)"
export OO_SIDE_PASSWORD="$(cat /secrets/side.pass)"
export OO_HIDDENFN_PASSWORD="$(cat /secrets/hiddenfn.pass)"
export OO_SCHEMACREATE_PASSWORD="$(cat /secrets/schemacreate.pass)"
export OO_DBCREATE_PASSWORD="$(cat /secrets/dbcreate.pass)"
export OO_DUAL_PASSWORD="$(cat /secrets/dual.pass)"
export OO_INHERIT_ONLY_PASSWORD="$(cat /secrets/inherit-only.pass)"
export OO_TRANSITIVE_PASSWORD="$(cat /secrets/transitive.pass)"
export OO_NOSET_PASSWORD="$(cat /secrets/noset.pass)"
export OO_COLUMN_PASSWORD="$(cat /secrets/column.pass)"
export OO_SUPER_PASSWORD="$(cat /secrets/login-super.pass)"
export OO_BYPASS_PASSWORD="$(cat /secrets/bypass.pass)"
export OO_CREATEROLE_PASSWORD="$(cat /secrets/createrole.pass)"
export OO_CREATEDB_PASSWORD="$(cat /secrets/createdb.pass)"
export OO_OPONLY_PASSWORD="$(cat /secrets/oponly.pass)"
export OO_CASTONLY_PASSWORD="$(cat /secrets/castonly.pass)"
gosu postgres psql -v ON_ERROR_STOP=1 -f /fixture/init.sql
gosu postgres psql -v ON_ERROR_STOP=1 -c "ALTER SYSTEM SET log_statement = 'all'" -c "SELECT pg_catalog.pg_reload_conf()"
