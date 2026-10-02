-- Synthetic review fixture. Roles and passwords are applied by init.sh. No secrets here.
CREATE SCHEMA rev;
REVOKE CREATE ON SCHEMA public FROM PUBLIC;
REVOKE ALL ON DATABASE revdb FROM PUBLIC;
GRANT CONNECT ON DATABASE revdb TO oo_rev_reader, oo_rev_writer, oo_rev_guc;

CREATE TABLE rev.item (
    id integer PRIMARY KEY,
    label text NOT NULL,
    flag boolean NOT NULL,
    code character varying(8) NOT NULL,
    "odd""note" text
);
CREATE TABLE rev."line item" (
    id integer PRIMARY KEY,
    item_id integer NOT NULL REFERENCES rev.item (id),
    qty smallint NOT NULL,
    memo text
);
CREATE TABLE rev.canary (
    id integer PRIMARY KEY,
    n integer NOT NULL
);
CREATE TABLE rev.twin (
    id integer PRIMARY KEY,
    label text NOT NULL
);
CREATE TABLE rev.twin_more () INHERITS (rev.twin);

INSERT INTO rev.canary (id, n) VALUES (1, 0);
INSERT INTO rev.item (id, label, flag, code, "odd""note")
VALUES (7, 'cedar', true, 'A116', NULL);
INSERT INTO rev."line item" (id, item_id, qty, memo)
VALUES (21, 7, 4, 'sprig'), (22, 7, 5, NULL);
INSERT INTO rev.twin (id, label) VALUES (3, 'one');
INSERT INTO rev.twin_more (id, label) VALUES (3, 'two');

CREATE FUNCTION rev.bump() RETURNS trigger
LANGUAGE plpgsql
SECURITY INVOKER
AS $$
BEGIN
    UPDATE rev.canary SET n = n + 1 WHERE id = 1;
    RETURN NULL;
END;
$$;

CREATE TRIGGER item_bump
AFTER INSERT OR UPDATE OR DELETE ON rev.item
FOR EACH ROW EXECUTE FUNCTION rev.bump();

CREATE TRIGGER line_bump
AFTER INSERT OR UPDATE OR DELETE ON rev."line item"
FOR EACH ROW EXECUTE FUNCTION rev.bump();

GRANT USAGE ON SCHEMA rev TO oo_rev_reader, oo_rev_writer, oo_rev_guc;
GRANT SELECT ON rev.item, rev."line item", rev.twin, rev.twin_more TO oo_rev_reader;
GRANT SELECT, INSERT, UPDATE, DELETE, TRUNCATE ON rev.item, rev."line item" TO oo_rev_writer;
GRANT SELECT, UPDATE ON rev.canary TO oo_rev_writer;
ALTER ROLE oo_rev_guc SET role = 'oo_rev_guc';
