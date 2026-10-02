#!/bin/bash
# Read role-GUC metadata. The password file is not printed.
set -eu
export PGAPPNAME=oo-a093-sslprobe
export PGPASSFILE=/tmp/a093-ident/guc.pgpass
CONN="host=oo-source-pg port=5432 dbname=source_lab user=oo_src_guc_login sslmode=verify-full sslrootcert=/var/lib/postgresql/tls/ca.crt"
echo "-- role row --"
psql "$CONN" -v ON_ERROR_STOP=1 -Atc "SELECT coalesce(name,'') || '|' || coalesce(source,'') || '|' || coalesce(setting,'') FROM pg_catalog.pg_settings WHERE name = 'role'"
echo "-- current_setting role --"
psql "$CONN" -v ON_ERROR_STOP=1 -Atc "SELECT pg_catalog.current_setting('role')"
echo "-- nondefault outside allow --"
psql "$CONN" -v ON_ERROR_STOP=1 -Atc "SELECT name || '|' || source FROM pg_catalog.pg_settings WHERE source NOT IN ('default','override','configuration file') AND name <> ALL(ARRAY['DateStyle','client_encoding','TimeZone','extra_float_digits','application_name']::text[]) ORDER BY 1"
echo "-- db role setting --"
psql "$CONN" -v ON_ERROR_STOP=1 -Atc "SELECT r.rolname || '|' || s.setdatabase::text || '|' || u.cfg FROM pg_catalog.pg_roles r JOIN pg_catalog.pg_db_role_setting s ON s.setrole = r.oid CROSS JOIN LATERAL unnest(s.setconfig) AS u(cfg) WHERE r.rolname = SESSION_USER::text"
echo "-- sources present --"
psql "$CONN" -v ON_ERROR_STOP=1 -Atc "SELECT source || '|' || count(*)::text FROM pg_catalog.pg_settings GROUP BY source ORDER BY source"
echo "-- proposed extra guc --"
psql "$CONN" -v ON_ERROR_STOP=1 -Atc "SELECT COALESCE((SELECT pg_catalog.string_agg(name, ',' ORDER BY name) FROM (SELECT s.name FROM pg_catalog.pg_settings s WHERE s.source IN ('client','session','startup','user','database','database user') AND s.name <> ALL(ARRAY['DateStyle','client_encoding','TimeZone','extra_float_digits','application_name']::text[]) UNION SELECT 'role' WHERE pg_catalog.current_setting('role') IS DISTINCT FROM 'none') AS extra(name)), '')"
export PGPASSFILE=/tmp/a093-ident/reader.pgpass
RCONN="host=oo-source-pg port=5432 dbname=source_lab user=oo_src_reader sslmode=verify-full sslrootcert=/var/lib/postgresql/tls/ca.crt"
echo "-- reader role setting --"
psql "$RCONN" -v ON_ERROR_STOP=1 -Atc "SELECT session_user || '|' || current_user || '|' || pg_catalog.current_setting('role')"
echo "-- proposed extra reader --"
psql "$RCONN" -v ON_ERROR_STOP=1 -Atc "SELECT COALESCE((SELECT pg_catalog.string_agg(name, ',' ORDER BY name) FROM (SELECT s.name FROM pg_catalog.pg_settings s WHERE s.source IN ('client','session','startup','user','database','database user') AND s.name <> ALL(ARRAY['DateStyle','client_encoding','TimeZone','extra_float_digits','application_name']::text[]) UNION SELECT 'role' WHERE pg_catalog.current_setting('role') IS DISTINCT FROM 'none') AS extra(name)), '')"
