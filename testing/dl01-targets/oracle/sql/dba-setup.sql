-- DBA identity only. Synthetic fixture OO_FIX. Not the reader journey.
SET SERVEROUTPUT ON SIZE UNLIMITED
SET DEFINE OFF
SET FEEDBACK OFF
SET HEADING OFF
SET PAGESIZE 0
SET LINESIZE 400
SET TRIMSPOOL ON
SET VERIFY OFF
WHENEVER SQLERROR EXIT SQL.SQLCODE

ALTER SESSION SET CONTAINER = FREEPDB1;
ALTER PROFILE DEFAULT LIMIT
  PASSWORD_VERIFY_FUNCTION NULL
  PASSWORD_LIFE_TIME UNLIMITED
  FAILED_LOGIN_ATTEMPTS UNLIMITED;

BEGIN
  EXECUTE IMMEDIATE 'NOAUDIT POLICY oo_dl01w1_10_acts';
EXCEPTION
  WHEN OTHERS THEN
    DBMS_OUTPUT.PUT_LINE('setup|noaudit|' || SQLCODE);
END;
/

BEGIN
  EXECUTE IMMEDIATE 'DROP AUDIT POLICY oo_dl01w1_10_acts';
EXCEPTION
  WHEN OTHERS THEN
    DBMS_OUTPUT.PUT_LINE('setup|drop_policy|' || SQLCODE);
END;
/

BEGIN EXECUTE IMMEDIATE 'DROP USER oo_reader CASCADE';
EXCEPTION WHEN OTHERS THEN IF SQLCODE != -1918 THEN RAISE; END IF; END;
/
BEGIN EXECUTE IMMEDIATE 'DROP USER oo_noflash CASCADE';
EXCEPTION WHEN OTHERS THEN IF SQLCODE != -1918 THEN RAISE; END IF; END;
/
BEGIN EXECUTE IMMEDIATE 'DROP USER oo_fix CASCADE';
EXCEPTION WHEN OTHERS THEN IF SQLCODE != -1918 THEN RAISE; END IF; END;
/

CREATE USER oo_fix IDENTIFIED EXTERNALLY ACCOUNT LOCK QUOTA UNLIMITED ON USERS;

CREATE TABLE oo_fix.t_number (
  id NUMBER PRIMARY KEY,
  n_free NUMBER,
  n_bounded NUMBER(38, 10)
);

CREATE TABLE oo_fix.t_date (
  id NUMBER PRIMARY KEY,
  d DATE NOT NULL
);

CREATE TABLE oo_fix.t_text (
  id NUMBER PRIMARY KEY,
  v2 VARCHAR2(20),
  c4 CHAR(4)
);

CREATE TABLE oo_fix.t_ts (
  id NUMBER PRIMARY KEY,
  ts TIMESTAMP(9) NOT NULL
);

INSERT INTO oo_fix.t_number (id, n_free, n_bounded) VALUES (1, NULL, NULL);
INSERT INTO oo_fix.t_number (id, n_free, n_bounded) VALUES (2, 0, 0);
INSERT INTO oo_fix.t_number (id, n_free, n_bounded) VALUES (3, 1, 1);
INSERT INTO oo_fix.t_number (id, n_free, n_bounded) VALUES (4, -1, -1);
INSERT INTO oo_fix.t_number (id, n_free, n_bounded) VALUES (5, 0.1, 0.1);
INSERT INTO oo_fix.t_number (id, n_free, n_bounded) VALUES (6, 9007199254740993, 9007199254740993);
INSERT INTO oo_fix.t_number (id, n_free, n_bounded) VALUES (7, 99999999999999999999999999999999999999, NULL);
INSERT INTO oo_fix.t_number (id, n_free, n_bounded) VALUES (8, -99999999999999999999999999999999999999, NULL);
INSERT INTO oo_fix.t_number (id, n_free, n_bounded) VALUES (9, 1E-130, NULL);
INSERT INTO oo_fix.t_number (id, n_free, n_bounded) VALUES (10, 9.9999999999999999999999999999999999999E125, NULL);

INSERT INTO oo_fix.t_date (id, d) VALUES (1, DATE '2024-03-15');
INSERT INTO oo_fix.t_date (id, d) VALUES (2, TO_DATE('2024-03-15 13:45:59', 'YYYY-MM-DD HH24:MI:SS'));
INSERT INTO oo_fix.t_date (id, d) VALUES (3, TO_DATE('2024-03-15 00:00:01', 'YYYY-MM-DD HH24:MI:SS'));

INSERT INTO oo_fix.t_text (id, v2, c4) VALUES (1, '', '');
INSERT INTO oo_fix.t_text (id, v2, c4) VALUES (2, NULL, NULL);
INSERT INTO oo_fix.t_text (id, v2, c4) VALUES (3, ' ', ' ');
INSERT INTO oo_fix.t_text (id, v2, c4) VALUES (4, 'abc', 'A');

