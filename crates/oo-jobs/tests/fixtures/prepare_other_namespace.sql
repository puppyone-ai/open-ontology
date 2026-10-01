CREATE SCHEMA other_ns;
CREATE TABLE other_ns.marker (id integer PRIMARY KEY);
INSERT INTO other_ns.marker (id) VALUES (1);
REVOKE ALL ON SCHEMA other_ns FROM PUBLIC;
REVOKE ALL ON TABLE other_ns.marker FROM PUBLIC;
