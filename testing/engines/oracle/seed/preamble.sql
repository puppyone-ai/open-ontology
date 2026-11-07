ALTER SESSION SET TIME_ZONE='+00:00';
ALTER SESSION SET NLS_NUMERIC_CHARACTERS='.,';
CREATE TABLE string_keys (id VARCHAR2(100 CHAR) PRIMARY KEY, value_no NUMBER(19));
-- 11g XE lacks data-bound collations. Preserve the VARCHAR PK and enforce
-- case/accent equivalence separately; this is NOT a collated primary key.
CREATE UNIQUE INDEX string_keys_ci ON string_keys (NLSSORT(id,'NLS_SORT=BINARY_AI'));
CREATE TABLE type_edges (
 id NUMBER(19) PRIMARY KEY, exact_value DECIMAL(38,10), unsigned_value DECIMAL(20,0) CHECK(unsigned_value>=0),
 local_time TIMESTAMP(6), occurred_at_utc TIMESTAMP(6) WITH TIME ZONE, original_zone VARCHAR2(40),
 text_lob CLOB, binary_lob BLOB, document CLOB,
 enum_value VARCHAR2(10) CHECK(enum_value IN ('new','closed')), set_value VARCHAR2(10), nullable_text VARCHAR2(100)
);
INSERT INTO type_edges VALUES (1,12345678901234567890.1234567890,18446744073709551615,TIMESTAMP '2026-01-01 08:00:00.123456',TO_TIMESTAMP_TZ('2026-01-01 08:00:00.123456 +08:00','YYYY-MM-DD HH24:MI:SS.FF TZH:TZM'),'+08:00','synthetic lob',HEXTORAW('0001FF'),'{"n":1,"empty":""}','new','a,b',NULL);
INSERT INTO type_edges (id,nullable_text) VALUES (2,'');
-- XE 11g has no partitioning option; this portable table is not partition proof.
CREATE TABLE partition_events (id NUMBER(19), month_no INTEGER, PRIMARY KEY(id,month_no));
INSERT INTO partition_events VALUES (1,1);
INSERT INTO partition_events VALUES (2,2);
