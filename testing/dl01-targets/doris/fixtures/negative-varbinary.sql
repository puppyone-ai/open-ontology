-- 3.x internal tables have no stored VARBINARY key. This create must fail.
USE oo_dl01w1_12;
CREATE TABLE bin_key (
  k VARBINARY(16) NOT NULL,
  n INT NULL
)
DUPLICATE KEY(k)
DISTRIBUTED BY HASH(k) BUCKETS 1
PROPERTIES ("replication_num" = "1");
