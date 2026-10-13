-- Run as oo_dl01w1_09_dba after the reader journey. This reads the XE file.
-- It does not change user tables. The runner classifies rows and stores hashes of
-- batch text, not the raw text, so a later edit cannot drag a password into evidence.
SELECT
    CAST(event_data AS xml).value('(event/@name)[1]', 'nvarchar(128)') AS event_name,
    CAST(event_data AS xml).value('(event/action[@name="username"]/value)[1]', 'nvarchar(128)') AS username,
    CAST(event_data AS xml).value('(event/data[@name="error_number"]/value)[1]', 'int') AS error_number
FROM sys.fn_xe_file_target_read_file(
    N'/var/opt/mssql/oo-dl01w1-09-xe/oo_dl01w1_09_reader*.xel',
    NULL,
    NULL,
    NULL
);
GO
