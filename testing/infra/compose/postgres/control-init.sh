#!/bin/bash
# First-boot marker for the local control fixture. No product schema.
set -euo pipefail

psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" <<'SQL'
CREATE SCHEMA oo_fixture;
CREATE TABLE oo_fixture.ownership (
    id integer PRIMARY KEY,
    fixture text NOT NULL,
    note text NOT NULL
);
INSERT INTO oo_fixture.ownership (id, fixture, note)
VALUES (1, 'pg-control', 'synthetic local control fixture');
REVOKE ALL ON SCHEMA oo_fixture FROM PUBLIC;
SQL
