-- Run only as oo_dl01w1_09_reader.
-- Each batch must fail. sqlcmd -b stops the client on the first error; the runner
-- executes these batches separately and records the error number.
-- A zero error is a least-privilege failure. CDC and Change Tracking are not invoked.
USE oo_dl01w1_09;
GO
INSERT INTO dbo.oracle_row (id, bin_key) VALUES (9, 0x01020304);
GO
UPDATE dbo.oracle_row SET note = N'x' WHERE id = 1;
GO
DELETE FROM dbo.oracle_row WHERE id = 1;
GO
CREATE TABLE dbo.reader_must_not_create (id int NOT NULL);
GO