INSERT INTO oo_fix.t_ts (id, ts) VALUES (1, TIMESTAMP '2024-03-15 13:45:59.123456789');
INSERT INTO oo_fix.t_ts (id, ts) VALUES (2, TIMESTAMP '2024-03-15 13:45:59.123456000');
INSERT INTO oo_fix.t_ts (id, ts) VALUES (3, TIMESTAMP '9999-12-31 23:59:59.999999999');

COMMIT;

-- A PL/SQL numeric literal 1E126 does not compile (PLS-00569, preserved in
-- attempts/1). These probes are runtime conversions. Tokens are selected.
VARIABLE c126 NUMBER
VARIABLE t126 VARCHAR2(200)
VARIABLE c131 NUMBER
VARIABLE t131 VARCHAR2(200)
VARIABLE c127 NUMBER
VARIABLE t127 VARCHAR2(200)
VARIABLE c_high NUMBER
VARIABLE t_high VARCHAR2(200)
VARIABLE c_low NUMBER
VARIABLE t_low VARCHAR2(200)
VARIABLE c_pow126 NUMBER
VARIABLE t_pow126 VARCHAR2(200)
VARIABLE c_pow130 NUMBER
VARIABLE t_pow130 VARCHAR2(200)
VARIABLE ov_scale NUMBER
BEGIN
  :c126 := 0; :t126 := '<unset>';
  :c131 := 0; :t131 := '<unset>';
  :c127 := 0; :t127 := '<unset>';
  :c_high := 0; :t_high := '<unset>';
  :c_low := 0; :t_low := '<unset>';
  :c_pow126 := 0; :t_pow126 := '<unset>';
  :c_pow130 := 0; :t_pow130 := '<unset>';
  :ov_scale := 0;
END;
/
DECLARE
  n NUMBER;
BEGIN
  n := TO_NUMBER('1E126');
  :t126 := TO_CHAR(n, 'TM9');
EXCEPTION
  WHEN OTHERS THEN
    :c126 := SQLCODE;
    :t126 := SQLERRM;
END;
/
DECLARE
  n NUMBER;
BEGIN
  n := TO_NUMBER('1E-131');
  :t131 := TO_CHAR(n, 'TM9');
EXCEPTION
  WHEN OTHERS THEN
    :c131 := SQLCODE;
    :t131 := SQLERRM;
END;
/
DECLARE
  n NUMBER;
BEGIN
  n := TO_NUMBER('1E127');
  :t127 := TO_CHAR(n, 'TM9');
EXCEPTION
  WHEN OTHERS THEN
    :c127 := SQLCODE;
    :t127 := SQLERRM;
END;
/
DECLARE
  n NUMBER;
BEGIN
  n := TO_NUMBER('9.9999999999999999999999999999999999999E125') * 10;
  :t_high := TO_CHAR(n, 'TM9');
EXCEPTION
  WHEN OTHERS THEN
    :c_high := SQLCODE;
    :t_high := SQLERRM;
END;
/
DECLARE
  n NUMBER;
BEGIN
  n := TO_NUMBER('1E-130') / 100;
  :t_low := TO_CHAR(n, 'TM9');
EXCEPTION
  WHEN OTHERS THEN
    :c_low := SQLCODE;
    :t_low := SQLERRM;
END;
/
DECLARE
  n NUMBER;
BEGIN
  n := POWER(10, 126);
  :t_pow126 := TO_CHAR(n, 'TM9');
EXCEPTION
  WHEN OTHERS THEN
    :c_pow126 := SQLCODE;
    :t_pow126 := SQLERRM;
END;
/
DECLARE
  n NUMBER;
BEGIN
  n := POWER(10, 130);
  :t_pow130 := TO_CHAR(n, 'TM9');
EXCEPTION
  WHEN OTHERS THEN
    :c_pow130 := SQLCODE;
    :t_pow130 := SQLERRM;
END;
/
DECLARE
  n NUMBER;
BEGIN
  -- NUMBER(38,10) allows 28 digits left of the decimal. 10^28 does not fit.
  EXECUTE IMMEDIATE 'INSERT INTO oo_fix.t_number (id, n_free, n_bounded) VALUES (90, NULL, 10000000000000000000000000000)';
  ROLLBACK;
EXCEPTION
  WHEN OTHERS THEN
    :ov_scale := SQLCODE;
END;
/
COLUMN overflow_line FORMAT A240
SELECT 'overflow|e126|' || :c126 || '|' || :t126 AS overflow_line FROM dual;
SELECT 'overflow|e131|' || :c131 || '|' || :t131 AS overflow_line FROM dual;
SELECT 'overflow|e127|' || :c127 || '|' || :t127 AS overflow_line FROM dual;
SELECT 'overflow|high|' || :c_high || '|' || :t_high AS overflow_line FROM dual;
SELECT 'overflow|low|' || :c_low || '|' || :t_low AS overflow_line FROM dual;
SELECT 'overflow|pow126|' || :c_pow126 || '|' || :t_pow126 AS overflow_line FROM dual;
SELECT 'overflow|pow130|' || :c_pow130 || '|' || :t_pow130 AS overflow_line FROM dual;
SELECT 'overflow|p4s2|' || :ov_scale AS overflow_line FROM dual;

PROMPT setup|fixture|committed
EXIT
