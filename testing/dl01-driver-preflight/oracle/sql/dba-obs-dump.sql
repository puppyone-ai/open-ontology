SET DEFINE OFF
SET FEEDBACK OFF
SET HEADING OFF
SET PAGESIZE 0
SET LINESIZE 500
SET TRIMSPOOL ON
WHENEVER SQLERROR EXIT SQL.SQLCODE
ALTER SESSION SET CONTAINER = FREEPDB1;
SELECT 'obs|' || id || '|' || sample_no || '|' || username || '|' || NVL(TO_CHAR(sid), '-')
  || '|' || NVL(TO_CHAR(serial_num), '-') || '|' || NVL(status, '-')
  || '|' || NVL(module, '-') || '|' || REPLACE(REPLACE(NVL(sql_text, '-'), CHR(10), ' '), '|', '/')
FROM sys.oo34_obs
ORDER BY id;
PROMPT obs|done
EXIT
