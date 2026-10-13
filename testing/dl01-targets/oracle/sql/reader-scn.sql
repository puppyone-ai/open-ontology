-- Reader flashback query at the SCN captured before the DBA update.
SET DEFINE ON
SET FEEDBACK OFF
SET HEADING OFF
SET PAGESIZE 0
SET LINESIZE 200
SET VERIFY OFF
WHENEVER SQLERROR EXIT SQL.SQLCODE

SELECT 'asof|' || TO_CHAR(d, 'YYYY-MM-DD HH24:MI:SS')
FROM oo_fix.t_date AS OF SCN &1
WHERE id = 2;

SELECT 'current|' || TO_CHAR(d, 'YYYY-MM-DD HH24:MI:SS')
FROM oo_fix.t_date
WHERE id = 2;

PROMPT reader_scn|done
EXIT
