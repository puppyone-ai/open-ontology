#!/bin/bash
# Approved local PG46 fixture deployment role correction; never touches source.
set -euo pipefail
container=oo-dl01w1-46-tgt
docker exec "$container" psql -U postgres -d dst -v ON_ERROR_STOP=1 <<'SQL'
BEGIN;
GRANT oo_pipeline_runtime TO oo_dl01w1_46_target WITH INHERIT TRUE, SET TRUE;
SELECT r.rolname, m.inherit_option, m.set_option, m.admin_option
FROM pg_auth_members m JOIN pg_roles r ON r.oid=m.roleid
JOIN pg_roles u ON u.oid=m.member WHERE u.rolname='oo_dl01w1_46_target';
SELECT has_table_privilege('oo_dl01w1_46_target','_oo.schema_migrations','SELECT') AS migration_select;
COMMIT;
SQL
