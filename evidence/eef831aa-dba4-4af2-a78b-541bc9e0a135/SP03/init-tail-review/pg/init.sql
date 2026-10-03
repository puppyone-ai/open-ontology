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

REVOKE ALL ON SCHEMA stream FROM PUBLIC;
REVOKE ALL ON ALL TABLES IN SCHEMA stream FROM PUBLIC;
GRANT USAGE ON SCHEMA stream TO oo_a131_reader, oo_a131_writer;
GRANT SELECT ON ALL TABLES IN SCHEMA stream TO oo_a131_reader;
GRANT INSERT ON stream.parent, stream.child TO oo_a131_writer;
