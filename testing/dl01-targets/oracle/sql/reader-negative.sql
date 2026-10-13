-- Necessary negative: reader DML and DDL must fail. No LogMiner call.
SET SERVEROUTPUT ON SIZE UNLIMITED
SET DEFINE OFF
SET FEEDBACK OFF
SET HEADING OFF
SET PAGESIZE 0

DECLARE
  PROCEDURE try(label VARCHAR2, stmt VARCHAR2) IS
  BEGIN
    EXECUTE IMMEDIATE stmt;
    DBMS_OUTPUT.PUT_LINE('neg|' || label || '|accepted');
  EXCEPTION
    WHEN OTHERS THEN
      DBMS_OUTPUT.PUT_LINE('neg|' || label || '|rejected|' || SQLCODE || '|' || SQLERRM);
  END;
BEGIN
  try('insert', 'INSERT INTO oo_fix.t_text (id, v2) VALUES (99, ''x'')');
  try('update', 'UPDATE oo_fix.t_text SET v2 = ''z'' WHERE id = 4');
  try('delete', 'DELETE FROM oo_fix.t_text WHERE id = 4');
  try('create_table', 'CREATE TABLE oo_fix.t_evil (id NUMBER)');
  try('alter_database', 'ALTER DATABASE ADD SUPPLEMENTAL LOG DATA');
  try('grant_any', 'GRANT SELECT ANY TABLE TO oo_reader');
  ROLLBACK;
END;
/
PROMPT reader_negative|done
EXIT
