-- Post-grant SCN anchor. Fixture n1 for id 45 ends at 1.
-- ORA-01466 stays while SCN_TO_TIMESTAMP(scn) is at or before LAST_DDL_TIME.
-- A no-op update does not move ora_rowscn, and retrying that SCN cannot catch up.
-- Each try commits a real row change, then tests AS OF that new SCN.
SET DEFINE OFF
SET FEEDBACK OFF
SET HEADING OFF
SET PAGESIZE 0
SET LINESIZE 400
SET TAB OFF
WHENEVER SQLERROR EXIT SQL.SQLCODE

ALTER SESSION SET CONTAINER = FREEPDB1;

BEGIN EXECUTE IMMEDIATE 'DROP TABLE sys.oo34_anchor_log PURGE';
EXCEPTION WHEN OTHERS THEN NULL; END;
/
CREATE TABLE sys.oo34_anchor_log (id NUMBER, line VARCHAR2(400));

SELECT 'obj_ddl|' || object_type || '|' || object_name || '|'
       || TO_CHAR(last_ddl_time, 'YYYY-MM-DD HH24:MI:SS')
FROM dba_objects
WHERE owner = 'OO34_FIX'
ORDER BY object_name, object_type;

DECLARE
  v_ddl TIMESTAMP;
  v_scn NUMBER;
  v_ok NUMBER;
  v_try NUMBER := 0;
  v_mapped TIMESTAMP;
  v_map_text VARCHAR2(80);
  v_delta NUMBER;
  v_code NUMBER;
  v_line VARCHAR2(400);
BEGIN
  SELECT last_ddl_time INTO v_ddl
  FROM dba_objects
  WHERE owner = 'OO34_FIX' AND object_name = 'T_BATCH' AND object_type = 'TABLE';

  LOOP
    v_try := v_try + 1;
    UPDATE oo34_fix.t_batch SET n1 = 0 WHERE id = 45;
    UPDATE oo34_fix.t_batch SET n1 = 1 WHERE id = 45;
    COMMIT;
    SELECT MAX(ora_rowscn) INTO v_scn FROM oo34_fix.t_batch;
    v_map_text := 'unmap';
    v_delta := NULL;
    BEGIN
      SELECT SCN_TO_TIMESTAMP(v_scn) INTO v_mapped FROM dual;
      v_map_text := TO_CHAR(v_mapped, 'YYYY-MM-DD HH24:MI:SS');
      v_delta := (CAST(v_mapped AS DATE) - CAST(v_ddl AS DATE)) * 86400;
    EXCEPTION
      WHEN OTHERS THEN
        v_map_text := 'unmap|' || SQLCODE;
    END;
    BEGIN
      SELECT COUNT(*) INTO v_ok FROM oo34_fix.t_batch AS OF SCN v_scn;
      v_line := 'asof_ok|tries|' || v_try || '|scn|' || v_scn || '|count|' || v_ok
        || '|mapped|' || v_map_text || '|ddl|' || TO_CHAR(v_ddl, 'YYYY-MM-DD HH24:MI:SS')
        || '|delta_s|' || v_delta;
      INSERT INTO sys.oo34_anchor_log (id, line) VALUES (v_try, v_line);
      INSERT INTO sys.oo34_anchor_log (id, line) VALUES (1000, 'status|ok|scn|' || v_scn);
      COMMIT;
      EXIT;
    EXCEPTION
      WHEN OTHERS THEN
        v_code := SQLCODE;
        v_line := 'asof_try|tries|' || v_try || '|code|' || v_code || '|scn|' || v_scn
          || '|mapped|' || v_map_text || '|ddl|' || TO_CHAR(v_ddl, 'YYYY-MM-DD HH24:MI:SS')
          || '|delta_s|' || v_delta;
        INSERT INTO sys.oo34_anchor_log (id, line) VALUES (v_try, v_line);
        COMMIT;
        IF v_code != -1466 OR v_try >= 15 THEN
          INSERT INTO sys.oo34_anchor_log (id, line)
          VALUES (1000, 'status|fail|tries|' || v_try || '|code|' || v_code || '|scn|' || v_scn);
          COMMIT;
          EXIT;
        END IF;
        DBMS_SESSION.SLEEP(3);
    END;
  END LOOP;
END;
/

SELECT line FROM sys.oo34_anchor_log ORDER BY id;
SELECT 'anchor_n1|' || id || '|' || n1 FROM oo34_fix.t_batch WHERE id = 45;
SELECT 'anchor_current|' || current_scn FROM v$database;
SELECT 'anchor_get|' || DBMS_FLASHBACK.GET_SYSTEM_CHANGE_NUMBER FROM dual;

DECLARE
  v_ok NUMBER;
BEGIN
  SELECT COUNT(*) INTO v_ok FROM sys.oo34_anchor_log WHERE line LIKE 'status|ok%';
  EXECUTE IMMEDIATE 'DROP TABLE sys.oo34_anchor_log PURGE';
  IF v_ok = 0 THEN
    RAISE_APPLICATION_ERROR(-20001, 'asof_fail');
  END IF;
END;
/
PROMPT anchor|done
EXIT
