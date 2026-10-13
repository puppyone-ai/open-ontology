-- DBA grants after the users exist. Least privilege. No ANY, no LOGMINING.
SET DEFINE OFF
SET FEEDBACK OFF
SET HEADING OFF
SET PAGESIZE 0
SET LINESIZE 200
WHENEVER SQLERROR EXIT SQL.SQLCODE

ALTER SESSION SET CONTAINER = FREEPDB1;

GRANT CREATE SESSION TO oo_reader;
GRANT SELECT ON oo_fix.t_number TO oo_reader;
GRANT SELECT ON oo_fix.t_date TO oo_reader;
GRANT SELECT ON oo_fix.t_text TO oo_reader;
GRANT SELECT ON oo_fix.t_ts TO oo_reader;
GRANT FLASHBACK ON oo_fix.t_number TO oo_reader;
GRANT FLASHBACK ON oo_fix.t_date TO oo_reader;
GRANT FLASHBACK ON oo_fix.t_text TO oo_reader;
GRANT FLASHBACK ON oo_fix.t_ts TO oo_reader;
GRANT EXECUTE ON sys.dbms_flashback TO oo_reader;

GRANT CREATE SESSION TO oo_noflash;
GRANT SELECT ON oo_fix.t_number TO oo_noflash;
GRANT SELECT ON oo_fix.t_date TO oo_noflash;
GRANT SELECT ON oo_fix.t_text TO oo_noflash;
GRANT SELECT ON oo_fix.t_ts TO oo_noflash;

CREATE AUDIT POLICY oo_dl01w1_10_acts ACTIONS ALL;
AUDIT POLICY oo_dl01w1_10_acts BY oo_reader, oo_noflash;

PROMPT grants|done
EXIT
