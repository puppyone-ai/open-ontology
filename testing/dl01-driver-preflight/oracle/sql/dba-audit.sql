-- Reader and noflash unified audit. SYS statement text is not selected.
SET DEFINE ON
SET FEEDBACK OFF
SET HEADING OFF
SET PAGESIZE 0
SET LINESIZE 2000
SET TRIMSPOOL ON
SET VERIFY OFF
WHENEVER SQLERROR EXIT SQL.SQLCODE

ALTER SESSION SET CONTAINER = FREEPDB1;
SELECT 'unified_auditing|' || value FROM v$option WHERE parameter = 'Unified Auditing';
SELECT 'audit|' || dbusername || '|' || action_name || '|' || return_code
  || '|' || NVL(object_schema, '-') || '|' || NVL(object_name, '-')
  || '|' || REPLACE(REPLACE(NVL(DBMS_LOB.SUBSTR(sql_text, 400, 1), '-'), CHR(10), ' '), CHR(13), ' ')
FROM unified_audit_trail
WHERE dbusername IN ('OO34_READER', 'OO34_NOFLASH')
  AND event_timestamp >= TO_TIMESTAMP('&1', 'YYYYMMDDHH24MISSFF6')
ORDER BY event_timestamp;
SELECT 'vsql|' || parsing_schema_name || '|' || executions || '|'
  || REPLACE(REPLACE(NVL(DBMS_LOB.SUBSTR(sql_fulltext, 400, 1), '-'), CHR(10), ' '), CHR(13), ' ')
FROM v$sql
WHERE parsing_schema_name IN ('OO34_READER', 'OO34_NOFLASH')
ORDER BY 1;
PROMPT audit|done
EXIT
