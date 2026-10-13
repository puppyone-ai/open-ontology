-- DBA mutation, labeled separately from the reader. Captures the SCN first.
SET DEFINE OFF
SET FEEDBACK OFF
SET HEADING OFF
SET PAGESIZE 0
SET LINESIZE 200
WHENEVER SQLERROR EXIT SQL.SQLCODE

ALTER SESSION SET CONTAINER = FREEPDB1;
SELECT 'scn_before|' || DBMS_FLASHBACK.GET_SYSTEM_CHANGE_NUMBER FROM dual;
SELECT 'current_scn|' || current_scn FROM v$database;
SELECT 'obj|' || object_name || '|' || TO_CHAR(last_ddl_time, 'YYYY-MM-DD HH24:MI:SS')
FROM dba_objects
WHERE owner = 'OO_FIX'
  AND object_type = 'TABLE'
ORDER BY object_name;
UPDATE oo_fix.t_date
   SET d = TO_DATE('2024-03-15 23:59:58', 'YYYY-MM-DD HH24:MI:SS')
 WHERE id = 2;
COMMIT;
SELECT 'scn_after|' || DBMS_FLASHBACK.GET_SYSTEM_CHANGE_NUMBER FROM dual;
SELECT 'date_now|' || TO_CHAR(d, 'YYYY-MM-DD HH24:MI:SS')
FROM oo_fix.t_date
WHERE id = 2;
PROMPT mutate|done
EXIT
