CREATE SCHEMA oo_a127;
CREATE TABLE oo_a127.cut (
    id int4 PRIMARY KEY,
    note text
);
INSERT INTO oo_a127.cut (id, note) VALUES
    (1, 'one'),
    (2, NULL),
    (3, 'three-original');
GRANT USAGE ON SCHEMA oo_a127 TO oo_a127_reader, oo_a127_writer;
GRANT SELECT ON oo_a127.cut TO oo_a127_reader;
GRANT SELECT, INSERT, UPDATE ON oo_a127.cut TO oo_a127_writer;
