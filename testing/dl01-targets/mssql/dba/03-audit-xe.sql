-- Run as oo_dl01w1_09_dba after the runner creates the directories and chowns them to mssql.
-- Paths: /var/opt/mssql/oo-dl01w1-09-audit and /var/opt/mssql/oo-dl01w1-09-xe
-- ON_FAILURE = CONTINUE so a full audit queue does not stop the instance.
-- This is fixture observation, not a production audit guarantee.
USE master;
GO
CREATE SERVER AUDIT oo_dl01w1_09_audit
    TO FILE (
        FILEPATH = N'/var/opt/mssql/oo-dl01w1-09-audit/',
        MAXSIZE = 64 MB,
        MAX_ROLLOVER_FILES = 4
    )
    WITH (QUEUE_DELAY = 1000, ON_FAILURE = CONTINUE);
GO
CREATE SERVER AUDIT SPECIFICATION oo_dl01w1_09_server_spec
    FOR SERVER AUDIT oo_dl01w1_09_audit
    ADD (FAILED_LOGIN_GROUP),
    ADD (SUCCESSFUL_LOGIN_GROUP);
GO
ALTER SERVER AUDIT oo_dl01w1_09_audit WITH (STATE = ON);
ALTER SERVER AUDIT SPECIFICATION oo_dl01w1_09_server_spec WITH (STATE = ON);
GO
USE oo_dl01w1_09;
GO
CREATE DATABASE AUDIT SPECIFICATION oo_dl01w1_09_db_spec
    FOR SERVER AUDIT oo_dl01w1_09_audit
    ADD (SELECT, INSERT, UPDATE, DELETE ON SCHEMA::dbo BY oo_dl01w1_09_reader),
    ADD (SCHEMA_OBJECT_CHANGE_GROUP);
GO
ALTER DATABASE AUDIT SPECIFICATION oo_dl01w1_09_db_spec WITH (STATE = ON);
GO
USE master;
GO
CREATE EVENT SESSION oo_dl01w1_09_reader_xe ON SERVER
ADD EVENT sqlserver.sql_batch_completed (
    ACTION (sqlserver.username, sqlserver.session_id, sqlserver.client_app_name)
    WHERE sqlserver.username = N'oo_dl01w1_09_reader'
),
ADD EVENT sqlserver.error_reported (
    ACTION (sqlserver.username, sqlserver.session_id, sqlserver.client_app_name)
    WHERE sqlserver.username = N'oo_dl01w1_09_reader'
),
ADD EVENT sqlserver.attention (
    ACTION (sqlserver.username, sqlserver.session_id, sqlserver.client_app_name)
    WHERE sqlserver.username = N'oo_dl01w1_09_reader'
)
ADD TARGET package0.event_file (
    SET filename = N'/var/opt/mssql/oo-dl01w1-09-xe/oo_dl01w1_09_reader.xel',
        max_file_size = 64
);
GO
ALTER EVENT SESSION oo_dl01w1_09_reader_xe ON SERVER STATE = START;
GO
