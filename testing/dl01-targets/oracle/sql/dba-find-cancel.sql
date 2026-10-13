SET DEFINE OFF
SET FEEDBACK OFF
SET HEADING OFF
SET PAGESIZE 0
SET LINESIZE 300
WHENEVER SQLERROR EXIT SQL.SQLCODE

ALTER SESSION SET CONTAINER = FREEPDB1;
SELECT 'active|' || s.sid || '|' || s.serial# || '|' || s.status || '|' || NVL(s.sql_id, '-')
FROM v$session s
WHERE s.username = 'OO_READER'
  AND s.status = 'ACTIVE';
SELECT 'marker|' || s.sid || '|' || s.serial# || '|' || s.status
FROM v$session s
JOIN v$sql q ON q.sql_id = s.sql_id
WHERE s.username = 'OO_READER'
  AND q.sql_text LIKE '%oo-dl01w1-10-cancel%';
SELECT 'sess|' || sid || '|' || serial# || '|' || status || '|' || NVL(sql_id, '-')
FROM v$session
WHERE username = 'OO_READER';
PROMPT find_cancel|done
EXIT
