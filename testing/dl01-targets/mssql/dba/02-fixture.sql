-- Run as oo_dl01w1_09_dba. Creates the synthetic database and the least-privilege reader.
-- The runner substitutes __READER_PASSWORD__ in a private copy.
CREATE DATABASE oo_dl01w1_09 COLLATE Latin1_General_CI_AS;
GO
USE oo_dl01w1_09;
GO
CREATE LOGIN oo_dl01w1_09_reader
    WITH PASSWORD = N'__READER_PASSWORD__',
         CHECK_POLICY = ON,
         CHECK_EXPIRATION = OFF;
CREATE USER oo_dl01w1_09_reader FOR LOGIN oo_dl01w1_09_reader;
GRANT SELECT ON SCHEMA::dbo TO oo_dl01w1_09_reader;
GRANT VIEW DEFINITION TO oo_dl01w1_09_reader;
GRANT VIEW DATABASE STATE TO oo_dl01w1_09_reader;
GO
CREATE TABLE dbo.oracle_row (
    id int NOT NULL,
    bin_key binary(4) NOT NULL,
    dt7 datetime2(7) NULL,
    amt decimal(38, 10) NULL,
    note nvarchar(20) NULL,
    name_ci nvarchar(20) COLLATE Latin1_General_CI_AS NULL,
    name_bin nvarchar(20) COLLATE Latin1_General_BIN2 NULL,
    CONSTRAINT pk_oracle_row PRIMARY KEY (bin_key)
);
GO
CREATE UNIQUE INDEX ux_oracle_row_id ON dbo.oracle_row (id);
GO
