-- Synthetic snapshot fixture. Roles are created by init.sh. No passwords in this file.
CREATE SCHEMA snap;
REVOKE CREATE ON SCHEMA public FROM PUBLIC;
REVOKE ALL ON SCHEMA snap FROM PUBLIC;
REVOKE CREATE ON DATABASE snapdb FROM PUBLIC;

CREATE TABLE snap.parent (
    id integer PRIMARY KEY,
    label text NOT NULL
);
CREATE TABLE snap.child (
    id integer PRIMARY KEY,
    parent_id integer NOT NULL REFERENCES snap.parent (id),
    qty integer NOT NULL
);
CREATE TABLE snap.touch (
    id integer PRIMARY KEY,
    n integer NOT NULL
);

CREATE FUNCTION snap.bump() RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    UPDATE snap.touch SET n = n + 1 WHERE id = 1;
    RETURN NULL;
END;
$$;

CREATE TRIGGER parent_bump
AFTER INSERT OR UPDATE OR DELETE ON snap.parent
FOR EACH ROW EXECUTE FUNCTION snap.bump();

CREATE TRIGGER child_bump
AFTER INSERT OR UPDATE OR DELETE ON snap.child
FOR EACH ROW EXECUTE FUNCTION snap.bump();

INSERT INTO snap.touch (id, n) VALUES (1, 0);
INSERT INTO snap.parent (id, label) VALUES (1, 'alpha');
INSERT INTO snap.child (id, parent_id, qty) VALUES (10, 1, 2), (11, 1, 3);

GRANT USAGE ON SCHEMA snap TO oo_snap_reader, oo_snap_writer, oo_snap_guc;
GRANT SELECT ON snap.parent, snap.child TO oo_snap_reader;
GRANT SELECT, INSERT, UPDATE, DELETE, TRUNCATE ON snap.parent, snap.child TO oo_snap_writer;
GRANT SELECT, UPDATE ON snap.touch TO oo_snap_writer;
ALTER ROLE oo_snap_guc SET role = 'oo_snap_guc';
