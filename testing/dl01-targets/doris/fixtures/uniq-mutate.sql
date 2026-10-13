-- Unique-model mutation. Duplicate and aggregate models do not offer the same UPDATE/DELETE.
USE oo_dl01w1_12;
UPDATE uniq_acct
   SET balance = CAST('-1.2500' AS DECIMAL(18,4)),
       label = 'adj'
 WHERE id = 2;
DELETE FROM uniq_acct WHERE id = 3;
