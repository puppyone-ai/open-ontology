CREATE SCHEMA IF NOT EXISTS stream;
CREATE TABLE IF NOT EXISTS stream.parent (
  id int4 PRIMARY KEY,
  flag bool NOT NULL,
  label text
);
CREATE TABLE IF NOT EXISTS stream.child (
  parent_id int4,
  note text
);
CREATE TABLE IF NOT EXISTS stream.empty (
  id int4 PRIMARY KEY
);
CREATE TABLE IF NOT EXISTS stream.zerocol ();
CREATE TABLE IF NOT EXISTS stream.bounds_wide (
  id int4,
  body text
);
CREATE TABLE IF NOT EXISTS stream.bounds_row (
  id int4
);

INSERT INTO stream.parent (id, flag, label)
SELECT v.id, v.flag, v.label
FROM (VALUES
  (1, true, 'alpha'::text),
  (2, false, NULL),
  (3, true, 'gamma'),
  (4, false, 'delta')
) AS v(id, flag, label)
WHERE NOT EXISTS (SELECT 1 FROM stream.parent p WHERE p.id = v.id);

INSERT INTO stream.child (parent_id, note)
SELECT v.parent_id, v.note
FROM (VALUES
  (1, 'c1'::text),
  (NULL, 'orphan'),
  (2, NULL)
) AS v(parent_id, note)
WHERE NOT EXISTS (
  SELECT 1 FROM stream.child c
  WHERE c.parent_id IS NOT DISTINCT FROM v.parent_id
    AND c.note IS NOT DISTINCT FROM v.note
);

INSERT INTO stream.zerocol
SELECT WHERE (SELECT count(*) FROM stream.zerocol) = 0;
INSERT INTO stream.zerocol
SELECT WHERE (SELECT count(*) FROM stream.zerocol) = 1;

INSERT INTO stream.bounds_wide (id, body)
SELECT 1, 'abcdef'
WHERE NOT EXISTS (SELECT 1 FROM stream.bounds_wide);

INSERT INTO stream.bounds_row (id)
SELECT 7
WHERE NOT EXISTS (SELECT 1 FROM stream.bounds_row);

CREATE SCHEMA IF NOT EXISTS rev;
CREATE TABLE IF NOT EXISTS rev.parent (
  id int4 PRIMARY KEY,
  flag bool NOT NULL,
  label text
);
CREATE TABLE IF NOT EXISTS rev.child (
  parent_id int4,
  note text
);
CREATE TABLE IF NOT EXISTS rev.empty (
  id int4 PRIMARY KEY
);
CREATE TABLE IF NOT EXISTS rev.zerocol ();
CREATE TABLE IF NOT EXISTS rev.split (
  body text
);
CREATE TABLE IF NOT EXISTS rev.oversize (
  body text
);
CREATE TABLE IF NOT EXISTS rev.numkey (
  id numeric PRIMARY KEY,
  payload int4 NOT NULL
);
CREATE TABLE IF NOT EXISTS rev.schema_a (
  id int4 PRIMARY KEY
);
CREATE TABLE IF NOT EXISTS rev.schema_b (
  id int4 PRIMARY KEY
);
CREATE TABLE IF NOT EXISTS rev.acl_a (
  id int4 PRIMARY KEY
);
CREATE TABLE IF NOT EXISTS rev.acl_b (
  id int4 PRIMARY KEY
);

INSERT INTO rev.parent (id, flag, label)
SELECT v.id, v.flag, v.label
FROM (VALUES
  (10, true, 'red'::text),
  (11, false, NULL),
  (12, true, 'blue')
) AS v(id, flag, label)
WHERE NOT EXISTS (SELECT 1 FROM rev.parent p WHERE p.id = v.id);

INSERT INTO rev.child (parent_id, note)
SELECT v.parent_id, v.note
FROM (VALUES
  (10, 'one'::text),
  (NULL, 'none'),
  (11, NULL)
) AS v(parent_id, note)
WHERE NOT EXISTS (
  SELECT 1 FROM rev.child c
  WHERE c.parent_id IS NOT DISTINCT FROM v.parent_id
    AND c.note IS NOT DISTINCT FROM v.note
);

INSERT INTO rev.zerocol
SELECT WHERE (SELECT count(*) FROM rev.zerocol) = 0;
INSERT INTO rev.zerocol
SELECT WHERE (SELECT count(*) FROM rev.zerocol) = 1;

INSERT INTO rev.split (body)
SELECT v.body
FROM (VALUES ('aa'::text), ('bbbb')) AS v(body)
WHERE NOT EXISTS (SELECT 1 FROM rev.split);

INSERT INTO rev.oversize (body)
SELECT 'zzzzzz'
WHERE NOT EXISTS (SELECT 1 FROM rev.oversize);

INSERT INTO rev.numkey (id, payload)
SELECT 1, 42
WHERE NOT EXISTS (SELECT 1 FROM rev.numkey);

INSERT INTO rev.schema_a (id) SELECT 1 WHERE NOT EXISTS (SELECT 1 FROM rev.schema_a);
INSERT INTO rev.schema_b (id) SELECT 2 WHERE NOT EXISTS (SELECT 1 FROM rev.schema_b);
INSERT INTO rev.acl_a (id) SELECT 7 WHERE NOT EXISTS (SELECT 1 FROM rev.acl_a);
INSERT INTO rev.acl_b (id) SELECT 8 WHERE NOT EXISTS (SELECT 1 FROM rev.acl_b);

REVOKE ALL ON SCHEMA stream FROM PUBLIC;
REVOKE ALL ON ALL TABLES IN SCHEMA stream FROM PUBLIC;
REVOKE ALL ON SCHEMA rev FROM PUBLIC;
REVOKE ALL ON ALL TABLES IN SCHEMA rev FROM PUBLIC;
GRANT USAGE ON SCHEMA stream, rev TO oo_a131_reader, oo_a131_writer;
GRANT SELECT ON ALL TABLES IN SCHEMA stream, rev TO oo_a131_reader;
GRANT INSERT ON stream.parent, stream.child, rev.parent, rev.child TO oo_a131_writer;
