-- March is outside both RANGE partitions. This must fail and write no row.
USE oo_dl01w1_12;
INSERT INTO part_day VALUES ('2026-03-01', 'east', CAST('9.00' AS DECIMAL(12,2)));
