SET NAMES utf8mb4;
SET time_zone='+00:00';
CREATE TABLE string_keys (id VARCHAR(100) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci PRIMARY KEY, value_no BIGINT);
CREATE TABLE type_edges (
 id BIGINT PRIMARY KEY, exact_value DECIMAL(38,10), unsigned_value BIGINT UNSIGNED,
 local_time DATETIME(6), occurred_at_utc TIMESTAMP(6), original_zone VARCHAR(40),
 text_lob LONGTEXT, binary_lob LONGBLOB, document JSON,
 enum_value ENUM('new','closed'), set_value SET('a','b'), nullable_text VARCHAR(100)
);
INSERT INTO type_edges VALUES (1,12345678901234567890.1234567890,18446744073709551615,'2026-01-01 08:00:00.123456','2026-01-01 00:00:00.123456','+08:00','synthetic lob',X'0001FF','{"n":1,"empty":""}','new','a,b',NULL);
INSERT INTO type_edges (id,nullable_text) VALUES (2,'');
CREATE TABLE partition_events (id BIGINT NOT NULL, month_no INTEGER NOT NULL, PRIMARY KEY(id,month_no))
 PARTITION BY RANGE(month_no) (PARTITION p1 VALUES LESS THAN(2), PARTITION p2 VALUES LESS THAN MAXVALUE);
INSERT INTO partition_events VALUES (1,1),(2,2);
