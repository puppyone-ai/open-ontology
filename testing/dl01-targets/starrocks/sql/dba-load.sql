-- DBA fixture load. No credentials. Database oo_dl01_sr already exists.
USE oo_dl01_sr;

CREATE TABLE dup_events (
  event_id BIGINT NOT NULL,
  bucket DATE NOT NULL,
  amount DECIMAL(18, 4) NULL,
  note VARCHAR(64) NULL
)
DUPLICATE KEY(event_id, bucket)
PARTITION BY RANGE(bucket) (
  PARTITION p202601 VALUES LESS THAN ("2026-02-01"),
  PARTITION p202602 VALUES LESS THAN ("2026-03-01")
)
DISTRIBUTED BY HASH(event_id) BUCKETS 1
PROPERTIES ("replication_num" = "1");

CREATE TABLE pk_accounts (
  account_id BIGINT NOT NULL,
  balance DECIMAL(38, 9) NULL,
  nickname VARCHAR(32) NULL
)
PRIMARY KEY(account_id)
DISTRIBUTED BY HASH(account_id) BUCKETS 1
PROPERTIES ("replication_num" = "1");

CREATE TABLE agg_metrics (
  metric_day DATE NOT NULL,
  metric_name VARCHAR(32) NOT NULL,
  qty BIGINT SUM,
  revenue DECIMAL(18, 4) SUM
)
AGGREGATE KEY(metric_day, metric_name)
DISTRIBUTED BY HASH(metric_day) BUCKETS 1
PROPERTIES ("replication_num" = "1");

CREATE TABLE uniq_dim (
  dim_id BIGINT NOT NULL,
  label VARCHAR(32) NULL,
  score DECIMAL(10, 2) NULL
)
UNIQUE KEY(dim_id)
DISTRIBUTED BY HASH(dim_id) BUCKETS 1
PROPERTIES ("replication_num" = "1");

INSERT INTO dup_events VALUES
  (1, '2026-01-15', CAST('10.2500' AS DECIMAL(18, 4)), 'alpha'),
  (1, '2026-01-15', CAST('10.2500' AS DECIMAL(18, 4)), 'alpha'),
  (2, '2026-01-20', NULL, ''),
  (5, '2026-01-31', CAST('0.0000' AS DECIMAL(18, 4)), '零'),
  (3, '2026-02-02', CAST('-0.0001' AS DECIMAL(18, 4)), NULL),
  (4, '2026-02-28', CAST('99999999999999.9999' AS DECIMAL(18, 4)), '边界');

INSERT INTO pk_accounts VALUES
  (100, CAST('0.100000000' AS DECIMAL(38, 9)), 'first');
INSERT INTO pk_accounts VALUES
  (100, CAST('12345678901234567890123456789.123456789' AS DECIMAL(38, 9)), 'replaced');
INSERT INTO pk_accounts VALUES
  (101, NULL, NULL);
INSERT INTO pk_accounts VALUES
  (102, CAST('0.000000000' AS DECIMAL(38, 9)), '');

INSERT INTO agg_metrics VALUES
  ('2026-01-01', 'clicks', 2, CAST('1.5000' AS DECIMAL(18, 4)));
INSERT INTO agg_metrics VALUES
  ('2026-01-01', 'clicks', 3, CAST('2.2500' AS DECIMAL(18, 4)));
INSERT INTO agg_metrics VALUES
  ('2026-01-02', 'views', 1, CAST('0.0000' AS DECIMAL(18, 4)));

INSERT INTO uniq_dim VALUES
  (7, 'first', CAST('1.10' AS DECIMAL(10, 2)));
INSERT INTO uniq_dim VALUES
  (7, 'second', CAST('2.20' AS DECIMAL(10, 2)));
INSERT INTO uniq_dim VALUES
  (8, 'keep', NULL);
