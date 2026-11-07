-- Fixture-admin connection only; never executed by the source engine.
CREATE SCHEMA fixture;
CREATE TABLE fixture.integer_pk (id bigint PRIMARY KEY, payload text);
INSERT INTO fixture.integer_pk SELECT i, CASE WHEN i%17=0 THEN NULL ELSE 'row-'||i END FROM generate_series(1,2000000) i;
CREATE TABLE fixture.text_pk (code text PRIMARY KEY, payload text);
INSERT INTO fixture.text_pk SELECT 'code-'||i, 'text-'||i FROM generate_series(1,1500) i;
INSERT INTO fixture.text_pk VALUES ('','empty'), ('Z','upper'), ('a','lower'), ('unicode-中文','value-😀');
CREATE TABLE fixture.composite (region bigint, code text, payload text, PRIMARY KEY(region,code));
INSERT INTO fixture.composite SELECT i%11, 'code-'||i, 'composite-'||i FROM generate_series(1,2000) i;
CREATE TABLE fixture.unique_only (id bigint NOT NULL UNIQUE, payload text);
INSERT INTO fixture.unique_only SELECT i, 'unique-'||i FROM generate_series(1,1500) i;
CREATE TABLE fixture.no_key (id bigint, payload text);
INSERT INTO fixture.no_key SELECT i, 'no-key-'||i FROM generate_series(1,1500) i;
INSERT INTO fixture.no_key VALUES (NULL,NULL),(NULL,''),(1,'no-key-1');
CREATE TABLE fixture.growing (id bigint PRIMARY KEY, payload text);
INSERT INTO fixture.growing SELECT i, 'growing-'||i FROM generate_series(1,500) i;
CREATE TABLE fixture.empty_table (id bigint PRIMARY KEY, payload text);
CREATE TABLE fixture.denied (id bigint);
CREATE TABLE fixture.special (id bigint PRIMARY KEY, amount numeric(30,8), data bytea, flag boolean, stamp timestamp, doc json);
INSERT INTO fixture.special VALUES
 (1,1234567890123456789012.12345678,decode('0001ff','hex'),true,'2026-01-02 03:04:05.123456','{"x": 1}'),
 (2,NULL,NULL,false,NULL,NULL);
GRANT USAGE ON SCHEMA fixture TO oo_reader;
GRANT SELECT ON ALL TABLES IN SCHEMA fixture TO oo_reader;
REVOKE SELECT ON fixture.denied FROM oo_reader;
ANALYZE;
