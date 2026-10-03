-- Synthetic non-secret fixture. Roles are created before statement logging.
-- The canary trigger is created after the seed so the seed does not bump it.

CREATE SCHEMA lot;
CREATE SCHEMA acl;
CREATE SCHEMA drift;
CREATE SCHEMA life;

CREATE TABLE lot.batch (
    id int4 PRIMARY KEY,
    title text NOT NULL,
    ready bool NOT NULL,
    code varchar(8) NOT NULL,
    "odd""mark" text
);

CREATE TABLE lot."child row" (
    id int4 PRIMARY KEY,
    batch_id int4 NOT NULL REFERENCES lot.batch (id),
    qty int2 NOT NULL,
    note text
);

CREATE TABLE lot.tick (
    n int4 PRIMARY KEY,
    hits int8 NOT NULL
);

CREATE TABLE lot.gate (
    id int4 PRIMARY KEY,
    ready bool NOT NULL DEFAULT false
);

CREATE TABLE acl.sheet (
    id int4 PRIMARY KEY,
    tag text NOT NULL,
    note text
);

CREATE TABLE acl.wide (
    id int4 PRIMARY KEY,
    tag text NOT NULL,
    note text
);

CREATE TABLE drift.item (
    id int4 PRIMARY KEY,
    label text NOT NULL
);

CREATE TABLE life.hold (
    id int4 PRIMARY KEY,
    label text NOT NULL
);

INSERT INTO lot.tick (n, hits) VALUES (1, 0);
INSERT INTO lot.gate (id) VALUES (1);
INSERT INTO lot.batch (id, title, ready, code, "odd""mark")
VALUES (11, 'maple', true, 'K121', NULL);
INSERT INTO lot."child row" (id, batch_id, qty, note)
VALUES (31, 11, 6, 'twig'), (32, 11, 8, NULL);
INSERT INTO acl.sheet (id, tag, note) VALUES (4, 'linen', 'fold');
INSERT INTO acl.wide (id, tag, note) VALUES (8, 'wool', 'bolt');
INSERT INTO drift.item (id, label) VALUES (5, 'pebble');
INSERT INTO life.hold (id, label) VALUES (1, 'still');

CREATE FUNCTION lot.bump() RETURNS trigger
LANGUAGE plpgsql
SECURITY INVOKER
AS $$
BEGIN
    UPDATE lot.tick SET hits = hits + 1 WHERE n = 1;
    RETURN NULL;
END;
$$;

CREATE TRIGGER batch_bump
AFTER INSERT OR UPDATE OR DELETE ON lot.batch
FOR EACH ROW EXECUTE FUNCTION lot.bump();

CREATE TRIGGER child_bump
AFTER INSERT OR UPDATE OR DELETE ON lot."child row"
FOR EACH ROW EXECUTE FUNCTION lot.bump();

REVOKE ALL ON SCHEMA public FROM PUBLIC;
REVOKE ALL ON SCHEMA lot, acl, drift, life FROM PUBLIC;
GRANT USAGE ON SCHEMA lot, acl, drift, life TO lot_reader;
GRANT USAGE ON SCHEMA lot TO lot_writer;

REVOKE ALL ON ALL TABLES IN SCHEMA lot, acl, drift, life FROM PUBLIC;
REVOKE ALL ON FUNCTION lot.bump() FROM PUBLIC;

GRANT SELECT ON lot.batch, lot."child row", drift.item, life.hold TO lot_reader;
GRANT SELECT (id, tag, note) ON TABLE acl.sheet TO lot_reader;
GRANT SELECT ON TABLE acl.wide TO lot_reader;

GRANT SELECT, INSERT, UPDATE, DELETE ON lot.batch, lot."child row" TO lot_writer;
GRANT UPDATE ON lot.tick TO lot_writer;
GRANT SELECT, UPDATE ON lot.gate TO lot_writer;
GRANT EXECUTE ON FUNCTION lot.bump() TO lot_writer;
