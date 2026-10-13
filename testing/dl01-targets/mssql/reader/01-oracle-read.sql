-- Run only as oo_dl01w1_09_reader. Statements are reads of catalog, permissions, and values.
USE oo_dl01w1_09;
GO
SELECT
    CONVERT(nvarchar(128), DATABASEPROPERTYEX(DB_NAME(), 'Collation')) AS database_collation,
    (SELECT collation_name FROM sys.columns WHERE object_id = OBJECT_ID(N'dbo.oracle_row') AND name = N'name_bin') AS name_bin_collation,
    (SELECT scale FROM sys.columns WHERE object_id = OBJECT_ID(N'dbo.oracle_row') AND name = N'dt7') AS dt7_scale,
    (SELECT DATETIME_PRECISION FROM INFORMATION_SCHEMA.COLUMNS WHERE TABLE_SCHEMA = N'dbo' AND TABLE_NAME = N'oracle_row' AND COLUMN_NAME = N'dt7') AS dt7_datetime_precision,
    (SELECT DATEPART(nanosecond, dt7) FROM dbo.oracle_row WHERE id = 1) AS dt7_nanosecond,
    (SELECT SQL_VARIANT_PROPERTY(amt, 'Precision') FROM dbo.oracle_row WHERE id = 1) AS amt_precision,
    (SELECT SQL_VARIANT_PROPERTY(amt, 'Scale') FROM dbo.oracle_row WHERE id = 1) AS amt_scale,
    (SELECT CASE WHEN amt = CAST(CAST(amt AS float) AS decimal(38, 10)) THEN 1 ELSE 0 END FROM dbo.oracle_row WHERE id = 1) AS survives_float_roundtrip,
    (SELECT SUM(CASE WHEN note IS NULL THEN 1 ELSE 0 END) FROM dbo.oracle_row) AS null_notes,
    (SELECT SUM(CASE WHEN note = N'' THEN 1 ELSE 0 END) FROM dbo.oracle_row) AS empty_notes,
    (SELECT SUM(CASE WHEN note = N' ' THEN 1 ELSE 0 END) FROM dbo.oracle_row) AS space_notes,
    CASE WHEN N'A' COLLATE Latin1_General_CI_AS = N'a' COLLATE Latin1_General_CI_AS THEN 1 ELSE 0 END AS ci_case_equal,
    CASE WHEN N'A' COLLATE Latin1_General_CI_AS = N'A ' COLLATE Latin1_General_CI_AS THEN 1 ELSE 0 END AS ci_trail_equal,
    CASE WHEN N'A' COLLATE Latin1_General_BIN2 = N'a' COLLATE Latin1_General_BIN2 THEN 1 ELSE 0 END AS bin_case_equal,
    CASE WHEN N'A' COLLATE Latin1_General_BIN2 = N'A ' COLLATE Latin1_General_BIN2 THEN 1 ELSE 0 END AS bin_trail_equal,
    CASE WHEN CONVERT(varbinary(20), N'A') = CONVERT(varbinary(20), N'A ') THEN 1 ELSE 0 END AS bin_trail_bytes_equal,
    (SELECT STRING_AGG(CONVERT(varchar(11), id), ',') WITHIN GROUP (ORDER BY bin_key) FROM dbo.oracle_row) AS bin_key_order,
    (SELECT STRING_AGG(CONVERT(varchar(11), id), ',') WITHIN GROUP (ORDER BY name_bin, id) FROM dbo.oracle_row) AS name_bin_order,
    IS_SRVROLEMEMBER('sysadmin') AS reader_sysadmin,
    IS_MEMBER('db_owner') AS reader_db_owner,
    IS_MEMBER('db_datawriter') AS reader_datawriter,
    IS_MEMBER('db_ddladmin') AS reader_ddladmin,
    HAS_PERMS_BY_NAME(N'dbo.oracle_row', N'OBJECT', N'INSERT') AS can_insert,
    HAS_PERMS_BY_NAME(N'dbo.oracle_row', N'OBJECT', N'SELECT') AS can_select,
    HAS_PERMS_BY_NAME(DB_NAME(), N'DATABASE', N'CREATE TABLE') AS can_create_table,
    HAS_PERMS_BY_NAME(NULL, NULL, N'ALTER ANY CONNECTION') AS can_alter_any_connection,
    (SELECT is_cdc_enabled FROM sys.databases WHERE name = DB_NAME()) AS is_cdc_enabled,
    CASE WHEN EXISTS (
        SELECT 1 FROM sys.change_tracking_databases c WHERE c.database_id = DB_ID()
    ) THEN 1 ELSE 0 END AS change_tracking_on;
GO
