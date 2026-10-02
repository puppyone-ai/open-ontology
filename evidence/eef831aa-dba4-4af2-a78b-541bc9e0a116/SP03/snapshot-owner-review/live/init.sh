#!/bin/bash
# Inside the fixture container as the postgres OS user. Peer auth. No argv secrets.
# Enables statement logging without truncating the log, then writes the preregistered cut.
set -eu
export PGHOST=/var/run/postgresql
export PGUSER=postgres
export PGDATABASE=revdb
reader=$(cat /secrets/reader.pass)
writer=$(cat /secrets/writer.pass)
guc=$(cat /secrets/guc.pass)
have_reader=$(psql -At -c "SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_rev_reader'")
if [ "$have_reader" != "1" ]; then
  umask 077
  cat > /tmp/a116-roles.sql <<EOF
CREATE ROLE oo_rev_reader LOGIN PASSWORD '${reader}';
CREATE ROLE oo_rev_writer LOGIN PASSWORD '${writer}';
CREATE ROLE oo_rev_guc LOGIN PASSWORD '${guc}';
EOF
  chmod 600 /tmp/a116-roles.sql
  psql -v ON_ERROR_STOP=1 -f /tmp/a116-roles.sql
  psql -v ON_ERROR_STOP=1 -f /fixture/init.sql
  rm -f /tmp/a116-roles.sql
fi
unset reader writer guc

psql -v ON_ERROR_STOP=1 -c "ALTER SYSTEM SET log_statement = 'all'"
psql -v ON_ERROR_STOP=1 -c "SELECT pg_catalog.pg_reload_conf()"
sleep 1

fail() {
  echo "bind_fail=$1" >&2
  exit 1
}

got_item=$(psql -At -F $'\t' -c "SELECT a.attnum::text, a.attname, a.atttypid::text, a.atttypmod::text, CASE WHEN a.attnotnull THEN 'true' ELSE 'false' END FROM pg_catalog.pg_attribute a JOIN pg_catalog.pg_class c ON c.oid = a.attrelid JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = 'rev' AND c.relname = 'item' AND a.attnum > 0 AND NOT a.attisdropped ORDER BY a.attnum")
want_item=$'1\tid\t23\t-1\ttrue\n2\tlabel\t25\t-1\ttrue\n3\tflag\t16\t-1\ttrue\n4\tcode\t1043\t12\ttrue\n5\todd"note\t25\t-1\tfalse'
if [ "$got_item" != "$want_item" ]; then
  printf '%s\n' "$got_item" > /tmp/a116-item-cols.txt
  fail item_columns
fi

got_line=$(psql -At -F $'\t' -c "SELECT a.attnum::text, a.attname, a.atttypid::text, a.atttypmod::text, CASE WHEN a.attnotnull THEN 'true' ELSE 'false' END FROM pg_catalog.pg_attribute a JOIN pg_catalog.pg_class c ON c.oid = a.attrelid JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = 'rev' AND c.relname = 'line item' AND a.attnum > 0 AND NOT a.attisdropped ORDER BY a.attnum")
want_line=$'1\tid\t23\t-1\ttrue\n2\titem_id\t23\t-1\ttrue\n3\tqty\t21\t-1\ttrue\n4\tmemo\t25\t-1\tfalse'
if [ "$got_line" != "$want_line" ]; then
  printf '%s\n' "$got_line" > /tmp/a116-line-cols.txt
  fail line_columns
fi

row7=$(psql -At -F $'\t' -c "SELECT id::text, label, CASE WHEN flag THEN 'true' ELSE 'false' END, code, COALESCE(\"odd\"\"note\", '<null>') FROM rev.item WHERE id = 7")
if [ "$row7" != $'7\tcedar\ttrue\tA116\t<null>' ]; then
  fail row7
fi
row21=$(psql -At -F $'\t' -c "SELECT id::text, item_id::text, qty::text, COALESCE(memo, '<null>') FROM rev.\"line item\" WHERE id = 21")
if [ "$row21" != $'21\t7\t4\tsprig' ]; then
  fail row21
fi
row22=$(psql -At -F $'\t' -c "SELECT id::text, item_id::text, qty::text, COALESCE(memo, '<null>') FROM rev.\"line item\" WHERE id = 22")
if [ "$row22" != $'22\t7\t5\t<null>' ]; then
  fail row22
