-- Fallback cancel target if the cartesian read finishes before cancel.
SET DEFINE OFF
SET FEEDBACK OFF
SET HEADING OFF
SET PAGESIZE 0
SET SERVEROUTPUT ON SIZE UNLIMITED
WHENEVER SQLERROR EXIT SQL.SQLCODE

BEGIN
  /* oo-dl01w1-10-cancel */
  DBMS_SESSION.SLEEP(60);
END;
/
PROMPT reader_cancel_sleep|finished
EXIT
