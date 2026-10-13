-- Run as oo_dl01w1_09_dba. Synthetic rows only. The reader does not run this file.
USE oo_dl01w1_09;
GO
INSERT INTO dbo.oracle_row (id, bin_key, dt7, amt, note, name_ci, name_bin) VALUES
    (1, 0x00000000, '2024-03-15T12:34:56.1234567', CAST('9007199254740993.0000000001' AS decimal(38, 10)), NULL, N'A', N'A'),
    (2, 0x00000001, '2024-03-15T12:34:56.1234567', CAST('9007199254740993.0000000001' AS decimal(38, 10)), N'', N'a', N'a'),
    (3, 0x000000FF, '2024-03-15T12:34:56.1234567', CAST('0' AS decimal(38, 10)), N' ', N'A ', N'Z'),
    (4, 0x00000100, NULL, NULL, N'中', N'中', N'中'),
    (5, 0x00FF0000, NULL, NULL, NULL, N'文', N'文'),
    (6, 0xFF000000, NULL, NULL, NULL, NULL, NULL);
GO