fi
absent=$(psql -At -c "SELECT count(*) FROM rev.\"line item\" WHERE id = 99")
if [ "$absent" != "0" ]; then
  fail absent99
fi
twin=$(psql -At -c "SELECT count(*) FROM rev.twin WHERE id = 3")
if [ "$twin" != "2" ]; then
  fail twin_count
fi
labels=$(psql -At -c "SELECT string_agg(label, ',' ORDER BY label) FROM rev.twin WHERE id = 3")
if [ "$labels" != "one,two" ]; then
  fail twin_labels
fi
canary=$(psql -At -c "SELECT n FROM rev.canary WHERE id = 1")
if [ "$canary" != "0" ]; then
  fail canary
fi
secdef=$(psql -At -c "SELECT prosecdef FROM pg_catalog.pg_proc p JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace WHERE n.nspname = 'rev' AND p.proname = 'bump'")
if [ "$secdef" != "f" ]; then
  fail bump_secdef
fi
insert_priv=$(psql -At -c "SELECT has_table_privilege('oo_rev_reader', 'rev.item', 'INSERT')")
select_priv=$(psql -At -c "SELECT has_table_privilege('oo_rev_reader', 'rev.item', 'SELECT')")
if [ "$insert_priv" != "f" ] || [ "$select_priv" != "t" ]; then
  fail reader_privileges
fi
version=$(psql -At -c "SHOW server_version")
case "$version" in
  17.10*) ;;
  *) fail server_version ;;
esac
recovery=$(psql -At -c "SELECT pg_catalog.pg_is_in_recovery()")
if [ "$recovery" != "f" ]; then
  fail recovery
fi
postmaster=$(psql -At -c "SELECT pg_catalog.pg_postmaster_start_time()::text")
if [ -z "$postmaster" ]; then
  fail postmaster
fi

psql -v ON_ERROR_STOP=1 -c "SELECT 'A116_AUDIT_WINDOW_START'" >/dev/null
sleep 1
if ! grep -F -q "A116_AUDIT_WINDOW_START" /pglog/postgresql.log; then
  fail audit_marker_missing
fi
if [ ! -s /pglog/postgresql.log ]; then
  fail audit_empty
fi
leak=0
for file in /secrets/reader.pass /secrets/writer.pass /secrets/guc.pass /secrets/super.pass; do
  secret=$(cat "$file")
  if [ -n "$secret" ] && grep -F -q -- "$secret" /pglog/postgresql.log; then
    leak=1
  fi
done
if [ "$leak" -ne 0 ]; then
  fail log_leak
fi
echo "log_leak=no"

umask 077
cat > /tmp/a116-cut.txt <<EOF
META	postmaster	${postmaster}
META	database	revdb
META	principal	oo_rev_reader
META	in_recovery	false
META	version	${version}
COL	rev	item	1	id	23	-1	true
COL	rev	item	2	label	25	-1	true
COL	rev	item	3	flag	16	-1	true
COL	rev	item	4	code	1043	12	true
COL	rev	item	5	odd"note	25	-1	false
PK	rev	item	id
COL	rev	line item	1	id	23	-1	true
COL	rev	line item	2	item_id	23	-1	true
COL	rev	line item	3	qty	21	-1	true
COL	rev	line item	4	memo	25	-1	false
PK	rev	line item	id
ROW	item	7	int:7	text:cedar	bool:true	text:A116	null
ROW	line item	21	int:21	int:7	int:4	text:sprig
ROW	line item	22	int:22	int:7	int:5	null
ABSENT	line item	99
FRESH	item	7	int:7	text:willow	bool:true	text:A116	text:kept
FRESH	line item	21	int:21	int:7	int:9	text:sprig
FRESH	line item	22	int:22	int:7	int:5	null
FRESH	line item	23	int:23	int:7	int:1	text:fresh
WRITER	1	UPDATE rev.item SET label = 'willow', "odd""note" = 'kept' WHERE id = 7
WRITER	2	UPDATE rev."line item" SET qty = 9 WHERE id = 21
WRITER	3	INSERT INTO rev."line item" (id, item_id, qty, memo) VALUES (23, 7, 1, 'fresh')
EXPECT	canary_before	0
EXPECT	writer_canary_delta	3
EXPECT	twin_count	2
MARKER	A116_AUDIT_WINDOW_START
EOF
chmod 644 /tmp/a116-cut.txt
echo "cut_ready"
