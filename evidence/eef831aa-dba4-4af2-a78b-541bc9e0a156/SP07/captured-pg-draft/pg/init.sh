#!/bin/bash
# Peer auth inside the fixture. The reader password is read from a file, not argv.
# Statement logging is enabled and the log truncated only after this DDL.
set -eu
export PGHOST=/var/run/postgresql
export PGUSER=postgres
export PGDATABASE=oo_a156
reader=$(cat /secrets/reader.pass)
reader=${reader//$'\r'/}
reader=${reader//$'\n'/}
have_reader=$(psql -At -c "SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_a156_reader'")
if [ "$have_reader" != "1" ]; then
  umask 077
  cat > /tmp/a156-init.sql <<EOF
CREATE ROLE a156_owner NOLOGIN;
CREATE ROLE oo_a156_reader LOGIN PASSWORD '${reader}';
GRANT a156_owner TO postgres;
CREATE SCHEMA shop AUTHORIZATION a156_owner;
SET ROLE a156_owner;
CREATE TABLE shop.customer (
  tenant_id int4 NOT NULL,
  id int4 NOT NULL,
  email text,
  PRIMARY KEY (tenant_id, id)
);
COMMENT ON COLUMN shop.customer.email IS 'a156-live-comment-body-do-not-carry';
CREATE TABLE shop.purchase (
  tenant_id int4 NOT NULL,
  id int4 NOT NULL,
  customer_id int4 NOT NULL,
  PRIMARY KEY (tenant_id, id),
  CONSTRAINT purchase_customer_fk FOREIGN KEY (tenant_id, customer_id)
    REFERENCES shop.customer (tenant_id, id)
);
RESET ROLE;
GRANT USAGE ON SCHEMA shop TO oo_a156_reader;
GRANT SELECT ON shop.customer, shop.purchase TO oo_a156_reader;
EOF
  chmod 600 /tmp/a156-init.sql
  psql -v ON_ERROR_STOP=1 -f /tmp/a156-init.sql
  rm -f /tmp/a156-init.sql
fi
unset reader
psql -v ON_ERROR_STOP=1 -c "ALTER ROLE oo_a156_reader RESET log_statement"
psql -v ON_ERROR_STOP=1 -c "ALTER SYSTEM SET log_statement = 'all'"
psql -v ON_ERROR_STOP=1 -c "SELECT pg_catalog.pg_reload_conf()"
got=""
for _ in 1 2 3 4 5 6 7 8 9 10; do
  got=$(psql -At -c "SHOW log_statement")
  if [ "$got" = "all" ]; then
    break
  fi
  sleep 1
done
if [ "$got" != "all" ]; then
  echo "log_statement=${got}" >&2
  exit 1
fi
: > /pglog/postgresql.log
version=$(psql -At -c "SHOW server_version")
case "$version" in
  17.10*) ;;
  *) echo "server_version=${version}" >&2; exit 1 ;;
esac
can_insert=$(psql -At -c "SELECT pg_catalog.has_table_privilege('oo_a156_reader', 'shop.customer', 'INSERT')")
can_select=$(psql -At -c "SELECT pg_catalog.has_table_privilege('oo_a156_reader', 'shop.customer', 'SELECT')")
owner=$(psql -At -c "SELECT pg_catalog.pg_get_userbyid(c.relowner) FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = 'shop' AND c.relname = 'customer'")
if [ "$can_insert" != "f" ] || [ "$can_select" != "t" ] || [ "$owner" != "a156_owner" ]; then
  echo "privilege or owner mismatch select=${can_select} insert=${can_insert} owner=${owner}" >&2
  exit 1
fi
echo "init_ok version=${version} log_statement=${got}"
