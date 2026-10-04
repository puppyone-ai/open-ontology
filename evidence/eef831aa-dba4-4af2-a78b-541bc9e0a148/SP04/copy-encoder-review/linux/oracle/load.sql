\set ON_ERROR_STOP on
\pset pager off
\pset tuples_only on
\pset format unaligned
\pset fieldsep '|'
SET TimeZone TO 'UTC';
\o /tmp/a148-version.txt
SELECT current_setting('server_version') || '|' || current_setting('server_version_num');
\o
BEGIN;
CREATE SCHEMA oo_copy_a148;
CREATE TABLE oo_copy_a148.fresh (
  pos numeric(38,0) NOT NULL,
  raw bytea,
  note text,
  flag boolean NOT NULL,
  n bigint NOT NULL,
  small smallint NOT NULL,
  wide integer NOT NULL,
  amount numeric(12,4) NOT NULL,
  day date NOT NULL,
  frac double precision NOT NULL,
  narrow real NOT NULL,
  clock time NOT NULL,
  ts timestamp NOT NULL,
  tstz timestamptz NOT NULL,
  doc json,
  token uuid,
  label varchar(4),
  code character(4)
);
\copy oo_copy_a148.fresh (pos, raw, note, flag, n, small, wide, amount, day, frac, narrow, clock, ts, tstz, doc, token, label, code) FROM '/tmp/fresh.copy' WITH (FORMAT binary)
\o /tmp/a148-count.txt
SELECT count(*)::text FROM oo_copy_a148.fresh;
\o
\o /tmp/a148-schema.txt
SELECT a.attnum::text || '|' || a.attname || '|' || format_type(a.atttypid, a.atttypmod) || '|' || a.attnotnull::text
FROM pg_attribute a
WHERE a.attrelid = 'oo_copy_a148.fresh'::regclass
  AND a.attnum > 0
  AND NOT a.attisdropped
ORDER BY a.attnum;
\o
\o /tmp/a148-rows.txt
SELECT
  pos::text,
  CASE WHEN raw IS NULL THEN '<NULL>' ELSE encode(raw, 'hex') END,
  CASE WHEN note IS NULL THEN '<NULL>' ELSE replace(replace(note, E'\t', E'\\t'), E'\n', E'\\n') END,
  flag::text,
  n::text,
  small::text,
  wide::text,
  amount::text,
  day::text,
  encode(float8send(frac), 'hex'),
  encode(float4send(narrow), 'hex'),
  to_char(clock, 'HH24:MI:SS.US'),
  to_char(ts, 'YYYY-MM-DD HH24:MI:SS.US'),
  to_char(tstz AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS.US'),
  CASE WHEN doc IS NULL THEN '<NULL>' ELSE doc::text END,
  CASE WHEN token IS NULL THEN '<NULL>' ELSE token::text END,
  CASE WHEN label IS NULL THEN '<NULL>' WHEN label = '' THEN '<EMPTY>' ELSE label END,
  CASE WHEN code IS NULL THEN '<NULL>' ELSE rtrim(code::text) END,
  CASE WHEN code IS NULL THEN '<NULL>' ELSE octet_length(code)::text END,
  CASE WHEN label IS NULL THEN '<NULL>' ELSE octet_length(label)::text END
FROM oo_copy_a148.fresh
ORDER BY pos;
\o
ROLLBACK;
\o /tmp/a148-rollback.txt
SELECT CASE WHEN to_regclass('oo_copy_a148.fresh') IS NULL THEN 'null' ELSE 'present' END;
\o
