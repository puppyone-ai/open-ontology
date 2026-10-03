#!/bin/bash
# Peer auth inside the fixture. Role passwords are read from files, not argv.
# Role names stay oo_a131_reader and oo_a131_writer because the immutable
# original tests filter the log on user=oo_a131_reader.
set -eu
export PGHOST=/var/run/postgresql
export PGUSER=postgres
export PGDATABASE=oo_a134
reader=$(cat /secrets/reader.pass)
writer=$(cat /secrets/writer.pass)
have_reader=$(psql -At -c "SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_a131_reader'")
if [ "$have_reader" != "1" ]; then
  umask 077
  cat > /tmp/a134-roles.sql <<EOF
CREATE ROLE oo_a131_reader LOGIN PASSWORD '${reader}';
CREATE ROLE oo_a131_writer LOGIN PASSWORD '${writer}';
EOF
  chmod 600 /tmp/a134-roles.sql
  psql -v ON_ERROR_STOP=1 -f /tmp/a134-roles.sql
  rm -f /tmp/a134-roles.sql
fi
unset reader writer
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
rev_parent=$(psql -At -c "SELECT count(*) FROM rev.parent")
rev_child=$(psql -At -c "SELECT count(*) FROM rev.child")
rev_zero=$(psql -At -c "SELECT count(*) FROM rev.zerocol")
rev_split=$(psql -At -c "SELECT count(*) FROM rev.split")
if [ "$parent" != "4" ] || [ "$child" != "3" ] || [ "$empty" != "0" ] || [ "$zero" != "2" ] || [ "$wide" != "6" ] || [ "$rowv" != "7" ] || [ "$rev_parent" != "3" ] || [ "$rev_child" != "3" ] || [ "$rev_zero" != "2" ] || [ "$rev_split" != "2" ]; then
  echo "seed mismatch parent=$parent child=$child empty=$empty zero=$zero wide=$wide row=$rowv rev_parent=$rev_parent rev_child=$rev_child rev_zero=$rev_zero rev_split=$rev_split" >&2
  exit 1
fi
reader_insert=$(psql -At -c "SELECT has_table_privilege('oo_a131_reader', 'stream.parent', 'INSERT')")
reader_select=$(psql -At -c "SELECT has_table_privilege('oo_a131_reader', 'stream.parent', 'SELECT')")
reader_zero=$(psql -At -c "SELECT has_table_privilege('oo_a131_reader', 'stream.zerocol', 'SELECT')")
reader_rev=$(psql -At -c "SELECT has_table_privilege('oo_a131_reader', 'rev.parent', 'INSERT')")
writer_insert=$(psql -At -c "SELECT has_table_privilege('oo_a131_writer', 'stream.parent', 'INSERT')")
writer_select=$(psql -At -c "SELECT has_table_privilege('oo_a131_writer', 'stream.child', 'SELECT')")
if [ "$reader_insert" != "f" ] || [ "$reader_select" != "t" ] || [ "$reader_zero" != "t" ] || [ "$reader_rev" != "f" ] || [ "$writer_insert" != "t" ] || [ "$writer_select" != "f" ]; then
  echo "privilege mismatch" >&2
  exit 1
fi
echo "init_ok version=$version"
