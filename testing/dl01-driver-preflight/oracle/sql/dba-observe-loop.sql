-- Long-lived DBA sampler. Samples are committed so a later dump sees them.
SET SERVEROUTPUT ON SIZE UNLIMITED
SET DEFINE OFF
SET FEEDBACK OFF
SET HEADING OFF
SET PAGESIZE 0
WHENEVER SQLERROR EXIT SQL.SQLCODE

ALTER SESSION SET CONTAINER = FREEPDB1;
BEGIN
  FOR i IN 1..720 LOOP
    FOR stop_row IN (SELECT 1 FROM sys.oo34_obs WHERE username = 'STOP' AND ROWNUM = 1) LOOP
      RETURN;
    END LOOP;
    INSERT INTO sys.oo34_obs (sample_no, username, sid, serial_num, status, module, sql_text)
    SELECT i,
           s.username,
           s.sid,
           s.serial#,
           s.status,
           SUBSTR(s.module, 1, 64),
           (SELECT SUBSTR(q.sql_text, 1, 360)
              FROM v$sql q
             WHERE q.sql_id = s.sql_id
               AND ROWNUM = 1)
      FROM v$session s
     WHERE s.username IN ('OO34_READER', 'OO34_NOFLASH');
    INSERT INTO sys.oo34_obs (sample_no, username, status, sql_text)
    VALUES (i, 'TICK', 'TICK', TO_CHAR(i));
    COMMIT;
    DBMS_SESSION.SLEEP(0.25);
  END LOOP;
END;
/
PROMPT observe_loop|done
EXIT
