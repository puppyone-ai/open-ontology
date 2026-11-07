SET NOCOUNT ON;
CREATE TABLE string_keys (id NVARCHAR(100) COLLATE Latin1_General_100_CI_AI_SC PRIMARY KEY, value_no BIGINT);
CREATE TABLE type_edges (
 id BIGINT PRIMARY KEY, exact_value DECIMAL(38,10), unsigned_value DECIMAL(20,0) CHECK(unsigned_value>=0),
 local_time DATETIME2(6), occurred_at_utc DATETIME2(6), original_zone NVARCHAR(40),
 text_lob NVARCHAR(MAX), binary_lob VARBINARY(MAX), document NVARCHAR(MAX) CHECK(ISJSON(document)=1),
 enum_value NVARCHAR(10) CHECK(enum_value IN ('new','closed')), set_value NVARCHAR(10), nullable_text NVARCHAR(100)
);
INSERT INTO type_edges VALUES (1,12345678901234567890.1234567890,18446744073709551615,'2026-01-01T08:00:00.123456','2026-01-01T00:00:00.123456','+08:00','synthetic lob',0x0001FF,'{"n":1,"empty":""}','new','a,b',NULL);
INSERT INTO type_edges (id,nullable_text) VALUES (2,'');
CREATE PARTITION FUNCTION fixture_months (INT) AS RANGE RIGHT FOR VALUES (2);
CREATE PARTITION SCHEME fixture_months AS PARTITION fixture_months ALL TO ([PRIMARY]);
CREATE TABLE partition_events (id BIGINT NOT NULL, month_no INT NOT NULL, PRIMARY KEY(id,month_no)) ON fixture_months(month_no);
INSERT INTO partition_events VALUES (1,1),(2,2);
