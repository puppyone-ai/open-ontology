-- In-flight reader statement. The DBA cancels it. This script does not write.
SET DEFINE OFF
SET FEEDBACK OFF
SET HEADING OFF
SET PAGESIZE 0
WHENEVER SQLERROR EXIT SQL.SQLCODE

SELECT /* oo-dl01w1-10-cancel */ COUNT(*)
FROM all_objects a, all_objects b;

PROMPT reader_cancel|finished
EXIT
