-- DBA appends one row into partition p202602 only.
USE oo_dl01_sr;
INSERT INTO dup_events VALUES
  (6, '2026-02-10', CAST('1.0000' AS DECIMAL(18, 4)), 'late');
