#!/bin/bash
# Peer auth inside the fixture. Role passwords are read from files, not argv.
set -eu
export PGHOST=/var/run/postgresql
export PGUSER=postgres
export PGDATABASE=oo_a146
reader=$(cat /secrets/reader.pass)
writer=$(cat /secrets/writer.pass)
have_reader=$(psql -At -c "SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_a131_reader'")
if [ "$have_reader" != "1" ]; then
  umask 077
  cat > /tmp/a146-roles.sql <<EOF
CREATE ROLE oo_a131_reader LOGIN PASSWORD '${reader}';
CREATE ROLE oo_a131_writer LOGIN PASSWORD '${writer}';
EOF
  chmod 600 /tmp/a146-roles.sql
  psql -v ON_ERROR_STOP=1 -f /tmp/a146-roles.sql
  rm -f /tmp/a146-roles.sql
fi
unset reader writer
psql -v ON_ERROR_STOP=1 -c "ALTER ROLE oo_a131_reader RESET log_statement"
psql -v ON_ERROR_STOP=1 -f /fixture/init.sql
psql -v ON_ERROR_STOP=1 -c "ALTER SYSTEM SET log_statement = 'all'"
psql -v ON_ERROR_STOP=1 -c "SELECT pg_catalog.pg_reload_conf()"
: > /pglog/postgresql.log
version=$(psql -At -c "SHOW server_version")
case "$version" in
  17.10*) ;;
  *) echo "server_version=$version" >&2; exit 1 ;;
esac
parent=$(psql -At -c "SELECT count(*) FROM stream.parent")
child=$(psql -At -c "SELECT count(*) FROM stream.child")
empty=$(psql -At -c "SELECT count(*) FROM stream.empty")
zero=$(psql -At -c "SELECT count(*) FROM stream.zerocol")
wide=$(psql -At -c "SELECT length(body) FROM stream.bounds_wide")
rowv=$(psql -At -c "SELECT id FROM stream.bounds_row")
acl_a=$(psql -At -c "SELECT id FROM acl.acl_a")
acl_b=$(psql -At -c "SELECT count(*)::int8 FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = 'acl' AND c.relname = 'acl_b'")
if [ "$parent" != "4" ] || [ "$child" != "3" ] || [ "$empty" != "0" ] || [ "$zero" != "2" ] || [ "$wide" != "6" ] || [ "$rowv" != "7" ] || [ "$acl_a" != "7" ] || [ "$acl_b" != "1" ]; then
  echo "seed mismatch parent=$parent child=$child empty=$empty zero=$zero wide=$wide row=$rowv acl_a=$acl_a acl_b=$acl_b" >&2
  exit 1
fi
reader_insert=$(psql -At -c "SELECT has_table_privilege('oo_a131_reader', 'stream.parent', 'INSERT')")
reader_select=$(psql -At -c "SELECT has_table_privilege('oo_a131_reader', 'stream.parent', 'SELECT')")
reader_acl=$(psql -At -c "SELECT has_table_privilege('oo_a131_reader', 'acl.acl_b', 'SELECT')")
writer_insert=$(psql -At -c "SELECT has_table_privilege('oo_a131_writer', 'stream.parent', 'INSERT')")
if [ "$reader_insert" != "f" ] || [ "$reader_select" != "t" ] || [ "$reader_acl" != "t" ] || [ "$writer_insert" != "t" ]; then
  echo "privilege mismatch" >&2
  exit 1
fi
echo "init_ok version=$version"
