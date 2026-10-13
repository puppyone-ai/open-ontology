-- Fixture DBA only. Pause scheduling and wait for no running source occurrence first.
-- Original six-row baseline and all historical expected/oracle files remain unchanged.
USE oo_dl01w1_09;
GO
BEGIN TRANSACTION;
UPDATE dbo.oracle_row SET amt=CAST('-9007199254740993.0000000001' AS decimal(38,10)),
    dt7='2024-03-15T12:34:56.7654321', note=N'更新' WHERE bin_key=0x00000000;
DELETE FROM dbo.oracle_row WHERE bin_key=0x00000100;
INSERT INTO dbo.oracle_row (id,bin_key,dt7,amt,note,name_ci,name_bin)
VALUES (7,0x00000101,'2024-03-15T12:34:56.1111111',CAST('1.0000000001' AS decimal(38,10)),N'新增',N'B',N'B');
COMMIT TRANSACTION;
GO
-- Stop writes here. Resume approved reader C2 and require full independent typed diff0.
