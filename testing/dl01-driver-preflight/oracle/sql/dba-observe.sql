-- DBA read of banner, parameters, privileges. No AWR, ASH, DBA_HIST, or LogMiner.
SET DEFINE OFF
SET SERVEROUTPUT ON SIZE UNLIMITED
SET FEEDBACK OFF
SET HEADING OFF
SET PAGESIZE 0
SET LINESIZE 500
SET TRIMSPOOL ON
SET VERIFY OFF
WHENEVER SQLERROR EXIT SQL.SQLCODE

ALTER SESSION SET CONTAINER = FREEPDB1;

SELECT 'banner|' || banner FROM v$version;
DECLARE
  c SYS_REFCURSOR;
  v VARCHAR2(512);
BEGIN
  OPEN c FOR 'SELECT banner_full FROM v$version';
  LOOP
    FETCH c INTO v;
    EXIT WHEN c%NOTFOUND;
    DBMS_OUTPUT.PUT_LINE('banner_full|' || v);
  END LOOP;
  CLOSE c;
EXCEPTION
  WHEN OTHERS THEN
    DBMS_OUTPUT.PUT_LINE('banner_full_error|' || SQLCODE);
END;
/
SELECT 'db|' || name || '|' || open_mode || '|' || cdb || '|' || log_mode
       || '|' || supplemental_log_data_min
FROM v$database;
SELECT 'pdb|' || name || '|' || open_mode FROM v$pdbs;
SELECT 'home|' || SYS_CONTEXT('USERENV', 'ORACLE_HOME') FROM dual;
SELECT 'param|' || name || '|' || value
FROM v$parameter
WHERE name IN (
  'undo_retention',
  'undo_management',
  'control_management_pack_access',
  'compatible',
  'cpu_count'
);
SELECT 'syspriv|' || grantee || '|' || privilege
FROM dba_sys_privs
WHERE grantee IN ('OO34_READER', 'OO34_NOFLASH', 'OO34_FIX')
ORDER BY 1;
SELECT 'role|' || grantee || '|' || granted_role
FROM dba_role_privs
WHERE grantee IN ('OO34_READER', 'OO34_NOFLASH', 'OO34_FIX')
ORDER BY 1;
SELECT 'tabpriv|' || grantee || '|' || privilege || '|' || owner || '|' || table_name
FROM dba_tab_privs
WHERE grantee IN ('OO34_READER', 'OO34_NOFLASH')
ORDER BY 1;
SELECT 'quota|' || username || '|' || tablespace_name || '|' || max_bytes
FROM dba_ts_quotas
WHERE username IN ('OO34_READER', 'OO34_NOFLASH', 'OO34_FIX')
ORDER BY 1;
PROMPT observe|done
EXIT
