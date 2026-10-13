-- Least-privilege reader. SELECT only.
SET DEFINE OFF
SET FEEDBACK OFF
SET HEADING OFF
SET PAGESIZE 0
SET LINESIZE 500
SET TRIMSPOOL ON
SET VERIFY OFF
WHENEVER SQLERROR EXIT SQL.SQLCODE

SELECT 'who|' || SYS_CONTEXT('USERENV', 'SESSION_USER')
       || '|' || SYS_CONTEXT('USERENV', 'CON_NAME')
FROM dual;

SELECT 'empty_literal|' || CASE WHEN '' IS NULL THEN 'Y' ELSE 'N' END FROM dual;

SELECT 'num|' || id
       || '|null=' || CASE WHEN n_free IS NULL THEN 'Y' ELSE 'N' END
       || '|eq=' || CASE
            WHEN id = 1 AND n_free IS NULL THEN 'Y'
            WHEN id = 2 AND n_free = 0 THEN 'Y'
            WHEN id = 3 AND n_free = 1 THEN 'Y'
            WHEN id = 4 AND n_free = -1 THEN 'Y'
            WHEN id = 5 AND n_free = 0.1 THEN 'Y'
            WHEN id = 6 AND n_free = 9007199254740993 THEN 'Y'
            WHEN id = 7 AND n_free = 99999999999999999999999999999999999999 THEN 'Y'
            WHEN id = 8 AND n_free = -99999999999999999999999999999999999999 THEN 'Y'
            WHEN id = 9 AND n_free = 1E-130 THEN 'Y'
            WHEN id = 10 AND n_free = 9.9999999999999999999999999999999999999E125 THEN 'Y'
            ELSE 'N'
          END
       || '|text=' || NVL(TO_CHAR(n_free, 'TM9', 'NLS_NUMERIC_CHARACTERS=''.,'''), '<NULL>')
FROM oo_fix.t_number
WHERE id BETWEEN 1 AND 10
ORDER BY id;

SELECT 'f64|' || id
       || '|' || CASE
            WHEN n_free = CAST(CAST(n_free AS BINARY_DOUBLE) AS NUMBER) THEN 'Y'
            ELSE 'N'
          END
FROM oo_fix.t_number
WHERE id IN (5, 6)
ORDER BY id;

SELECT 'date|' || id || '|' || TO_CHAR(d, 'YYYY-MM-DD HH24:MI:SS')
FROM oo_fix.t_date
ORDER BY id;

SELECT 'text|' || id
       || '|v2_null=' || CASE WHEN v2 IS NULL THEN 'Y' ELSE 'N' END
       || '|v2_len=' || NVL(TO_CHAR(LENGTH(v2)), '<NULL>')
       || '|c4_null=' || CASE WHEN c4 IS NULL THEN 'Y' ELSE 'N' END
       || '|c4_len=' || NVL(TO_CHAR(LENGTH(c4)), '<NULL>')
       || '|c4=' || NVL(REPLACE(c4, ' ', '.'), '<NULL>')
FROM oo_fix.t_text
ORDER BY id;

SELECT 'ts|' || id || '|' || TO_CHAR(ts, 'YYYY-MM-DD HH24:MI:SS.FF9')
FROM oo_fix.t_ts
ORDER BY id;

SELECT 'scn_now|' || DBMS_FLASHBACK.GET_SYSTEM_CHANGE_NUMBER FROM dual;

PROMPT reader_positive|done
EXIT
