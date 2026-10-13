-- Reader and noflash unified audit only. SYS statement text is not selected.
SET DEFINE ON
SET FEEDBACK OFF
SET HEADING OFF
SET PAGESIZE 0
SET LINESIZE 500
SET TRIMSPOOL ON
SET VERIFY OFF
WHENEVER SQLERROR EXIT SQL.SQLCODE

ALTER SESSION SET CONTAINER = FREEPDB1;
-- 26ai writes unified audit rows directly. FLUSH_UNIFIED_AUDIT_TRAIL is desupported.
SELECT 'unified_auditing|' || value FROM v$option WHERE parameter = 'Unified Auditing';
SELECT 'audit|' || dbusername || '|' || action_name || '|' || return_code || '|' || COUNT(*)
FROM unified_audit_trail
WHERE dbusername IN ('OO_READER', 'OO_NOFLASH')
  AND event_timestamp >= TO_TIMESTAMP('&1', 'YYYYMMDDHH24MISSFF3')
GROUP BY dbusername, action_name, return_code
ORDER BY 1;
PROMPT audit|done
EXIT
