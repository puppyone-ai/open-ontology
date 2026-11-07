-- DBA-only synthetic setup. Product sessions never execute this file.
CREATE SCHEMA fixture;
CREATE TABLE fixture.items (id bigint PRIMARY KEY, payload text);
INSERT INTO fixture.items VALUES (1,'one'),(2,'two'),(3,NULL);
CREATE TABLE fixture.root (id bigint PRIMARY KEY, payload text) PARTITION BY RANGE(id);
CREATE TABLE fixture.leaf PARTITION OF fixture.root FOR VALUES FROM (0) TO (100);
INSERT INTO fixture.root VALUES (1,'partition');
GRANT USAGE ON SCHEMA fixture TO oo_reader;
GRANT SELECT ON ALL TABLES IN SCHEMA fixture TO oo_reader;
GRANT pg_read_all_stats TO oo_reader;
CREATE PUBLICATION oo_changes FOR TABLE fixture.items, fixture.root
    WITH (publish_via_partition_root=true);
ALTER SYSTEM SET max_slot_wal_keep_size='256MB';
SELECT pg_reload_conf();
SELECT pg_create_logical_replication_slot('oo_changes','pgoutput');
ANALYZE;
