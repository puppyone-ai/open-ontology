SET TIME ZONE 'UTC';
CREATE COLLATION fixture_ci (provider=icu,locale='und-u-ks-level1',deterministic=false);
CREATE TABLE string_keys (id VARCHAR(100) COLLATE fixture_ci PRIMARY KEY, value_no BIGINT);
CREATE TABLE type_edges (
 id BIGINT PRIMARY KEY, exact_value DECIMAL(38,10), unsigned_value DECIMAL(20,0) CHECK(unsigned_value>=0),
 local_time TIMESTAMP(6), occurred_at_utc TIMESTAMPTZ(6), original_zone VARCHAR(40),
 text_lob TEXT, binary_lob BYTEA, document JSONB,
 enum_value VARCHAR(10) CHECK(enum_value IN ('new','closed')), set_value TEXT[], nullable_text VARCHAR(100)
);
INSERT INTO type_edges VALUES (1,12345678901234567890.1234567890,18446744073709551615,TIMESTAMP '2026-01-01 08:00:00.123456',TIMESTAMPTZ '2026-01-01 08:00:00.123456+08:00','+08:00','synthetic lob',decode('0001ff','hex'),'{"n":1,"empty":""}','new',ARRAY['a','b'],NULL);
INSERT INTO type_edges (id,nullable_text) VALUES (2,'');
CREATE TABLE partition_events (id BIGINT NOT NULL, month_no INTEGER NOT NULL, PRIMARY KEY(id,month_no)) PARTITION BY RANGE(month_no);
CREATE TABLE partition_events_p1 PARTITION OF partition_events FOR VALUES FROM (1) TO (2);
CREATE TABLE partition_events_p2 PARTITION OF partition_events FOR VALUES FROM (2) TO (13);
INSERT INTO partition_events VALUES (1,1),(2,2);
