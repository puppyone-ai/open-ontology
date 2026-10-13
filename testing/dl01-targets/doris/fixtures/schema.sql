-- DBA fixture DDL for a synthetic Apache Doris source. Not a product migration.
-- Key columns are a prefix, as Doris requires. replication_num=1 matches one BE.
CREATE DATABASE IF NOT EXISTS oo_dl01w1_12;
USE oo_dl01w1_12;

CREATE TABLE dup_events (
  event_id BIGINT NOT NULL,
  kind VARCHAR(16) NOT NULL,
  qty DECIMAL(10,2) NULL
)
DUPLICATE KEY(event_id, kind)
DISTRIBUTED BY HASH(event_id) BUCKETS 1
PROPERTIES ("replication_num" = "1");

CREATE TABLE agg_qty (
  sku VARCHAR(16) NOT NULL,
  qty DECIMAL(10,2) SUM NULL,
  note VARCHAR(32) REPLACE NULL
)
AGGREGATE KEY(sku)
DISTRIBUTED BY HASH(sku) BUCKETS 1
PROPERTIES ("replication_num" = "1");

CREATE TABLE uniq_acct (
  id BIGINT NOT NULL,
  balance DECIMAL(18,4) NULL,
  label VARCHAR(32) NULL
)
UNIQUE KEY(id)
DISTRIBUTED BY HASH(id) BUCKETS 1
PROPERTIES ("replication_num" = "1");

CREATE TABLE dec_exact (
  id INT NOT NULL,
  d18 DECIMAL(18,4) NULL,
  d38 DECIMAL(38,4) NULL
)
DUPLICATE KEY(id)
DISTRIBUTED BY HASH(id) BUCKETS 1
PROPERTIES ("replication_num" = "1");

CREATE TABLE part_day (
  day DATE NOT NULL,
  region VARCHAR(8) NOT NULL,
  amount DECIMAL(12,2) NULL
)
DUPLICATE KEY(day, region)
PARTITION BY RANGE(day) (
  PARTITION p202601 VALUES [('2026-01-01'), ('2026-02-01')),
  PARTITION p202602 VALUES [('2026-02-01'), ('2026-03-01'))
)
DISTRIBUTED BY HASH(region) BUCKETS 1
PROPERTIES ("replication_num" = "1");

CREATE TABLE str_key (
  k VARCHAR(16) NOT NULL,
  note VARCHAR(8) NULL
)
DUPLICATE KEY(k)
DISTRIBUTED BY HASH(k) BUCKETS 1
PROPERTIES ("replication_num" = "1");
