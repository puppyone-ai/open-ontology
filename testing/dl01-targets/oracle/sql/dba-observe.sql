-- DBA read of version, license-relevant parameters, privileges, undo, row SCN.
-- Does not query AWR, ASH, ADDM, or DBA_HIST views. Does not call LogMiner.
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
    DBMS_OUTPUT.PUT_LINE('banner|' || v);
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
SELECT 'param|' || name || '|' || value
FROM v$parameter
WHERE name IN (
  'undo_retention',
  'undo_management',
  'control_management_pack_access',
  'compatible',
  'cpu_count',
  'sga_target',
  'pga_aggregate_target',
  'memory_target',
  'os_authent_prefix'
);
SELECT 'undo_ts|' || tablespace_name || '|' || retention
FROM dba_tablespaces
WHERE contents = 'UNDO';
SELECT 'nls|' || parameter || '|' || value
FROM nls_database_parameters
WHERE parameter IN (
  'NLS_CHARACTERSET',
  'NLS_NCHAR_CHARACTERSET',
  'NLS_COMP',
  'NLS_SORT'
);
SELECT 'syspriv|' || grantee || '|' || privilege
FROM dba_sys_privs
WHERE grantee IN ('OO_READER', 'OO_NOFLASH', 'OO_FIX')
ORDER BY 1;
SELECT 'role|' || grantee || '|' || granted_role
FROM dba_role_privs
WHERE grantee IN ('OO_READER', 'OO_NOFLASH', 'OO_FIX')
ORDER BY 1;
SELECT 'tabpriv|' || grantee || '|' || privilege || '|' || owner || '|' || table_name
FROM dba_tab_privs
WHERE grantee IN ('OO_READER', 'OO_NOFLASH')
ORDER BY 1;
SELECT 'rowscn|t_number|' || MAX(ora_rowscn) FROM oo_fix.t_number;
SELECT 'rowscn|t_date|' || MAX(ora_rowscn) FROM oo_fix.t_date;
SELECT 'rowscn|t_text|' || MAX(ora_rowscn) FROM oo_fix.t_text;
SELECT 'rowscn|t_ts|' || MAX(ora_rowscn) FROM oo_fix.t_ts;

PROMPT observe|done
EXIT
