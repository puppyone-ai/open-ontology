-- DBA deletes one primary-key row. The reader does not run this statement.
USE oo_dl01_sr;
DELETE FROM pk_accounts WHERE account_id = 102;
