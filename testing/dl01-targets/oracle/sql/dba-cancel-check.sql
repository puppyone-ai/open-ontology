SET DEFINE OFF
SET FEEDBACK OFF
SET HEADING OFF
SET PAGESIZE 0
SET LINESIZE 300
WHENEVER SQLERROR EXIT SQL.SQLCODE

ALTER SESSION SET CONTAINER = FREEPDB1;
SELECT 'still|' || COUNT(*)
FROM v$sql
WHERE sql_text LIKE '%oo-dl01w1-10-can' || 'cel%'
  AND users_executing > 0;
SELECT 'sess|' || sid || '|' || status || '|' || NVL(sql_id, '-')
FROM v$session
WHERE username = 'OO_READER';
PROMPT cancel_check|done
EXIT
