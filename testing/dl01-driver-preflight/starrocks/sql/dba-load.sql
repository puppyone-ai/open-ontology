-- DBA fixture load for the W45 reader gate. No credentials.
-- Row literals are this gate's fixture. They are not the W13 setup gold.
USE oo_w45_sr;

CREATE TABLE dup_events (
  event_id BIGINT NOT NULL,
  bucket DATE NOT NULL,
  seen DATETIME NULL,
  amount DECIMAL(18, 4) NULL,
  note VARCHAR(64) NULL
)
DUPLICATE KEY(event_id, bucket)
PARTITION BY RANGE(bucket) (
  PARTITION p202605 VALUES LESS THAN ("2026-06-01"),
  PARTITION p202606 VALUES LESS THAN ("2026-07-01")
)
DISTRIBUTED BY HASH(event_id) BUCKETS 1
PROPERTIES ("replication_num" = "1");

CREATE TABLE pk_accounts (
  account_id BIGINT NOT NULL,
  balance DECIMAL(38, 9) NULL,
  nickname VARCHAR(32) NULL,
  opened DATETIME NULL
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

CREATE TABLE sketch_hold (
  id BIGINT NOT NULL,
  bits BITMAP BITMAP_UNION,
  cards HLL HLL_UNION
)
AGGREGATE KEY(id)
DISTRIBUTED BY HASH(id) BUCKETS 1
PROPERTIES ("replication_num" = "1");

INSERT INTO dup_events VALUES
  (21, '2026-05-06', '2026-05-06 01:02:03', CAST('4.5000' AS DECIMAL(18, 4)), 'west'),
  (21, '2026-05-06', '2026-05-06 01:02:03', CAST('4.5000' AS DECIMAL(18, 4)), 'west'),
  (22, '2026-05-19', NULL, NULL, ''),
  (25, '2026-05-31', '2026-05-31 00:00:00', CAST('0.0000' AS DECIMAL(18, 4)), '空'),
  (23, '2026-06-03', '2026-06-03 23:59:59', CAST('-0.0003' AS DECIMAL(18, 4)), NULL),
  (24, '2026-06-21', '2026-06-21 12:30:45', CAST('77777777777777.7777' AS DECIMAL(18, 4)), '边');

INSERT INTO pk_accounts VALUES
  (300, CAST('1.250000000' AS DECIMAL(38, 9)), 'old', '2026-05-01 00:00:00');
INSERT INTO pk_accounts VALUES
  (300, CAST('22345678901234567890123456789.123456789' AS DECIMAL(38, 9)), 'new', '2026-05-02 03:04:05');
INSERT INTO pk_accounts VALUES
  (301, NULL, NULL, NULL);
INSERT INTO pk_accounts VALUES
  (302, CAST('0.000000000' AS DECIMAL(38, 9)), '', '2026-05-03 00:00:00');

INSERT INTO agg_metrics VALUES
  ('2026-05-01', 'taps', 4, CAST('1.2500' AS DECIMAL(18, 4)));
INSERT INTO agg_metrics VALUES
  ('2026-05-01', 'taps', 6, CAST('0.7500' AS DECIMAL(18, 4)));
INSERT INTO agg_metrics VALUES
  ('2026-05-02', 'hits', 8, CAST('0.0000' AS DECIMAL(18, 4)));

INSERT INTO uniq_dim VALUES
  (17, 'first', CAST('3.30' AS DECIMAL(10, 2)));
INSERT INTO uniq_dim VALUES
  (17, 'second', CAST('4.40' AS DECIMAL(10, 2)));
INSERT INTO uniq_dim VALUES
  (18, 'keep', NULL);

INSERT INTO sketch_hold SELECT 1, bitmap_hash('w45'), hll_hash('w45');
