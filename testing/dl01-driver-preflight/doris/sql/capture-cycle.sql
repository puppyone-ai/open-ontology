USE oo_dl01w1_43;
INSERT INTO part_day VALUES ('2026-03-20', 'east', CAST('1.50' AS DECIMAL(12,2)));
UPDATE uniq_acct SET balance = CAST('3.5000' AS DECIMAL(18,4)), label = 'cycle' WHERE id = 2;
INSERT INTO uniq_acct VALUES (5, CAST('4.0000' AS DECIMAL(18,4)), 'new');
DELETE FROM nz_obs WHERE id = 3;
