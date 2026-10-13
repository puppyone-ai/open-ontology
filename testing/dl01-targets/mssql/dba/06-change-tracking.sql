-- Fixture DBA only, manually run after 02-fixture.sql and before 04-load.sql.
-- Native product reader never runs this file or enables source capabilities.
-- Only approved owned synthetic namespace. No customer database use.
USE master;
GO
ALTER DATABASE oo_dl01w1_09 SET ALLOW_SNAPSHOT_ISOLATION ON;
ALTER DATABASE oo_dl01w1_09 SET CHANGE_TRACKING = ON
    (CHANGE_RETENTION = 7 DAYS, AUTO_CLEANUP = ON);
GO
USE oo_dl01w1_09;
GO
ALTER TABLE dbo.oracle_row ENABLE CHANGE_TRACKING WITH (TRACK_COLUMNS_UPDATED = OFF);
GRANT VIEW CHANGE TRACKING ON OBJECT::dbo.oracle_row TO oo_dl01w1_09_reader;
GO
-- Risk: CT stores keys/versions with finite retention; long SNAPSHOT reads delay cleanup.
-- Rollback by DBA only, after consumers paused: disable table CT, disable database CT.
-- Do not grant source writer roles to the product reader.
SELECT DB_NAME() AS database_name, CHANGE_TRACKING_CURRENT_VERSION() AS current_version,
       CHANGE_TRACKING_MIN_VALID_VERSION(OBJECT_ID(N'dbo.oracle_row')) AS min_valid_version,
       OBJECT_ID(N'dbo.oracle_row') AS object_id;
GO
