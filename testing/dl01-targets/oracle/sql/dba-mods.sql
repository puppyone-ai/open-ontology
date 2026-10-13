-- DBA monitoring flush and modification counters. Not a reader statement.
SET DEFINE OFF
SET FEEDBACK OFF
SET HEADING OFF
SET PAGESIZE 0
SET LINESIZE 200
SET SERVEROUTPUT ON
WHENEVER SQLERROR EXIT SQL.SQLCODE

ALTER SESSION SET CONTAINER = FREEPDB1;
BEGIN
  DBMS_STATS.FLUSH_DATABASE_MONITORING_INFO;
END;
/
SELECT 'mods|' || table_name || '|' || NVL(TO_CHAR(inserts), '0')
       || '|' || NVL(TO_CHAR(updates), '0')
       || '|' || NVL(TO_CHAR(deletes), '0')
FROM dba_tab_modifications
WHERE table_owner = 'OO_FIX'
ORDER BY table_name;
PROMPT mods|done
EXIT
