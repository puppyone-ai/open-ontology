-- DBA mutation after the initial reader snapshot. The reader does not run this file.
USE oo_w45_sr;

INSERT INTO dup_events VALUES
  (26, '2026-06-11', '2026-06-11 08:00:00', CAST('2.2500' AS DECIMAL(18, 4)), 'late');

UPDATE pk_accounts SET nickname = 'edited' WHERE account_id = 300;

DELETE FROM pk_accounts WHERE account_id = 302;
