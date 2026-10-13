USE oo_dl01w1_43;
UPDATE uniq_acct
   SET balance = CAST('9.2500' AS DECIMAL(18,4)),
       label = 'upd2'
 WHERE id = 3;
DELETE FROM uniq_acct WHERE id = 4;
